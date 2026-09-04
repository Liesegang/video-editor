//! Color-managed stills used by editor Asset previews.
//!
//! This is deliberately a thin composition of the same loader, decoded-media
//! validation, Project color pipeline, and terminal conversion used by the
//! production renderer. It owns no decoder, renderer, or cache of its own.

use crate::cache::CacheManager;
use crate::core::rendering::managed_color_backend::{
    ManagedRenderDestination, ProjectColorPipeline,
};
use crate::core::rendering::managed_color_source::ingest_loaded_media_from_assets;
use crate::core::rendering::media_color_ingress::MediaAssetKind;
use crate::error::LibraryError;
use crate::model::asset::AssetKind;
use crate::model::authoring::AuthoringProject;
use crate::model::frame::Image;
use crate::model::frame::entity::ImageSurface;
use crate::model::frame::transform::Transform;
use crate::plugin::{LoadRequest, PluginManager};

/// Decode and color-manage one visual Asset into the Project's Preview surface.
///
/// `source_time` is used only for Video Assets. Callers pass the same shared
/// cache and plugin registry as Timeline Preview, so this path cannot establish
/// a second media cache or choose a different decoder.
pub fn load_asset_preview_frame(
    project: &AuthoringProject,
    asset_id: uuid::Uuid,
    source_time: f64,
    plugins: &PluginManager,
    cache: &CacheManager,
) -> Result<Image, LibraryError> {
    let asset = project
        .assets
        .iter()
        .find(|asset| asset.id == asset_id)
        .ok_or_else(|| LibraryError::Validation(format!("Asset {asset_id} was not found")))?;
    let (request, expected_kind) = match asset.kind {
        AssetKind::Image => (
            LoadRequest::Image {
                path: asset.path.clone(),
            },
            MediaAssetKind::Image,
        ),
        AssetKind::Video => {
            if !source_time.is_finite() || source_time < 0.0 {
                return Err(LibraryError::Validation(format!(
                    "Asset preview time must be finite and non-negative, not {source_time}"
                )));
            }
            (
                LoadRequest::VideoFrame {
                    path: asset.path.clone(),
                    source_time,
                    stream_index: asset.stream_index,
                    source_color_authority: asset.source_color.decoder_color_authority(),
                },
                MediaAssetKind::Video,
            )
        }
        AssetKind::Audio | AssetKind::Model3D | AssetKind::Other => {
            return Err(LibraryError::Validation(format!(
                "Asset {asset_id} has no visual preview frame"
            )));
        }
    };

    let pipeline =
        ProjectColorPipeline::for_authoring_project(project, ManagedRenderDestination::Preview)?;
    let response = plugins.load_resource(&request, cache)?;
    let surface = ImageSurface {
        asset_id: Some(asset.id),
        file_path: asset.path.clone(),
        effects: Vec::new(),
        input_color_space: None,
        output_color_space: None,
        transform: Transform::default(),
    };
    let working = ingest_loaded_media_from_assets(
        &project.assets,
        &pipeline,
        &surface,
        expected_kind,
        response,
    )?;
    pipeline.terminal_image(&working)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use crate::cache::CacheManager;
    use crate::model::asset::{
        Asset, AssetKind, SourceColorDescription, SourceColorPrimaries,
        SourceTransferCharacteristic,
    };
    use crate::model::authoring::{AuthoringProject, MediaTime, RationalRate};
    use crate::plugin::{
        DecodedColorSpace, DecodedPixelBuffer, DecodedPixelDescription, DecodedStraightRgba32F,
        LoadPlugin, LoadPluginError, LoadPluginResult, LoadRequest, LoadResponse, Plugin,
        PluginManager,
    };

    use super::load_asset_preview_frame;

    #[derive(Clone, Copy)]
    enum ResponseColor {
        Srgb,
        DisplayP3,
    }

    struct FloatLoader {
        color: ResponseColor,
        request: Arc<Mutex<Option<LoadRequest>>>,
    }

    impl Plugin for FloatLoader {
        fn id(&self) -> &str {
            "asset-preview-float-loader"
        }

        fn name(&self) -> String {
            "Asset preview float test loader".to_string()
        }

        fn category(&self) -> String {
            "Tests".to_string()
        }

        fn version(&self) -> (u32, u32, u32) {
            (0, 1, 0)
        }
    }

    impl LoadPlugin for FloatLoader {
        fn open(&self, _path: &str) -> LoadPluginResult<Vec<crate::plugin::AssetMetadata>> {
            Err(LoadPluginError::Unsupported)
        }

        fn load(
            &self,
            request: &LoadRequest,
            _cache: &CacheManager,
        ) -> LoadPluginResult<LoadResponse> {
            *self
                .request
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(request.clone());
            let pixels = DecodedStraightRgba32F::new(
                2,
                1,
                vec![[0.25, 0.5, 0.75, 1.0], [1.0, 0.0, 0.0, 0.5]],
            )?;
            let color_space = match self.color {
                ResponseColor::Srgb => DecodedColorSpace::Srgb,
                ResponseColor::DisplayP3 => {
                    DecodedColorSpace::SourceEncoded(display_p3_description())
                }
            };
            Ok(LoadResponse::new(
                DecodedPixelBuffer::StraightRgba32F(pixels),
                DecodedPixelDescription::full_range_rgb(color_space),
            ))
        }
    }

    fn display_p3_description() -> SourceColorDescription {
        SourceColorDescription {
            primaries: Some(SourceColorPrimaries::DisplayP3),
            transfer: Some(SourceTransferCharacteristic::Srgb),
            bit_depth: Some(16),
            ..SourceColorDescription::default()
        }
    }

    fn project_with_asset(
        kind: AssetKind,
        source: Option<SourceColorDescription>,
    ) -> (AuthoringProject, tempfile::NamedTempFile, uuid::Uuid) {
        let mut project = AuthoringProject::new(
            "asset preview",
            640,
            360,
            RationalRate::new(30, 1).expect("rate"),
            MediaTime::new(5, 1).expect("duration"),
        )
        .expect("project");
        let file = tempfile::NamedTempFile::new().expect("temporary media");
        let mut asset = Asset::new("preview source", &file.path().to_string_lossy(), kind);
        if let Some(source) = source {
            asset.source_color.replace_detected(source);
        }
        let id = asset.id;
        project.assets.push(asset);
        (project, file, id)
    }

    fn manager(color: ResponseColor, request: Arc<Mutex<Option<LoadRequest>>>) -> PluginManager {
        let manager = PluginManager::new();
        manager.register_load_plugin(Arc::new(FloatLoader { color, request }));
        manager
    }

    #[test]
    fn float_video_frame_uses_exact_loader_time_and_reaches_preview_terminal() {
        let (project, _file, asset_id) = project_with_asset(AssetKind::Video, None);
        let request = Arc::new(Mutex::new(None));
        let image = load_asset_preview_frame(
            &project,
            asset_id,
            1.25,
            &manager(ResponseColor::Srgb, Arc::clone(&request)),
            &CacheManager::new(),
        )
        .expect("managed preview frame");

        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(image.data.len(), 8);
        assert!(matches!(
            request.lock().expect("request probe").as_ref(),
            Some(LoadRequest::VideoFrame { source_time, .. }) if *source_time == 1.25
        ));
    }

    #[test]
    fn high_precision_display_p3_frame_uses_asset_color_authority() {
        let source = display_p3_description();
        let (project, _file, asset_id) = project_with_asset(AssetKind::Image, Some(source));
        let request = Arc::new(Mutex::new(None));
        let image = load_asset_preview_frame(
            &project,
            asset_id,
            0.0,
            &manager(ResponseColor::DisplayP3, request),
            &CacheManager::new(),
        )
        .expect("color-managed P3 preview frame");

        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!(image.data.len(), 8);
    }

    #[test]
    fn conflicting_decoder_color_is_rejected_instead_of_displayed_as_srgb() {
        let (mut project, _file, asset_id) =
            project_with_asset(AssetKind::Image, Some(display_p3_description()));
        let asset = project
            .assets
            .iter_mut()
            .find(|asset| asset.id == asset_id)
            .expect("asset");
        asset
            .source_color
            .replace_complete_override(display_p3_description());
        let error = load_asset_preview_frame(
            &project,
            asset_id,
            0.0,
            &manager(ResponseColor::Srgb, Arc::new(Mutex::new(None))),
            &CacheManager::new(),
        )
        .expect_err("conflicting sRGB decode must fail closed");

        assert!(
            error
                .to_string()
                .contains("conflicts with authoritative source")
        );
    }
}
