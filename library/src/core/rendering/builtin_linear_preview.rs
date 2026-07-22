//! Strict, explicit Preview path for the verified built-in color pipeline.
//!
//! This is intentionally strict: only frames whose complete operation set can
//! remain scene-linear are accepted. It is not automatically dispatched by
//! the normal Preview: mixing this partial path with the legacy renderer would
//! make zoom, ROI, or graph shape change the color math.

use ruvie_color_management::{
    BuiltinColorTransform, ColorTransformBackend, ColorTransformRequest, CpuColorProcessor,
    LINEAR_SRGB_SPACE_ID, ManagedSceneLinearImage, SRGB_SPACE_ID, SceneLinearImage,
    WorkingColorIdentity,
};

use super::renderer::Affine2D;
use super::skia_working_surface::composite_source_over;
use crate::cache::CacheManager;
use crate::error::LibraryError;
use crate::model::BlendMode;
use crate::model::asset::{
    Asset, AssetKind, SourceColorPrimaries, SourceColorProfile, SourceTransferCharacteristic,
};
use crate::model::frame::Image;
use crate::model::frame::entity::{FrameContent, FrameItem, FrameObject, ImageSurface};
use crate::model::frame::frame::FrameInfo;
use crate::model::frame::transform::Transform;
use crate::model::project::{
    ColorConfigIdentity, DEFAULT_BUNDLED_COLOR_CONFIG_ID, Project, ResolvedColorManagementConfig,
};
use crate::plugin::{
    DecodedColorSpace, DecodedComponentStorage, DecodedPixelDescription, LoadRequest, PluginManager,
};
use ruvie_color_management::AlphaRepresentation;

struct VerifiedBuiltinPipeline {
    working: WorkingColorIdentity,
    source_to_working: Box<dyn CpuColorProcessor>,
    working_to_preview: Box<dyn CpuColorProcessor>,
}

pub(crate) enum LinearPreviewAttempt {
    Rendered(Image),
    LegacyRequired { reason: String },
}

pub(crate) fn try_render(
    project: &Project,
    frame: &FrameInfo,
    plugin_manager: &PluginManager,
    cache: &CacheManager,
) -> Result<LinearPreviewAttempt, LibraryError> {
    let pipeline = VerifiedBuiltinPipeline::for_project(project)?;
    if !frame.color_profile.eq_ignore_ascii_case("srgb") {
        return Err(LibraryError::Render(format!(
            "Composition color profile '{}' conflicts with the verified built-in sRGB Preview pipeline",
            frame.color_profile
        )));
    }
    let mut surfaces = Vec::new();
    if let Err(reason) = collect_surfaces(frame, &frame.items, &mut surfaces) {
        return Ok(LinearPreviewAttempt::LegacyRequired { reason });
    }

    let background = ManagedSceneLinearImage::new(
        pipeline.working.clone(),
        SceneLinearImage::solid_from_straight_rgba8(
            target_width(frame)?,
            target_height(frame)?,
            [
                frame.background_color.r,
                frame.background_color.g,
                frame.background_color.b,
                frame.background_color.a,
            ],
            pipeline.source_to_working.as_ref(),
        )
        .map_err(render_image_error)?,
    );

    let mut sources = Vec::with_capacity(surfaces.len());
    for surface in surfaces {
        let Some(asset) = source_asset(project, surface)? else {
            return Ok(LinearPreviewAttempt::LegacyRequired {
                reason: format!(
                    "managed Preview cannot infer source color for path {:?} without an Asset identity",
                    surface.file_path
                ),
            });
        };
        let decoded = plugin_manager.load_resource(
            &LoadRequest::Image {
                path: surface.file_path.clone(),
            },
            cache,
        )?;
        match classify_source(asset, surface, &decoded.decoded) {
            SourceSupport::Srgb => {}
            SourceSupport::Untagged(reason) => {
                return Ok(LinearPreviewAttempt::LegacyRequired { reason });
            }
            SourceSupport::Unsupported(reason) => {
                return Err(LibraryError::Render(reason));
            }
        }
        if (decoded.image.width, decoded.image.height)
            != (background.pixels().width(), background.pixels().height())
        {
            return Ok(LinearPreviewAttempt::LegacyRequired {
                reason: format!(
                    "managed Preview currently requires canvas-sized stills; asset {} is {}x{}, canvas is {}x{}",
                    asset.id,
                    decoded.image.width,
                    decoded.image.height,
                    background.pixels().width(),
                    background.pixels().height()
                ),
            });
        }
        sources.push(ManagedSceneLinearImage::new(
            pipeline.working.clone(),
            SceneLinearImage::from_straight_rgba8(
                decoded.image.width,
                decoded.image.height,
                &decoded.image.data,
                pipeline.source_to_working.as_ref(),
            )
            .map_err(render_image_error)?,
        ));
    }

    let composed = composite(background, sources)?;
    let rgba = composed
        .pixels()
        .to_straight_rgba8(pipeline.working_to_preview.as_ref())
        .map_err(render_image_error)?;
    Ok(LinearPreviewAttempt::Rendered(Image::new(
        composed.pixels().width(),
        composed.pixels().height(),
        rgba,
    )))
}

impl VerifiedBuiltinPipeline {
    fn for_project(project: &Project) -> Result<Self, LibraryError> {
        let resolved = project.resolved_color_management();
        let intent = match &resolved {
            ResolvedColorManagementConfig::Ready(intent) => intent,
            ResolvedColorManagementConfig::Unavailable { diagnostics, .. } => {
                let diagnostics = diagnostics
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; ");
                return Err(LibraryError::Render(format!(
                    "Project color configuration is unavailable: {diagnostics}"
                )));
            }
        };
        let config = intent.config();
        match config.config() {
            ColorConfigIdentity::Bundled { id } if id == DEFAULT_BUNDLED_COLOR_CONFIG_ID => {}
            identity => {
                return Err(LibraryError::Render(format!(
                    "Project color configuration {identity:?} requires an OpenColorIO backend that is not available in this build"
                )));
            }
        }
        if config.working_space() != LINEAR_SRGB_SPACE_ID
            || config.preview().display() != SRGB_SPACE_ID
            || config.preview().view().is_some()
        {
            return Err(LibraryError::Render(format!(
                "Built-in color backend cannot satisfy working='{}', display='{}', view={:?}",
                config.working_space(),
                config.preview().display(),
                config.preview().view()
            )));
        }

        let backend = BuiltinColorTransform;
        let source_request =
            ColorTransformRequest::source_to_working(SRGB_SPACE_ID, config.working_space());
        let preview_request = ColorTransformRequest::working_to_display(
            config.working_space(),
            config.preview().display(),
            config.preview().view().map(str::to_string),
        );
        let working = WorkingColorIdentity::scene_linear_f32(
            intent.cache_identity().as_str(),
            backend.backend_id(),
            backend.build(),
            backend.config_fingerprint(),
            config.working_space(),
        );
        Ok(Self {
            working,
            source_to_working: backend
                .create_cpu_processor(&source_request)
                .map_err(color_error)?,
            working_to_preview: backend
                .create_cpu_processor(&preview_request)
                .map_err(color_error)?,
        })
    }
}

fn collect_surfaces<'a>(
    frame: &FrameInfo,
    items: &'a [FrameItem],
    surfaces: &mut Vec<&'a ImageSurface>,
) -> Result<(), String> {
    if frame.render_scale.into_inner() != 1.0 {
        return Err("managed Preview currently requires render_scale=1".to_string());
    }
    if let Some(region) = frame.region
        && (region.x != 0.0
            || region.y != 0.0
            || region.width != frame.width as f64
            || region.height != frame.height as f64)
    {
        return Err("managed Preview currently requires the full composition region".to_string());
    }
    collect_items(frame, items, surfaces)
}

fn collect_items<'a>(
    frame: &FrameInfo,
    items: &'a [FrameItem],
    surfaces: &mut Vec<&'a ImageSurface>,
) -> Result<(), String> {
    for item in items {
        match item {
            FrameItem::Object(object) => collect_object(object, surfaces)?,
            FrameItem::Group(group) => {
                if group.width != frame.width
                    || group.height != frame.height
                    || group.background_color.a != 0
                    || !is_identity_transform(&group.transform)
                    || group.blend_mode != BlendMode::Normal
                    || !group.effects.is_empty()
                {
                    return Err(format!(
                        "group {:?} requires legacy transform, isolation, or effect rendering",
                        group.kind
                    ));
                }
                collect_items(frame, &group.items, surfaces)?;
            }
        }
    }
    Ok(())
}

fn collect_object<'a>(
    object: &'a FrameObject,
    surfaces: &mut Vec<&'a ImageSurface>,
) -> Result<(), String> {
    let FrameContent::Image { surface } = &object.content else {
        return Err("managed Preview currently accepts static Image objects only".to_string());
    };
    if !is_identity_transform(&object.spatial_transform)
        || !is_identity_transform(&surface.transform)
        || !surface.effects.is_empty()
    {
        return Err("Image requires legacy transform or effect rendering".to_string());
    }
    surfaces.push(surface);
    Ok(())
}

fn is_identity_transform(transform: &Transform) -> bool {
    const TOLERANCE: f64 = 1.0e-10;
    let affine = Affine2D::from(transform);
    (affine.scale_x - 1.0).abs() <= TOLERANCE
        && affine.skew_x.abs() <= TOLERANCE
        && affine.translate_x.abs() <= TOLERANCE
        && affine.skew_y.abs() <= TOLERANCE
        && (affine.scale_y - 1.0).abs() <= TOLERANCE
        && affine.translate_y.abs() <= TOLERANCE
        && (transform.opacity - 1.0).abs() <= TOLERANCE
}

fn source_asset<'a>(
    project: &'a Project,
    surface: &ImageSurface,
) -> Result<Option<&'a Asset>, LibraryError> {
    let Some(asset_id) = surface.asset_id else {
        return Ok(None);
    };
    let asset = project.get_asset(asset_id).ok_or_else(|| {
        LibraryError::Render(format!(
            "Managed Preview source Asset {asset_id} no longer exists"
        ))
    })?;
    if asset.kind != AssetKind::Image || asset.path != surface.file_path {
        return Err(LibraryError::Render(format!(
            "Managed Preview source {} does not match Image Asset {asset_id}",
            surface.file_path
        )));
    }
    Ok(Some(asset))
}

enum SourceSupport {
    Srgb,
    Untagged(String),
    Unsupported(String),
}

fn classify_source(
    asset: &Asset,
    surface: &ImageSurface,
    decoded: &DecodedPixelDescription,
) -> SourceSupport {
    if let Some(output) = surface.output_color_space.as_deref() {
        return SourceSupport::Unsupported(format!(
            "Image Asset {} requests loader-side output color space '{output}', which would quantize before scene-linear rendering",
            asset.id
        ));
    }
    if let Some(input) = surface.input_color_space.as_deref() {
        return SourceSupport::Unsupported(format!(
            "Image Asset {} requests legacy loader-side source color space '{input}'; managed rendering requires the authoritative Asset override",
            asset.id
        ));
    }

    if !decoded.rgb_matrix_applied || !decoded.full_range {
        return SourceSupport::Unsupported(format!(
            "Image Asset {} loader output is not full-range RGB; matrix/range conversion must happen exactly once in the loader adapter",
            asset.id
        ));
    }
    if decoded.alpha != AlphaRepresentation::Straight
        || decoded.storage != DecodedComponentStorage::Unorm8
    {
        return SourceSupport::Unsupported(format!(
            "Image Asset {} loader output {:?}/{:?} is not the supported straight RGBA8 adapter",
            asset.id, decoded.alpha, decoded.storage
        ));
    }
    match &decoded.color_space {
        DecodedColorSpace::Srgb => return SourceSupport::Srgb,
        DecodedColorSpace::Named(space) if space.eq_ignore_ascii_case(SRGB_SPACE_ID) => {
            return SourceSupport::Srgb;
        }
        DecodedColorSpace::Named(space) => {
            return SourceSupport::Unsupported(format!(
                "Image Asset {} decoded into '{space}', which requires an OpenColorIO backend",
                asset.id
            ));
        }
        DecodedColorSpace::Unknown => {
            return SourceSupport::Untagged(format!(
                "Image Asset {} loader did not describe its decoded pixels",
                asset.id
            ));
        }
        DecodedColorSpace::SourceEncoded(_) => {}
    }

    let source = asset.source_color.effective();
    let exact_srgb_chromaticity = source.primaries == Some(SourceColorPrimaries::Bt709)
        && source.transfer == Some(SourceTransferCharacteristic::Srgb)
        && source.profile.is_none();
    if exact_srgb_chromaticity {
        return match source.bit_depth {
            Some(1 | 2 | 4 | 8) => SourceSupport::Srgb,
            Some(bit_depth) => SourceSupport::Unsupported(format!(
                "Image Asset {} is {bit_depth}-bit sRGB, but the current native loader would quantize it to RGBA8 before color conversion",
                asset.id
            )),
            None => SourceSupport::Untagged(format!(
                "Image Asset {} has explicit sRGB chromaticity but unknown source bit depth",
                asset.id
            )),
        };
    }
    let has_explicit_color_identity =
        source.primaries.is_some() || source.transfer.is_some() || source.profile.is_some();
    if has_explicit_color_identity {
        let profile = match &source.profile {
            Some(SourceColorProfile::Icc { sha256, .. }) => format!("ICC {sha256}"),
            Some(SourceColorProfile::Other { identity, .. }) => identity.clone(),
            None => "none".to_string(),
        };
        SourceSupport::Unsupported(format!(
            "Image Asset {} source metadata {:?}/{:?}/profile={} requires OpenColorIO or ICC processing",
            asset.id, source.primaries, source.transfer, profile
        ))
    } else {
        SourceSupport::Untagged(format!(
            "Image Asset {} is untagged; compatibility rendering is retained instead of guessing sRGB",
            asset.id
        ))
    }
}

fn composite(
    background: ManagedSceneLinearImage,
    sources: Vec<ManagedSceneLinearImage>,
) -> Result<ManagedSceneLinearImage, LibraryError> {
    if let Some(source) = sources
        .iter()
        .find(|source| source.identity() != background.identity())
    {
        return Err(LibraryError::Render(format!(
            "Cannot composite working image {:?} into {:?}",
            source.identity(),
            background.identity()
        )));
    }
    let identity = background.identity().clone();
    let pixels = sources
        .into_iter()
        .map(|source| source.into_parts().1)
        .collect::<Vec<_>>();
    Ok(ManagedSceneLinearImage::new(
        identity,
        composite_source_over(background.pixels(), &pixels)?,
    ))
}

fn target_width(frame: &FrameInfo) -> Result<u32, LibraryError> {
    u32::try_from(frame.width)
        .map_err(|_| LibraryError::Render("Preview width exceeds u32".to_string()))
}

fn target_height(frame: &FrameInfo) -> Result<u32, LibraryError> {
    u32::try_from(frame.height)
        .map_err(|_| LibraryError::Render("Preview height exceeds u32".to_string()))
}

fn color_error(error: ruvie_color_management::ColorManagementError) -> LibraryError {
    LibraryError::Render(format!("Cannot create verified color processor: {error}"))
}

fn render_image_error(error: ruvie_color_management::SceneLinearImageError) -> LibraryError {
    LibraryError::Render(format!("Scene-linear image conversion failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::{LinearPreviewAttempt, try_render};
    use crate::cache::CacheManager;
    use crate::model::asset::{
        Asset, AssetKind, SourceColorDescription, SourceColorPrimaries, SourceColorRange,
        SourceMatrixCoefficients, SourceTransferCharacteristic,
    };
    use crate::model::frame::color::Color;
    use crate::model::frame::entity::{FrameContent, FrameItem, FrameObject, ImageSurface};
    use crate::model::frame::frame::FrameInfo;
    use crate::model::frame::transform::Transform;
    use crate::model::project::Project;
    use crate::plugin::PluginManager;
    use ordered_float::OrderedFloat;
    use uuid::Uuid;

    fn explicit_srgb_asset(path: &str) -> Asset {
        let mut asset = Asset::new("managed", path, AssetKind::Image);
        asset
            .source_color
            .replace_complete_override(SourceColorDescription {
                primaries: Some(SourceColorPrimaries::Bt709),
                transfer: Some(SourceTransferCharacteristic::Srgb),
                matrix: Some(SourceMatrixCoefficients::Identity),
                range: Some(SourceColorRange::Full),
                bit_depth: Some(8),
                profile: None,
            });
        asset
    }

    fn object(asset: &Asset) -> FrameItem {
        FrameItem::Object(FrameObject {
            source_node_id: Uuid::new_v4(),
            spatial_transform_node_id: None,
            spatial_transform: Box::new(Transform::default()),
            content_bounds: None,
            content: FrameContent::Image {
                surface: ImageSurface {
                    asset_id: Some(asset.id),
                    file_path: asset.path.clone(),
                    effects: Vec::new(),
                    input_color_space: None,
                    output_color_space: None,
                    transform: Transform::default(),
                },
            },
        })
    }

    fn frame(items: Vec<FrameItem>) -> FrameInfo {
        FrameInfo {
            width: 1,
            height: 1,
            background_color: Color::black(),
            color_profile: "sRGB".to_string(),
            render_scale: OrderedFloat(1.0),
            now_time: OrderedFloat(0.0),
            region: None,
            items,
        }
    }

    #[test]
    fn strict_attempt_composites_half_white_in_linear_light() {
        let path =
            std::env::temp_dir().join(format!("ruvie-linear-preview-{}.png", Uuid::new_v4()));
        image::save_buffer(&path, &[255, 255, 255, 128], 1, 1, image::ColorType::Rgba8).unwrap();
        let asset = explicit_srgb_asset(path.to_str().unwrap());
        let mut project = Project::new("linear Preview");
        project.assets.push(asset.clone());

        let attempt = try_render(
            &project,
            &frame(vec![object(&asset)]),
            &PluginManager::default(),
            &CacheManager::new(),
        )
        .unwrap();
        let LinearPreviewAttempt::Rendered(image) = attempt else {
            panic!("explicit sRGB frame must use the managed path");
        };
        assert_eq!(image.data, [188, 188, 188, 255]);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn missing_asset_identity_reports_ineligibility_instead_of_guessing() {
        let asset = explicit_srgb_asset("legacy-path.png");
        let mut item = object(&asset);
        if let FrameItem::Object(object) = &mut item
            && let FrameContent::Image { surface } = &mut object.content
        {
            surface.asset_id = None;
        }
        let attempt = try_render(
            &Project::new("legacy"),
            &frame(vec![item]),
            &PluginManager::default(),
            &CacheManager::new(),
        )
        .unwrap();
        let LinearPreviewAttempt::LegacyRequired { reason } = attempt else {
            panic!("unannotated source must not enter managed rendering");
        };
        assert!(reason.contains("without an Asset identity"));
    }

    #[test]
    fn explicitly_wide_source_fails_instead_of_falling_back_to_srgb() {
        let path = std::env::temp_dir().join(format!("ruvie-wide-preview-{}.png", Uuid::new_v4()));
        image::save_buffer(&path, &[10, 20, 30, 255], 1, 1, image::ColorType::Rgba8).unwrap();
        let mut asset = explicit_srgb_asset(path.to_str().unwrap());
        asset.source_color.edit_override(|source| {
            source.primaries = Some(SourceColorPrimaries::Bt2020);
            source.transfer = Some(SourceTransferCharacteristic::Pq);
        });
        let mut project = Project::new("wide");
        project.assets.push(asset.clone());

        let error = match try_render(
            &project,
            &frame(vec![object(&asset)]),
            &PluginManager::default(),
            &CacheManager::new(),
        ) {
            Ok(_) => panic!("wide source must not silently use sRGB"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("requires OpenColorIO"));
        std::fs::remove_file(path).unwrap();
    }
}
