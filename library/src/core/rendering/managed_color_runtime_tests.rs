use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use ordered_float::OrderedFloat;
use ruvie_color_management::{LINEAR_SRGB_SPACE_ID, SRGB_SPACE_ID};
use uuid::Uuid;

use crate::cache::CacheManager;
use crate::model::asset::{
    Asset, AssetKind, SourceColorDescription, SourceColorPrimaries, SourceTransferCharacteristic,
};
use crate::model::frame::Image;
use crate::model::frame::color::Color;
use crate::model::frame::entity::{FrameContent, FrameItem, FrameObject, ImageSurface};
use crate::model::frame::frame::FrameInfo;
use crate::model::frame::transform::Transform;
#[cfg(feature = "opencolorio")]
use crate::model::project::Composition;
use crate::model::project::Project;
use crate::model::project::{
    ColorConfigIdentity, ColorManagementConfig, ColorManagementIssue, ExportColorConfig,
    HdrColorField, HdrColorSettings, LEGACY_BUNDLED_COLOR_CONFIG_V1_ID, PreviewColorConfig,
};
use crate::plugin::{
    AssetMetadata, DecodedColorSpace, DecodedPixelBuffer, DecodedPixelDescription,
    DecodedStraightRgba8, DecodedStraightRgba16F, DecodedStraightRgba32F, LoadPlugin,
    LoadPluginError, LoadPluginResult, LoadRequest, LoadResponse, Plugin, PluginManager,
};
use crate::rendering::renderer::RenderOutput;
use crate::{RenderDestination, RenderService, SkiaRenderer};

#[test]
fn effect_color_conversion_uses_the_exact_project_working_processor() {
    let project = Project::new("effect color conversion");
    let pipeline = super::managed_color_backend::ProjectColorPipeline::for_project(
        &project,
        super::managed_color_backend::ManagedRenderDestination::Preview,
    )
    .expect("default Project color pipeline");
    let authored = crate::model::property::ColorValue::from_straight_srgba8(&Color {
        r: 128,
        g: 128,
        b: 128,
        a: 127,
    });
    let working = pipeline
        .effect_color_to_working(&authored)
        .expect("convert effect color through Project processor");
    assert_eq!(
        working.color_space(),
        &crate::model::property::ColorSpaceRef::linear_srgb()
    );
    let [r, g, b, a] = working.rgba();
    for component in [r, g, b] {
        assert!(
            (component - 0.215_860_500_113_899_26).abs() <= 1.0e-12,
            "encoded gray 128 was not linearized: {component}"
        );
    }
    assert_eq!(a, 127.0 / 255.0);
}

#[derive(Clone)]
struct Payload {
    pixels: DecodedPixelBuffer,
    decoded: DecodedPixelDescription,
}

struct ExactTestLoader {
    payloads: HashMap<String, Payload>,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Plugin for ExactTestLoader {
    fn id(&self) -> &'static str {
        "managed_color_exact_test_loader"
    }

    fn name(&self) -> String {
        "Managed color exact test loader".to_string()
    }

    fn category(&self) -> String {
        "Test".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 0, 1)
    }
}

impl LoadPlugin for ExactTestLoader {
    fn open(&self, _path: &str) -> LoadPluginResult<Vec<AssetMetadata>> {
        Err(LoadPluginError::Unsupported)
    }

    fn load(&self, request: &LoadRequest, _cache: &CacheManager) -> LoadPluginResult<LoadResponse> {
        let Some(payload) = self.payloads.get(request.path()) else {
            return Err(LoadPluginError::Unsupported);
        };
        self.requests
            .lock()
            .map_err(|_| {
                LoadPluginError::Failed(crate::error::LibraryError::Plugin(
                    "managed color test request log lock was poisoned".to_string(),
                ))
            })?
            .push(request_kind(request).to_string());
        Ok(LoadResponse::new(
            payload.pixels.clone(),
            payload.decoded.clone(),
        ))
    }
}

fn request_kind(request: &LoadRequest) -> &'static str {
    match request {
        LoadRequest::Image { .. } => "image",
        LoadRequest::VideoFrame { .. } => "video",
    }
}

static TEST_MEDIA_FILES: OnceLock<Mutex<Vec<tempfile::NamedTempFile>>> = OnceLock::new();

fn asset(label: &str, kind: AssetKind) -> Asset {
    let file = tempfile::Builder::new()
        .prefix("ruvie-managed-color-media-")
        .tempfile()
        .expect("create direct regular media locator for automatic render test");
    let path = file.path().to_string_lossy().into_owned();
    TEST_MEDIA_FILES
        .get_or_init(|| Mutex::new(Vec::new()))
        .lock()
        .expect("managed color test file registry lock")
        .push(file);
    Asset::new(label, &path, kind)
}

fn surface(asset: &Asset) -> ImageSurface {
    ImageSurface {
        asset_id: Some(asset.id),
        file_path: asset.path.clone(),
        effects: Vec::new(),
        input_color_space: None,
        output_color_space: None,
        transform: Transform::default(),
    }
}

fn image_object(asset: &Asset) -> FrameItem {
    object(FrameContent::Image {
        surface: surface(asset),
    })
}

fn video_object(asset: &Asset) -> FrameItem {
    object(FrameContent::Video {
        surface: surface(asset),
        source_time: 0.25,
        stream_index: Some(0),
    })
}

fn object(content: FrameContent) -> FrameItem {
    FrameItem::Object(FrameObject {
        source_node_id: Uuid::new_v4(),
        spatial_transform_node_id: None,
        spatial_transform: Box::new(Transform::default()),
        content_bounds: None,
        content,
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

fn service(
    payloads: impl IntoIterator<Item = (String, Payload)>,
) -> (RenderService<SkiaRenderer>, Arc<Mutex<Vec<String>>>) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let manager = Arc::new(PluginManager::new());
    manager.register_load_plugin(Arc::new(ExactTestLoader {
        payloads: payloads.into_iter().collect(),
        requests: Arc::clone(&requests),
    }));
    let cache = Arc::new(CacheManager::new());
    let renderer = SkiaRenderer::new(1, 1, Color::black(), false, None, Some(cache.clone()))
        .expect("CPU renderer");
    (RenderService::new(renderer, manager, cache), requests)
}

fn srgb_payload(rgba: [u8; 4]) -> Payload {
    Payload {
        pixels: DecodedPixelBuffer::StraightRgba8(
            DecodedStraightRgba8::new(Image::new(1, 1, rgba.to_vec()))
                .expect("valid RGBA8 test payload"),
        ),
        decoded: DecodedPixelDescription::full_range_rgb(DecodedColorSpace::Srgb),
    }
}

fn encoded_payload(rgba: [u8; 4], source: SourceColorDescription) -> Payload {
    let mut payload = srgb_payload(rgba);
    payload.decoded =
        DecodedPixelDescription::full_range_rgb(DecodedColorSpace::SourceEncoded(source));
    payload
}

fn srgb_rgba16f_payload(rgba: [f32; 4]) -> Payload {
    Payload {
        pixels: DecodedPixelBuffer::StraightRgba16F(
            DecodedStraightRgba16F::new(1, 1, vec![rgba.map(half::f16::from_f32)])
                .expect("valid RGBA16F test payload"),
        ),
        decoded: DecodedPixelDescription::full_range_rgb(DecodedColorSpace::Srgb),
    }
}

fn srgb_rgba32f_payload(rgba: [f32; 4]) -> Payload {
    Payload {
        pixels: DecodedPixelBuffer::StraightRgba32F(
            DecodedStraightRgba32F::new(1, 1, vec![rgba]).expect("valid RGBA32F test payload"),
        ),
        decoded: DecodedPixelDescription::full_range_rgb(DecodedColorSpace::Srgb),
    }
}

fn image_data(output: RenderOutput) -> Vec<u8> {
    match output {
        RenderOutput::Image(image) => image.data,
        RenderOutput::Working(_) => {
            panic!("managed Project output must pass through exactly one terminal transform")
        }
        RenderOutput::Texture(_) => panic!("managed CPU terminal must own its Image output"),
    }
}

/// Independent f64 oracle for the neutral-axis PQ -> relative display light ->
/// sRGB path. Constants are the normalized SMPTE ST 2084 inverse EOTF and the
/// IEC 61966-2-1 sRGB OETF; the terminal u8 contract rounds to nearest after
/// clamping, matching [`LinearWorkingImage::to_straight_rgba8`].
fn pq_gray_u8_to_srgb_u8(code: u8, reference_white_nits: f64) -> u8 {
    const M1: f64 = 2610.0 / 16_384.0;
    const M2: f64 = (2523.0 / 4096.0) * 128.0;
    const C1: f64 = 3424.0 / 4096.0;
    const C2: f64 = (2413.0 / 4096.0) * 32.0;
    const C3: f64 = (2392.0 / 4096.0) * 32.0;

    let encoded = f64::from(code) / 255.0;
    let magnitude = encoded.powf(1.0 / M2);
    let normalized_nits = ((magnitude - C1).max(0.0) / (C2 - C3 * magnitude)).powf(1.0 / M1);
    let relative_linear = 10_000.0 * normalized_nits / reference_white_nits;
    let srgb = if relative_linear <= 0.003_130_8 {
        12.92 * relative_linear
    } else {
        1.055 * relative_linear.powf(1.0 / 2.4) - 0.055
    };
    (srgb.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[test]
fn production_call_graph_composites_image_and_video_in_linear_float() {
    let image_asset = asset("managed-red.still", AssetKind::Image);
    let video_asset = asset("managed-green.video", AssetKind::Video);
    let mut project = Project::new("managed mixed media");
    project
        .assets
        .extend([image_asset.clone(), video_asset.clone()]);
    let (mut service, requests) = service([
        (image_asset.path.clone(), srgb_payload([255, 0, 0, 128])),
        (video_asset.path.clone(), srgb_payload([0, 255, 0, 128])),
    ]);

    let output = service
        .render_project_frame(
            &project,
            &frame(vec![image_object(&image_asset), video_object(&video_asset)]),
            RenderDestination::Preview,
        )
        .expect("managed production frame");

    assert_eq!(image_data(output), [137, 188, 0, 255]);
    assert_eq!(
        *requests.lock().expect("request log lock"),
        ["image", "video"]
    );
}

#[test]
fn production_call_graph_accepts_rgba16f_and_rgba32f_without_rgba8_ingress() {
    let image_asset = asset("managed-red-16f.still", AssetKind::Image);
    let video_asset = asset("managed-green-32f.video", AssetKind::Video);
    let mut project = Project::new("managed float media");
    project
        .assets
        .extend([image_asset.clone(), video_asset.clone()]);
    let (mut service, _) = service([
        (
            image_asset.path.clone(),
            srgb_rgba16f_payload([1.0, 0.0, 0.0, 0.5]),
        ),
        (
            video_asset.path.clone(),
            srgb_rgba32f_payload([0.0, 1.0, 0.0, 0.5]),
        ),
    ]);

    let output = service
        .render_project_frame(
            &project,
            &frame(vec![image_object(&image_asset), video_object(&video_asset)]),
            RenderDestination::Preview,
        )
        .expect("typed float media must remain float until the sRGB terminal");

    assert_eq!(image_data(output), [137, 188, 0, 255]);
}

#[test]
fn production_preview_and_export_compile_distinct_terminal_processors() {
    let image_asset = asset("managed-terminal.still", AssetKind::Image);
    let mut project = Project::new("managed terminal intent");
    project.assets.push(image_asset.clone());
    project
        .set_color_management(ColorManagementConfig::new(
            ColorConfigIdentity::default(),
            LINEAR_SRGB_SPACE_ID,
            PreviewColorConfig::direct(SRGB_SPACE_ID),
            ExportColorConfig::new(SRGB_SPACE_ID),
        ))
        .expect("valid built-in terminal intent");
    let (mut service, _) =
        service([(image_asset.path.clone(), srgb_payload([128, 128, 128, 255]))]);
    let frame = frame(vec![image_object(&image_asset)]);

    let preview = service
        .render_project_frame(&project, &frame, RenderDestination::Preview)
        .expect("sRGB preview terminal");
    let export = service
        .render_project_frame(&project, &frame, RenderDestination::Export)
        .expect("sRGB export terminal");

    assert_eq!(image_data(preview), [128, 128, 128, 255]);
    let RenderOutput::Image(export_image) = export else {
        panic!("managed media-only export must produce owned CPU pixels");
    };
    let export_frame =
        crate::plugin::ExportFrame::from_graph_project_render(&project, export_image)
            .expect("managed media-only export must retain typed Project color authority");
    assert_eq!(export_frame.image().data, [128, 128, 128, 255]);
}

#[test]
fn former_builtin_v1_project_renders_preview_and_typed_export_without_reinterpretation() {
    let project = Project::new("former bundled v1 runtime");
    let mut persisted = serde_json::to_value(project).unwrap();
    persisted["color_management"] = serde_json::json!({
        "config": {
            "kind": "bundled",
            "id": LEGACY_BUNDLED_COLOR_CONFIG_V1_ID
        },
        "working_space": "linear-srgb",
        "preview": {
            "display": "srgb",
            "view": null
        },
        "export": {
            "output_space": "srgb"
        }
    });
    let project = Project::load(&serde_json::to_string(&persisted).unwrap()).unwrap();
    let (mut service, _) = service([]);
    let empty_frame = frame(Vec::new());

    let preview = service
        .render_project_frame(&project, &empty_frame, RenderDestination::Preview)
        .expect("v1 Preview backend remains available");
    assert_eq!(image_data(preview), [0, 0, 0, 255]);

    let export = service
        .render_project_frame(&project, &empty_frame, RenderDestination::Export)
        .expect("v1 Export backend remains available");
    let RenderOutput::Image(image) = export else {
        panic!("managed v1 Export must produce typed CPU pixels");
    };
    let typed = crate::plugin::ExportFrame::from_graph_project_render(&project, image)
        .expect("v1 terminal pixels retain Project-derived export authority");
    assert_eq!(typed.image().data, [0, 0, 0, 255]);
}

#[test]
fn production_video_uses_detected_display_p3_processor_instead_of_srgb_fallback() {
    let source = SourceColorDescription {
        primaries: Some(SourceColorPrimaries::DisplayP3),
        transfer: Some(SourceTransferCharacteristic::Srgb),
        bit_depth: Some(8),
        ..SourceColorDescription::default()
    };
    let mut video_asset = asset("managed-p3.video", AssetKind::Video);
    video_asset.source_color.replace_detected(source.clone());
    let mut project = Project::new("managed Display P3 video");
    project.assets.push(video_asset.clone());
    let (mut service, requests) = service([(
        video_asset.path.clone(),
        encoded_payload([128, 0, 0, 255], source),
    )]);

    let output = service
        .render_project_frame(
            &project,
            &frame(vec![video_object(&video_asset)]),
            RenderDestination::Preview,
        )
        .expect("Display P3 video must use the managed source processor");

    assert_eq!(image_data(output), [141, 0, 0, 255]);
    assert_eq!(*requests.lock().expect("request log lock"), ["video"]);
}

#[test]
fn production_pq_video_uses_explicit_reference_white_policy_to_srgb_preview() {
    let source = SourceColorDescription {
        primaries: Some(SourceColorPrimaries::Bt2020),
        transfer: Some(SourceTransferCharacteristic::Pq),
        bit_depth: Some(8),
        ..SourceColorDescription::default()
    };
    let mut video_asset = asset("managed-pq.video", AssetKind::Video);
    video_asset.source_color.replace_detected(source.clone());
    let mut project = Project::new("managed PQ video");
    project.assets.push(video_asset.clone());
    project
        .set_color_management(
            ColorManagementConfig::default()
                .with_hdr_settings(HdrColorSettings::for_pq(203.0).expect("valid PQ intent")),
        )
        .expect("PQ source settings must not invalidate the Project");
    let (mut service, _) = service([(
        video_asset.path.clone(),
        encoded_payload([128, 128, 128, 255], source),
    )]);

    let output = service
        .render_project_frame(
            &project,
            &frame(vec![video_object(&video_asset)]),
            RenderDestination::Preview,
        )
        .expect("PQ source must linearize relative to explicit reference white");

    let expected = pq_gray_u8_to_srgb_u8(128, 203.0);
    assert_eq!(expected, 181, "independent ST 2084/sRGB oracle changed");
    assert_eq!(image_data(output), [expected, expected, expected, 255]);
}

#[test]
fn pq_source_without_project_policy_fails_only_that_render_operation() {
    let source = SourceColorDescription {
        primaries: Some(SourceColorPrimaries::Bt2020),
        transfer: Some(SourceTransferCharacteristic::Pq),
        bit_depth: Some(8),
        ..SourceColorDescription::default()
    };
    let mut video_asset = asset("managed-pq-missing.video", AssetKind::Video);
    video_asset.source_color.replace_detected(source.clone());
    let mut project = Project::new("repairable PQ video");
    project.assets.push(video_asset.clone());
    assert!(matches!(
        project.resolved_color_management(),
        crate::model::project::ResolvedColorManagementConfig::Ready(_)
    ));
    let diagnostics = project.color_management_diagnostics();
    for field in [
        HdrColorField::ReferenceWhiteNits,
        HdrColorField::PqLinearizationPolicy,
    ] {
        assert!(diagnostics.iter().any(|issue| matches!(
            issue,
            ColorManagementIssue::MissingHdrSetting { field: missing, required_by }
                if *missing == field
                    && required_by.contains(&video_asset.id.to_string())
        )));
    }
    let (mut service, _) = service([(
        video_asset.path.clone(),
        encoded_payload([128, 128, 128, 255], source),
    )]);

    let error = service
        .render_project_frame(
            &project,
            &frame(vec![video_object(&video_asset)]),
            RenderDestination::Preview,
        )
        .expect_err("PQ without an explicit Project policy must fail closed");
    assert!(error.to_string().contains("RUVIE_PQ_LINEARIZATION_POLICY"));
}

#[test]
fn non_srgb_preview_terminal_is_rejected_before_untagged_egui_output() {
    let image_asset = asset("managed-p3-display.still", AssetKind::Image);
    let mut project = Project::new("unsupported P3 display boundary");
    project.assets.push(image_asset.clone());
    project
        .set_color_management(ColorManagementConfig::new(
            ColorConfigIdentity::default(),
            LINEAR_SRGB_SPACE_ID,
            PreviewColorConfig::direct(ruvie_color_management::DISPLAY_P3_SPACE_ID),
            ExportColorConfig::default(),
        ))
        .expect("P3 display intent remains editable");
    let (mut service, _) =
        service([(image_asset.path.clone(), srgb_payload([128, 128, 128, 255]))]);

    let error = service
        .render_project_frame(
            &project,
            &frame(vec![image_object(&image_asset)]),
            RenderDestination::Preview,
        )
        .expect_err("untagged egui output must not claim Display P3");
    assert!(
        error
            .to_string()
            .contains("exact Project-bound sRGB surface space")
    );
}

#[test]
fn non_srgb_export_terminal_is_rejected_before_untagged_rgba8_output() {
    let image_asset = asset("managed-p3-export.still", AssetKind::Image);
    let mut project = Project::new("unsupported P3 export boundary");
    project.assets.push(image_asset.clone());
    project
        .set_color_management(ColorManagementConfig::new(
            ColorConfigIdentity::default(),
            LINEAR_SRGB_SPACE_ID,
            PreviewColorConfig::direct(SRGB_SPACE_ID),
            ExportColorConfig::new(ruvie_color_management::DISPLAY_P3_SPACE_ID),
        ))
        .expect("P3 export intent remains editable");
    let (mut service, _) =
        service([(image_asset.path.clone(), srgb_payload([128, 128, 128, 255]))]);

    let error = service
        .render_project_frame(
            &project,
            &frame(vec![image_object(&image_asset)]),
            RenderDestination::Export,
        )
        .expect_err("untagged export output must not claim Display P3");
    assert!(error.to_string().contains("untagged RGBA8 export"));
}

#[cfg(feature = "opencolorio")]
#[test]
fn production_named_ocio_preview_chains_view_output_to_bound_srgb_surface() {
    if std::env::var("RUVIE_REQUIRE_REAL_OCIO").as_deref() != Ok("1") {
        eprintln!("skipped: production OCIO surface test is run by the verified real-runtime gate");
        return;
    }

    for malicious_literal_srgb in [false, true] {
        let bytes = ocio_surface_fixture(malicious_literal_srgb);
        let directory =
            std::env::temp_dir().join(format!("ruvie-ocio-surface-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&directory).expect("create exact config fixture directory");
        let path = directory.join("surface-authority-v1.ocio");
        std::fs::write(&path, &bytes).expect("write exact OCIO config fixture");

        let mut config_asset = Asset::new(
            "surface authority config",
            path.to_str().expect("UTF-8 temp path"),
            AssetKind::Other,
        );
        let checksum = config_asset.verify_imported_content(&bytes);
        let config_identity = ColorConfigIdentity::ProjectAsset {
            asset_id: config_asset.id,
            sha256: checksum,
            ocio_version: "2.5.2".to_string(),
        };
        let config = ColorManagementConfig::new(
            config_identity.clone(),
            "fixture-linear-working",
            PreviewColorConfig::named_view(
                "fixture-display",
                "fixture-view",
                "fixture-view-linear",
                crate::model::project::PreviewSurfaceEncoding::Srgb,
            ),
            ExportColorConfig::new("fixture-ui-srgb"),
        )
        .with_srgb_surface_space("fixture-ui-srgb");
        let mut project = Project::new("exact sRGB surface authority");
        project.assets.push(config_asset);
        project
            .set_color_management(config)
            .expect("complete exact custom color config");

        let (mut service, _) = service(std::iter::empty::<(String, Payload)>());
        let mut authored_frame = frame(Vec::new());
        authored_frame.background_color = Color {
            r: 118,
            g: 118,
            b: 118,
            a: 255,
        };
        // This legacy free string is deliberately either unavailable or
        // malicious in the active config. Production background ingress must
        // use the Project-bound exact authoring authority instead.
        authored_frame.color_profile = "sRGB".to_string();
        service
            .renderer
            .resize_render_target(
                authored_frame
                    .width
                    .try_into()
                    .expect("fixture width fits u32"),
                authored_frame
                    .height
                    .try_into()
                    .expect("fixture height fits u32"),
                authored_frame.background_color.clone(),
            )
            .expect("synchronize the production render target");
        let output = service
            .render_project_frame(&project, &authored_frame, RenderDestination::Preview)
            .expect("named view must chain into the exact sRGB surface binding");
        let actual = image_data(output);
        assert!(
            (117..=119).contains(&actual[0]),
            "without the view-output -> surface processor, the identity view's linear value packs near 46 instead of the authored 118; got {actual:?}"
        );
        assert_eq!(actual[0], actual[1]);
        assert_eq!(actual[1], actual[2]);
        assert_eq!(actual[3], 255);

        let (mut composition, track) = Composition::new("surface export", 1, 1, 24.0, 1.0);
        composition.background_color = Color {
            r: 118,
            g: 118,
            b: 118,
            a: 255,
        };
        composition.color_profile = "sRGB".to_string();
        project
            .add_track(track)
            .expect("insert export fixture Track");
        project
            .add_composition(composition)
            .expect("insert export fixture Composition");
        let export = service
            .render_project_frame(&project, &authored_frame, RenderDestination::Export)
            .expect("custom nonliteral sRGB surface binding must reach ExportFrame");
        let RenderOutput::Image(export) = export else {
            panic!("managed export must terminate to an image");
        };
        let export_rgba = &export.data;
        assert!(
            (117..=119).contains(&export_rgba[0]),
            "RenderService -> ExportFrame must use the exact custom surface binding; got {export_rgba:?}"
        );
        assert_eq!(export_rgba[0], export_rgba[1]);
        assert_eq!(export_rgba[1], export_rgba[2]);
        assert_eq!(export_rgba[3], 255);

        let rejected_config = ColorManagementConfig::new(
            config_identity,
            "fixture-linear-working",
            PreviewColorConfig::named_view(
                "fixture-display",
                "fixture-view",
                "fixture-view-linear",
                crate::model::project::PreviewSurfaceEncoding::Srgb,
            ),
            // A familiar global name is not authority inside this config.
            ExportColorConfig::new("srgb"),
        )
        .with_srgb_surface_space("fixture-ui-srgb");
        let mut rejected_project = project.clone();
        rejected_project
            .set_color_management(rejected_config)
            .expect("mismatched export intent remains editable");
        let error = service
            .render_project_frame(
                &rejected_project,
                &authored_frame,
                RenderDestination::Export,
            )
            .expect_err("an unbound literal srgb output must never inherit surface authority");
        assert!(
            error
                .to_string()
                .contains("not the exact active-config sRGB surface space"),
            "unexpected unbound literal rejection: {error}"
        );

        std::fs::remove_file(&path).expect("remove exact config fixture");
        std::fs::remove_dir(&directory).expect("remove exact config fixture directory");
    }
}

#[cfg(feature = "opencolorio")]
fn ocio_surface_fixture(malicious_literal_srgb: bool) -> Vec<u8> {
    let malicious = if malicious_literal_srgb {
        r#"
  - !<ColorSpace>
    name: sRGB
    family: malicious
    bitdepth: 32f
    isdata: false
    allocation: uniform
    to_scene_reference: !<ExponentTransform> {value: [4.0, 4.0, 4.0, 1.0], style: pass_thru}
"#
    } else {
        ""
    };
    format!(
        r#"ocio_profile_version: 2

search_path: ""
strictparsing: true

roles:
  default: fixture-ui-srgb
  scene_linear: fixture-linear-working

displays:
  fixture-display:
    - !<View> {{name: fixture-view, colorspace: fixture-view-linear}}

active_displays: [fixture-display]
active_views: [fixture-view]

colorspaces:
  - !<ColorSpace>
    name: fixture-linear-working
    family: fixture
    bitdepth: 32f
    isdata: false
    allocation: uniform

  - !<ColorSpace>
    name: fixture-view-linear
    family: fixture
    bitdepth: 32f
    isdata: false
    allocation: uniform

  - !<ColorSpace>
    name: fixture-ui-srgb
    family: fixture
    bitdepth: 32f
    isdata: false
    allocation: uniform
    to_scene_reference: !<ExponentTransform> {{value: [2.2, 2.2, 2.2, 1.0], style: pass_thru}}
{malicious}"#
    )
    .into_bytes()
}
