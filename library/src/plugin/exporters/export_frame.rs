use crate::core::rendering::managed_color_backend::ProjectColorAuthority;
use crate::error::LibraryError;
use crate::model::authoring::AuthoringProject;
use crate::model::frame::Image;
use crate::model::project::{Project, ResolvedColorManagementConfig};

/// Exact color and pixel-storage semantics of a rendered export frame.
///
/// The current encoder boundary deliberately has one supported variant. New
/// output spaces, HDR transfer functions, or higher bit depths must add a new
/// typed variant and a matching encoder implementation; they must never be
/// reinterpreted as this sRGB contract.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ExportColorAuthority {
    /// SDR sRGB, BT.709 primaries, sRGB transfer, full-range RGB, straight
    /// alpha, and unsigned 8-bit RGBA storage.
    SdrSrgbFullRangeStraightRgba8 {
        /// Stable identity of the exact Project color pipeline that produced
        /// the bytes. This includes config identity, exact sRGB surface
        /// binding, output space, and HDR context.
        pipeline_identity: String,
    },
}

impl ExportColorAuthority {
    pub(crate) fn from_project(project: &Project) -> Result<Self, LibraryError> {
        Self::from_authority(project)
    }

    pub(super) fn from_authority(
        project: &dyn ProjectColorAuthority,
    ) -> Result<Self, LibraryError> {
        let intent = match project.resolved_color_management() {
            ResolvedColorManagementConfig::Ready(intent) => intent,
            ResolvedColorManagementConfig::Unavailable { diagnostics, .. } => {
                return Err(LibraryError::Render(format!(
                    "cannot establish export color authority: {}",
                    diagnostics
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("; ")
                )));
            }
        };
        let config = intent.config();
        let output_space = config.export().output_space();
        let surface_binding = intent.srgb_surface_space().map_err(|issue| {
            LibraryError::Render(format!(
                "cannot establish config-bound sRGB export authority: {issue}"
            ))
        })?;
        if output_space != surface_binding.color_space() {
            return Err(LibraryError::Render(format!(
                "export output space '{output_space}' is not the active config's explicitly bound sRGB surface space '{}'",
                surface_binding.color_space()
            )));
        }
        Ok(Self::SdrSrgbFullRangeStraightRgba8 {
            pipeline_identity: intent.cache_identity().to_string(),
        })
    }

    pub const fn description(&self) -> &'static str {
        match self {
            Self::SdrSrgbFullRangeStraightRgba8 { .. } => {
                "SDR sRGB/BT.709 primaries, sRGB transfer, full-range straight RGBA8"
            }
        }
    }

    pub fn pipeline_identity(&self) -> &str {
        match self {
            Self::SdrSrgbFullRangeStraightRgba8 { pipeline_identity } => pipeline_identity,
        }
    }
}

/// Rendered pixels paired with the Project-derived authority that gives those
/// bytes meaning at an exporter boundary.
#[derive(Clone, Debug)]
pub struct ExportFrame {
    image: Image,
    color_authority: ExportColorAuthority,
}

impl ExportFrame {
    pub(crate) fn from_project_render(
        project: &Project,
        image: Image,
    ) -> Result<Self, LibraryError> {
        let color_authority = ExportColorAuthority::from_project(project)?;
        Self::new_verified(image, color_authority)
    }

    pub(crate) fn from_authoring_render(
        project: &AuthoringProject,
        image: Image,
    ) -> Result<Self, LibraryError> {
        let color_authority = ExportColorAuthority::from_authority(project)?;
        Self::new_verified(image, color_authority)
    }

    fn new_verified(
        image: Image,
        color_authority: ExportColorAuthority,
    ) -> Result<Self, LibraryError> {
        if image.width == 0 || image.height == 0 {
            return Err(LibraryError::Render(
                "export frame dimensions must be non-zero".to_string(),
            ));
        }
        let expected_bytes = u64::from(image.width)
            .checked_mul(u64::from(image.height))
            .and_then(|pixels| pixels.checked_mul(4))
            .and_then(|bytes| usize::try_from(bytes).ok())
            .ok_or_else(|| LibraryError::Render("export frame size overflows".to_string()))?;
        if image.data.len() != expected_bytes {
            return Err(LibraryError::Render(format!(
                "export frame declares {}x{} RGBA8 pixels but contains {} bytes instead of {expected_bytes}",
                image.width,
                image.height,
                image.data.len()
            )));
        }
        Ok(Self {
            image,
            color_authority,
        })
    }

    pub fn image(&self) -> &Image {
        &self.image
    }

    pub const fn color_authority(&self) -> &ExportColorAuthority {
        &self.color_authority
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::project::{
        ColorConfigIdentity, ColorManagementConfig, ExportColorConfig, PreviewColorConfig,
        PreviewSurfaceEncoding,
    };

    #[test]
    fn current_boundary_accepts_only_exact_srgb_output() {
        let project = Project::new("sRGB export");
        assert!(matches!(
            ExportColorAuthority::from_project(&project).unwrap(),
            ExportColorAuthority::SdrSrgbFullRangeStraightRgba8 { .. }
        ));

        let mut unsupported = project;
        let default = ColorManagementConfig::default();
        let config = ColorManagementConfig::new(
            default.config().clone(),
            default.working_space(),
            default.preview().clone(),
            ExportColorConfig::new("display-p3"),
        );
        unsupported.set_color_management(config).unwrap();
        let error = ExportColorAuthority::from_project(&unsupported).unwrap_err();
        assert!(error.to_string().contains("display-p3"));
        assert!(error.to_string().contains("explicitly bound sRGB"));
    }

    #[test]
    fn malformed_rgba_storage_cannot_become_an_export_frame() {
        let project = Project::new("bad frame");
        let image = Image::new(2, 2, vec![0; 15]);
        let error = ExportFrame::from_project_render(&project, image).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("contains 15 bytes instead of 16")
        );
    }

    #[test]
    fn custom_config_literal_srgb_is_not_authority_without_exact_binding() {
        let custom_identity = ColorConfigIdentity::OcioBuiltin {
            uri: "ocio://studio-config-v2.2.0_aces-v1.3_ocio-v2.4".to_string(),
            ocio_version: "2.5.2".to_string(),
        };
        let unbound = ColorManagementConfig::new(
            custom_identity.clone(),
            "scene_linear",
            PreviewColorConfig::named_view("sRGB", "Display", "srgb", PreviewSurfaceEncoding::Srgb),
            ExportColorConfig::new("srgb"),
        );
        let mut raw = serde_json::to_value(Project::new("malicious literal")).unwrap();
        raw["color_management"] = serde_json::to_value(unbound).unwrap();
        let project: Project = serde_json::from_value(raw).unwrap();
        let error = ExportColorAuthority::from_project(&project).unwrap_err();
        assert!(
            error.to_string().contains("no exact sRGB"),
            "unexpected authority error: {error}"
        );

        let mut explicitly_bound = Project::new("bound custom config");
        let bound = ColorManagementConfig::new(
            custom_identity,
            "scene_linear",
            PreviewColorConfig::named_view("sRGB", "Display", "srgb", PreviewSurfaceEncoding::Srgb),
            ExportColorConfig::new("srgb"),
        )
        .with_srgb_surface_space("srgb");
        explicitly_bound.set_color_management(bound).unwrap();
        assert!(matches!(
            ExportColorAuthority::from_project(&explicitly_bound).unwrap(),
            ExportColorAuthority::SdrSrgbFullRangeStraightRgba8 { .. }
        ));
    }

    #[test]
    fn authority_token_retains_exact_custom_pipeline_identity() {
        let project_for_uri = |uri: &str| {
            let mut project = Project::new(uri);
            let config = ColorManagementConfig::new(
                ColorConfigIdentity::OcioBuiltin {
                    uri: uri.to_string(),
                    ocio_version: "2.5.2".to_string(),
                },
                "scene_linear",
                PreviewColorConfig::named_view(
                    "sRGB",
                    "Display",
                    "surface_srgb",
                    PreviewSurfaceEncoding::Srgb,
                ),
                ExportColorConfig::new("surface_srgb"),
            )
            .with_srgb_surface_space("surface_srgb");
            project.set_color_management(config).unwrap();
            project
        };
        let first =
            ExportColorAuthority::from_project(&project_for_uri("ocio://config/first_ocio-v2.5.2"))
                .unwrap();
        let second = ExportColorAuthority::from_project(&project_for_uri(
            "ocio://config/second_ocio-v2.5.2",
        ))
        .unwrap();

        assert_ne!(first.pipeline_identity(), second.pipeline_identity());
        assert_ne!(first, second);
    }

    #[test]
    fn timeline_first_project_produces_typed_export_authority() {
        let project = AuthoringProject::new("export", 1, 1, 24.0, 1.0).expect("Project");
        let frame =
            ExportFrame::from_authoring_render(&project, Image::new(1, 1, vec![10, 20, 30, 255]))
                .expect("typed export frame");
        assert!(matches!(
            frame.color_authority(),
            ExportColorAuthority::SdrSrgbFullRangeStraightRgba8 { .. }
        ));
    }
}
