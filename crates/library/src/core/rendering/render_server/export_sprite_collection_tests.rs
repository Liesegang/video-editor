#![cfg(all(feature = "gl", target_os = "windows"))]

use super::*;
use crate::model::property::{ImageCollectionValue, PropertyValue};
use crate::plugin::{DecodedPixelBuffer, LoadRequest};

fn collection_project(paths: &[&std::path::Path]) -> Arc<AuthoringProject> {
    let mut project = point_tests::grid_export_project().as_ref().clone();
    let assets = paths
        .iter()
        .enumerate()
        .map(|(index, path)| {
            Asset::new(
                &format!("Sprite {index}"),
                &path.to_string_lossy(),
                AssetKind::Image,
            )
        })
        .collect::<Vec<_>>();
    let collection = ImageCollectionValue {
        assets: assets.iter().map(|asset| asset.id).collect(),
    };
    project.assets.extend(assets);
    for instance in project.module_instances.values_mut() {
        let definition = &project.module_definitions[&instance.definition_id];
        let parameter = definition
            .interface
            .parameters
            .iter()
            .find(|parameter| parameter.target.port == "sprites")
            .unwrap();
        instance.parameter_overrides.insert(
            parameter.id,
            PropertyValue::ImageCollection(collection.clone()),
        );
    }
    project.validate().unwrap();
    Arc::new(project)
}

#[cfg(all(feature = "gl", target_os = "windows"))]
#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn sprite_collection_png_export_matches_preview_and_contains_both_images() {
    let red = TemporaryPng::new();
    let green = TemporaryPng::new();
    image::RgbaImage::from_pixel(12, 4, image::Rgba([255, 0, 0, 180]))
        .save(&red.0)
        .unwrap();
    image::RgbaImage::from_pixel(4, 12, image::Rgba([0, 255, 0, 255]))
        .save(&green.0)
        .unwrap();
    let project = collection_project(&[&red.0, &green.0]);
    let pixels = assert_authoring_png_matches_preview(
        RenderServer::new(
            Arc::new(PluginManager::default()),
            Arc::new(CacheManager::new()),
        ),
        project,
        30,
    );
    let red_pixels = pixels
        .chunks_exact(4)
        .filter(|pixel| pixel[3] > 16 && u16::from(pixel[0]) > u16::from(pixel[1]) + 32)
        .count();
    let green_pixels = pixels
        .chunks_exact(4)
        .filter(|pixel| pixel[3] > 16 && u16::from(pixel[1]) > u16::from(pixel[0]) + 32)
        .count();
    assert!(
        red_pixels > 0 && green_pixels > 0,
        "both imported images must render in one frame: red={red_pixels}, green={green_pixels}"
    );
}

#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU and bundled FFmpeg"]
fn sprite_collection_video_export_decodes_both_images_from_the_real_ffmpeg_output() {
    let red = TemporaryPng::new();
    let green = TemporaryPng::new();
    image::RgbaImage::from_pixel(12, 4, image::Rgba([255, 0, 0, 255]))
        .save(&red.0)
        .unwrap();
    image::RgbaImage::from_pixel(4, 12, image::Rgba([0, 255, 0, 255]))
        .save(&green.0)
        .unwrap();
    let mut project = collection_project(&[&red.0, &green.0]).as_ref().clone();
    let one_frame = MediaTime::new(1, 30).unwrap();
    let timeline_id = project.root_timeline_id;
    let timeline = project.timelines.get_mut(&timeline_id).unwrap();
    timeline.duration = one_frame;
    // MP4's opaque delivery requires an authored scene-linear matte; the
    // exporter must never silently discard the PNG fixture's transparency.
    timeline.background_color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    for item in project.items.values_mut() {
        item.interval = TimelineInterval::new(MediaTime::zero(), one_frame).unwrap();
    }
    project.validate().unwrap();
    let project = Arc::new(project);
    let plan = Arc::new(RenderPlanCompiler::compile(project.as_ref()).unwrap());
    let plugins = Arc::new(PluginManager::default());
    let cache = Arc::new(CacheManager::new());
    let server = RenderServer::new(Arc::clone(&plugins), Arc::clone(&cache));
    let directory = tempfile::tempdir().unwrap();
    let output = directory.path().join("sprite-collection.mp4");

    assert!(server.send_authoring_video_export_request(
        RenderRequestId::new(992),
        Arc::clone(&project),
        plan,
        timeline_id,
        output.to_string_lossy().into_owned(),
    ));
    let result = server
        .rx_authoring_export_result
        .recv_timeout(Duration::from_secs(30))
        .unwrap();
    result.output.as_ref().unwrap();
    assert_eq!(result.frames_exported, 1);
    assert!(result.published);

    let decoded = plugins
        .load_resource(
            &LoadRequest::VideoFrame {
                path: output.to_string_lossy().into_owned(),
                source_time: 0.0,
                stream_index: None,
                source_color_authority: None,
            },
            cache.as_ref(),
        )
        .unwrap();
    let DecodedPixelBuffer::StraightRgba32F(decoded) = decoded.pixels() else {
        panic!("FFmpeg video decode must preserve its typed RGBAF32 output")
    };
    let red_pixels = decoded
        .data()
        .iter()
        .filter(|pixel| pixel[0] > pixel[1] + 0.125)
        .count();
    let green_pixels = decoded
        .data()
        .iter()
        .filter(|pixel| pixel[1] > pixel[0] + 0.125)
        .count();
    assert!(
        red_pixels > 0 && green_pixels > 0,
        "decoded video frame must contain both Sprite images: red={red_pixels}, green={green_pixels}"
    );
}

#[cfg(all(feature = "gl", target_os = "windows"))]
#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn sprite_collection_export_preflight_rejects_missing_source_before_writing() {
    let missing = TemporaryPng::new();
    let project = collection_project(&[&missing.0]);
    let plan = Arc::new(RenderPlanCompiler::compile(&project).unwrap());
    let timeline_id = project.root_timeline_id;
    let output = TemporaryPng::new();
    let server = RenderServer::new(
        Arc::new(PluginManager::default()),
        Arc::new(CacheManager::new()),
    );
    assert!(server.send_authoring_png_export_request(
        RenderRequestId::new(991),
        project,
        plan,
        timeline_id,
        30,
        output.0.to_string_lossy().into_owned()
    ));
    let result = server
        .rx_authoring_export_result
        .recv_timeout(Duration::from_secs(30))
        .unwrap();
    assert!(result.output.is_err());
    assert_eq!(result.frames_exported, 0);
    assert!(
        !output.0.exists(),
        "failed image preflight must not create an output"
    );
}
