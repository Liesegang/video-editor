mod support;
#[path = "media_fixture_e2e_tests/text_overlay.rs"]
mod text_overlay;

use anyhow::{Context, Result, anyhow, bail};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use library::SkiaRenderer;
use library::cache::CacheManager;
use library::editor::project_service::{GeneratorNodeRequest, MediaNodeRequest, ProjectManager};
use library::editor::{ProjectModel, RenderDestination, RenderService};
use library::framing::get_frame_from_project;
use library::model::asset::{
    SourceColorAssumption, SourceColorDescription, SourceColorPrimaries, SourceColorRange,
    SourceMatrixCoefficients, SourceTransferCharacteristic,
};
use library::model::frame::Image;
use library::model::frame::color::Color;
use library::model::frame::entity::{FrameContent, FrameItem};
use library::model::project::NodeGraphBundle;
use library::model::property::{Property, PropertyValue, Vec2};
use library::model::{
    Asset, AssetKind, Clip, Composition, Node, NodeContainer, NodeContent, Project, Track,
};
use library::plugin::{
    ExportSettings, LoadPlugin, LoadPluginError, LoadRequest, NativeImageLoader, PluginManager,
};
use library::rendering::renderer::RenderOutput;
use ordered_float::OrderedFloat;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use support::{
    generator_node_for_canvas, media_node_for_canvas, media_project_with_asset,
    transformed_image_graph,
};
use text_overlay::text_overlay_graph;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("test_data/e2e_media")
}

fn fixture(name: &str) -> String {
    fixture_dir().join(name).to_string_lossy().into_owned()
}

fn rgba_hash(image: &Image) -> u64 {
    image.data.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

fn constant(value: PropertyValue) -> Property {
    Property::constant(value)
}

fn set_declared_property(node: &mut Node, key: &str, value: PropertyValue) -> Result<()> {
    node.set_property(key.to_string(), constant(value))
        .map_err(|error| anyhow!("factory must initialize {key}: {error}"))
}

fn add_clip_node(
    project: &mut Project,
    track_id: Uuid,
    name: &str,
    node: Node,
) -> Result<(Uuid, Uuid)> {
    let clip = Clip::new(name, 0.0, 3.0);
    let clip_id = clip.id;
    let node_id = node.id;
    project.add_clip(clip);
    project.add_node(node);
    project.attach_clip_to_track(track_id, clip_id)?;
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
        .map_err(|error| anyhow!(error))?;
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(node_id))
        .map_err(|error| anyhow!(error))?;
    Ok((clip_id, node_id))
}

fn add_clip_graph(
    project: &mut Project,
    track_id: Uuid,
    name: &str,
    graph: NodeGraphBundle,
) -> Result<(Uuid, Uuid)> {
    let clip = Clip::new(name, 0.0, 3.0);
    let clip_id = clip.id;
    let output_node_id = graph
        .output_node_id
        .context("fixture graph must have an image output")?;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    project
        .insert_node_graph(NodeContainer::Clip(clip_id), graph)
        .map_err(|error| anyhow!(error))?;
    Ok((clip_id, output_node_id))
}

#[derive(Clone, Copy)]
struct MixedMediaIds {
    video_clip: Uuid,
    video_node: Uuid,
    video_transform: Uuid,
}

fn mixed_media_project(plugin_manager: &PluginManager) -> Result<(Project, MixedMediaIds)> {
    let mut project = Project::new("mixed media e2e");
    let (mut composition, solid_track) = Composition::new("main", 12, 8, 24.0, 3.0);
    composition.background_color = Color::black();
    let composition_id = composition.id;
    let solid_track_id = solid_track.id;
    assert!(
        project.add_track(solid_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );

    let solid = generator_node_for_canvas(
        "solid",
        GeneratorNodeRequest::Solid {
            color: Color {
                r: 30,
                g: 45,
                b: 60,
                a: 255,
            },
        },
        12,
        8,
        12,
        8,
    );
    add_clip_node(&mut project, solid_track_id, "solid clip", solid)?;

    let image_track = Track::new("image track");
    let image_track_id = image_track.id;
    assert!(
        project.add_track(image_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    project.attach_track_to_composition(composition_id, image_track_id)?;
    let mut image_asset = Asset::new("rgba.png", &fixture("rgba.png"), AssetKind::Image);
    image_asset.width = Some(8);
    image_asset.height = Some(6);
    let image_asset_id = image_asset.id;
    project.assets.push(image_asset);
    let image = media_node_for_canvas(
        "image",
        MediaNodeRequest::Image {
            asset_id: image_asset_id,
            file_path: fixture("rgba.png"),
        },
        12,
        8,
        8,
        6,
    );
    let (image_graph, _) = transformed_image_graph(plugin_manager, image, [6.0, 4.0], [4.0, 3.0])?;
    let (image_clip, _) = add_clip_graph(&mut project, image_track_id, "image clip", image_graph)?;
    project
        .get_clip_mut(image_clip)
        .context("image Clip must exist")?
        .properties
        .set("opacity".into(), constant(70.0.into()));

    let video_track = Track::new("video track");
    let video_track_id = video_track.id;
    assert!(
        project.add_track(video_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    project.attach_track_to_composition(composition_id, video_track_id)?;
    let mut video_asset = Asset::new("h264_24.mp4", &fixture("h264_24.mp4"), AssetKind::Video);
    video_asset.duration = Some(3.0);
    video_asset.fps = Some(24.0);
    video_asset.width = Some(12);
    video_asset.height = Some(8);
    video_asset.stream_index = Some(0);
    video_asset
        .source_color
        .replace_detected(SourceColorDescription {
            assumption: Some(SourceColorAssumption::UntaggedYuvBt709LimitedV1),
            primaries: Some(SourceColorPrimaries::Bt709),
            transfer: Some(SourceTransferCharacteristic::Bt709),
            matrix: Some(SourceMatrixCoefficients::Bt709),
            range: Some(SourceColorRange::Limited),
            bit_depth: Some(8),
            profile: None,
        });
    let video_asset_id = video_asset.id;
    project.assets.push(video_asset);
    let video = media_node_for_canvas(
        "video",
        MediaNodeRequest::Video {
            asset_id: video_asset_id,
            file_path: fixture("h264_24.mp4"),
            stream_index: None,
            audio_stream_index: None,
        },
        12,
        8,
        12,
        8,
    );
    let video_node = video.id;
    let (video_graph, video_transform) =
        transformed_image_graph(plugin_manager, video, [6.0, 4.0], [6.0, 4.0])?;
    let (video_clip, _) = add_clip_graph(&mut project, video_track_id, "video clip", video_graph)?;
    project
        .get_clip_mut(video_clip)
        .context("video Clip must exist")?
        .properties
        .set("opacity".into(), constant(65.0.into()));

    let text_track = Track::new("text track");
    let text_track_id = text_track.id;
    assert!(
        project.add_track(text_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    project.attach_track_to_composition(composition_id, text_track_id)?;
    add_clip_graph(
        &mut project,
        text_track_id,
        "text clip",
        text_overlay_graph(plugin_manager)?,
    )?;

    // Keep a real, time-dependent shader in the same Preview/Export matrix.
    // It occupies only the lower-right corner so the media layers remain
    // observable while its iTime pixels independently change every sample.
    let shader_track = Track::new("shader track");
    let shader_track_id = shader_track.id;
    assert!(
        project.add_track(shader_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    project.attach_track_to_composition(composition_id, shader_track_id)?;
    let shader_source = r#"
half4 main(float2 fragCoord) {
    float2 uv = fragCoord / iResolution.xy;
    float3 color = 0.5 + 0.5 * cos(iTime + uv.xyx * 3.0 + float3(0.0, 2.0, 4.0));
    return half4(color, 1.0);
}
"#;
    let mut shader = generator_node_for_canvas(
        "shader",
        GeneratorNodeRequest::SkSL {
            shader: shader_source.to_string(),
        },
        12,
        8,
        12,
        8,
    );
    set_declared_property(
        &mut shader,
        "width",
        PropertyValue::Number(OrderedFloat(3.0)),
    )?;
    set_declared_property(
        &mut shader,
        "height",
        PropertyValue::Number(OrderedFloat(3.0)),
    )?;
    let (shader_graph, _) =
        transformed_image_graph(plugin_manager, shader, [9.0, 5.0], [0.0, 0.0])?;
    add_clip_graph(&mut project, shader_track_id, "shader clip", shader_graph)?;

    Ok((
        project,
        MixedMediaIds {
            video_clip,
            video_node,
            video_transform,
        },
    ))
}

fn preview_frame(
    project: &Project,
    frame_number: u64,
    plugins: &Arc<PluginManager>,
) -> Result<Image> {
    let frame = get_frame_from_project(
        project,
        0,
        frame_number,
        1.0,
        None,
        &plugins.get_property_evaluators(),
        plugins,
    )?;
    let renderer = SkiaRenderer::new(
        frame.width as u32,
        frame.height as u32,
        frame.background_color.clone(),
        false,
        None,
        None,
    )?;
    let mut service =
        RenderService::new(renderer, Arc::clone(plugins), Arc::new(CacheManager::new()));
    let RenderOutput::Image(image) =
        service.render_project_frame(project, &frame, RenderDestination::Preview)?
    else {
        bail!("CPU preview renderer must return an Image");
    };
    Ok(image)
}

fn collect_content_kinds(items: &[FrameItem], kinds: &mut HashSet<&'static str>) {
    for item in items {
        match item {
            FrameItem::Object(object) => {
                kinds.insert(match object.content {
                    FrameContent::Video { .. } => "video",
                    FrameContent::Image { .. } => "image",
                    FrameContent::Text { .. } => "text",
                    FrameContent::Shape { .. } => "solid-or-shape",
                    FrameContent::SkSL { .. } => "sksl",
                    FrameContent::ParticleScene { .. } => "particle-scene",
                });
            }
            FrameItem::Group(group) => collect_content_kinds(&group.items, kinds),
            FrameItem::Transition(transition) => {
                collect_content_kinds(std::slice::from_ref(&transition.from.item), kinds);
                collect_content_kinds(std::slice::from_ref(&transition.to.item), kinds);
            }
        }
    }
}

#[test]
fn manifest_and_hash_list_cover_every_tiny_fixture() -> Result<()> {
    let directory = fixture_dir();
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.join("manifest.json"))?)?;
    let mut manifest_files = HashSet::new();
    for entry in manifest["fixtures"]
        .as_array()
        .context("fixture manifest must contain a fixtures array")?
    {
        manifest_files.insert(
            entry["file"]
                .as_str()
                .context("fixture manifest entry must contain a file name")?
                .to_string(),
        );
    }
    let checksum_contents = fs::read_to_string(directory.join("SHA256SUMS"))?;
    let mut expected_hashes = Vec::new();
    for line in checksum_contents.lines() {
        let mut fields = line.split_whitespace();
        let hash = fields
            .next()
            .context("checksum line must contain a SHA-256 digest")?;
        let name = fields
            .next()
            .context("checksum line must contain a fixture name")?;
        expected_hashes.push((hash.to_string(), name.to_string()));
    }
    let hash_files = expected_hashes
        .iter()
        .map(|(_, name)| name.clone())
        .collect::<HashSet<_>>();

    assert_eq!(manifest_files, hash_files);
    for name in manifest_files {
        let bytes = fs::read(directory.join(&name))?;
        let metadata = fs::metadata(directory.join(&name))?;
        assert!(metadata.len() > 0, "fixture {name} is empty");
        assert!(metadata.len() < 64 * 1024, "fixture {name} is not tiny");
        let expected = expected_hashes
            .iter()
            .find_map(|(hash, candidate)| (candidate == &name).then_some(hash))
            .with_context(|| format!("fixture {name} must have a checksum"))?;
        assert_eq!(format!("{:x}", Sha256::digest(&bytes)), *expected);
    }

    assert_eq!(
        manifest["loader_contract"]["native_image_extensions"],
        serde_json::json!([
            "png", "jpg", "jpeg", "bmp", "webp", "tiff", "tga", "gif", "ico", "pnm"
        ])
    );
    Ok(())
}

#[test]
fn native_image_loader_decodes_png_jpeg_and_webp_with_alpha_contracts() -> Result<()> {
    let loader = NativeImageLoader::new();
    let cache = CacheManager::new();
    let load = |name: &str| -> Result<Image> {
        Ok(loader
            .load(
                &LoadRequest::Image {
                    path: fixture(name),
                },
                &cache,
            )?
            .into_rgba8()?)
    };

    let png = load("rgba.png")?;
    let jpeg = load("rgb.jpg")?;
    let webp = load("rgba.webp")?;
    for image in [&png, &jpeg, &webp] {
        assert_eq!((image.width, image.height), (8, 6));
        assert_eq!(image.data.len(), 8 * 6 * 4);
    }
    assert!(png.data.chunks_exact(4).any(|pixel| pixel[3] < 255));
    assert!(webp.data.chunks_exact(4).any(|pixel| pixel[3] < 255));
    assert!(jpeg.data.chunks_exact(4).all(|pixel| pixel[3] == 255));
    assert_eq!(png.data, webp.data, "lossless WebP must preserve RGBA");
    assert_ne!(rgba_hash(&png), rgba_hash(&jpeg));

    assert!(matches!(
        loader.open("unsupported.svg"),
        Err(LoadPluginError::Unsupported)
    ));
    Ok(())
}

fn collect_video_times(items: &[FrameItem], times: &mut Vec<f64>) {
    for item in items {
        match item {
            FrameItem::Object(object) => {
                if let FrameContent::Video { source_time, .. } = object.content {
                    times.push(source_time);
                }
            }
            FrameItem::Group(group) => collect_video_times(&group.items, times),
            FrameItem::Transition(transition) => {
                collect_video_times(std::slice::from_ref(&transition.from.item), times);
                collect_video_times(std::slice::from_ref(&transition.to.item), times);
            }
        }
    }
}

#[test]
fn imported_frame_count_is_persisted_and_bounds_padded_video_before_render() -> Result<()> {
    let path = fixture("av_duration_mismatch.mp4");
    let plugins = Arc::new(PluginManager::default());
    let shared = Arc::new(RwLock::new(Project::new("frame bound import")));
    let manager = ProjectManager::new(Arc::clone(&shared), Arc::clone(&plugins));
    let imported_ids = manager.import_file(&path)?;
    let video = shared
        .read()
        .map_err(|error| anyhow!("project lock poisoned: {error}"))?
        .assets
        .iter()
        .find(|asset| imported_ids.contains(&asset.id) && asset.kind == AssetKind::Video)
        .cloned()
        .context("import must produce a Video Asset")?;
    assert_eq!(video.duration, Some(1.0));
    assert_eq!(video.fps, Some(12.0));
    assert_eq!(video.frame_count, Some(12));

    let (project, video_id) = media_project_with_asset(video)?;
    let saved = project.save()?;
    assert!(saved.contains("\"frame_count\":12"));
    let project = Project::load(&saved)?;
    assert_eq!(
        project
            .get_asset(video_id)
            .context("saved Video Asset must exist")?
            .frame_count,
        Some(12)
    );

    let last_valid = get_frame_from_project(
        &project,
        0,
        11,
        1.0,
        None,
        &plugins.get_property_evaluators(),
        &plugins,
    )?;
    let mut last_valid_times = Vec::new();
    collect_video_times(&last_valid.items, &mut last_valid_times);
    assert_eq!(last_valid_times, vec![11.0 / 12.0]);

    let first_invalid = get_frame_from_project(
        &project,
        0,
        12,
        1.0,
        None,
        &plugins.get_property_evaluators(),
        &plugins,
    )?;
    let mut invalid_times = Vec::new();
    collect_video_times(&first_invalid.items, &mut invalid_times);
    assert!(invalid_times.is_empty());
    assert!(
        first_invalid.items.is_empty(),
        "known source-frame overflow must become NoOutput before a loader request"
    );

    let renderer = SkiaRenderer::new(12, 8, Color::black(), false, None, None)?;
    let mut render_service = RenderService::new(
        renderer,
        Arc::clone(&plugins),
        Arc::new(CacheManager::new()),
    );
    render_service.render_project_frame(&project, &last_valid, RenderDestination::Preview)?;
    render_service.render_project_frame(&project, &first_invalid, RenderDestination::Preview)?;
    Ok(())
}

#[test]
fn mixed_media_preview_and_export_render_have_identical_first_middle_late_and_last_pixels()
-> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let (project, _) = mixed_media_project(&plugins)?;
    let frame_numbers = [0, 36, 60, 71];

    let frame_info = get_frame_from_project(
        &project,
        0,
        frame_numbers[0],
        1.0,
        None,
        &plugins.get_property_evaluators(),
        &plugins,
    )?;
    let mut content_kinds = HashSet::new();
    collect_content_kinds(&frame_info.items, &mut content_kinds);
    assert_eq!(
        content_kinds,
        HashSet::from(["solid-or-shape", "image", "video", "text", "sksl"])
    );

    let previews = frame_numbers
        .iter()
        .map(|frame_number| preview_frame(&project, *frame_number, &plugins))
        .collect::<Result<Vec<_>>>()?;
    assert!(
        previews
            .iter()
            .all(|image| (image.width, image.height) == (12, 8))
    );
    let preview_hash_values = previews.iter().map(rgba_hash).collect::<Vec<_>>();
    let preview_hashes = preview_hash_values.iter().copied().collect::<HashSet<_>>();
    assert_eq!(
        preview_hashes.len(),
        frame_numbers.len(),
        "animated source must produce distinct first/middle/late/last composites: {preview_hash_values:?}"
    );

    let settings = ExportSettings::from_project(&project, &project.compositions[0])?;
    let project_model = ProjectModel::new(Arc::new(project), 0)?;
    let renderer = SkiaRenderer::new(12, 8, Color::black(), false, None, None)?;
    let mut render_service = RenderService::new(
        renderer,
        Arc::clone(&plugins),
        Arc::new(CacheManager::new()),
    );
    for (frame_number, preview) in frame_numbers.into_iter().zip(previews) {
        let exported = render_service
            .render_export_frame(&project_model, settings.frame_time(frame_number)?)?;
        let exported = exported.image();
        assert_eq!((exported.width, exported.height), (12, 8));
        assert_eq!(
            rgba_hash(exported),
            rgba_hash(&preview),
            "Preview and export render diverged at frame {frame_number}"
        );
        assert_eq!(exported.data, preview.data);
    }
    Ok(())
}

#[test]
fn node_and_timeline_edits_share_one_model_and_update_the_next_preview() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let (mut project, ids) = mixed_media_project(&plugins)?;
    let initial = preview_frame(&project, 0, &plugins)?;

    set_declared_property(
        project
            .get_node_mut(ids.video_transform)
            .context("video Image Transform must exist")?,
        "scale",
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(0.0),
            y: OrderedFloat(0.0),
        }),
    )?;
    let after_node_edit = preview_frame(&project, 0, &plugins)?;
    assert_ne!(rgba_hash(&initial), rgba_hash(&after_node_edit));
    assert_eq!(
        project.find_node_container(ids.video_transform),
        Some(NodeContainer::Clip(ids.video_clip))
    );
    assert_eq!(
        project
            .get_clip(ids.video_clip)
            .context("video Clip must exist")?
            .node_ids,
        vec![ids.video_node, ids.video_transform]
    );

    set_declared_property(
        project
            .get_node_mut(ids.video_transform)
            .context("video Image Transform must exist")?,
        "scale",
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(100.0),
            y: OrderedFloat(100.0),
        }),
    )?;
    let clip = project
        .get_clip_mut(ids.video_clip)
        .context("video Clip must exist")?;
    clip.start_time = OrderedFloat(1.0);
    clip.duration = OrderedFloat(2.0);

    let frame_zero = get_frame_from_project(
        &project,
        0,
        0,
        1.0,
        None,
        &plugins.get_property_evaluators(),
        &plugins,
    )?;
    let mut frame_zero_kinds = HashSet::new();
    collect_content_kinds(&frame_zero.items, &mut frame_zero_kinds);
    assert!(!frame_zero_kinds.contains("video"));

    let frame_at_start = get_frame_from_project(
        &project,
        0,
        24,
        1.0,
        None,
        &plugins.get_property_evaluators(),
        &plugins,
    )?;
    let mut frame_at_start_kinds = HashSet::new();
    collect_content_kinds(&frame_at_start.items, &mut frame_at_start_kinds);
    assert!(frame_at_start_kinds.contains("video"));
    assert!(matches!(
        project
            .get_node(ids.video_node)
            .context("video Node must exist")?
            .content(),
        NodeContent::Media(_)
    ));
    assert_ne!(
        rgba_hash(&preview_frame(&project, 0, &plugins)?),
        rgba_hash(&preview_frame(&project, 24, &plugins)?)
    );
    Ok(())
}

fn solid_node(name: &str, color: Color) -> Node {
    generator_node_for_canvas(name, GeneratorNodeRequest::Solid { color }, 4, 4, 4, 4)
}

#[test]
fn track_and_clip_reordering_change_pixels_immediately() -> Result<()> {
    let plugins = Arc::new(PluginManager::default());
    let red = Color {
        r: 255,
        g: 0,
        b: 0,
        a: 255,
    };
    let blue = Color {
        r: 0,
        g: 0,
        b: 255,
        a: 255,
    };

    let mut track_project = Project::new("track order");
    let (composition, first_track) = Composition::new("main", 4, 4, 1.0, 1.0);
    let composition_id = composition.id;
    let first_track_id = first_track.id;
    assert!(
        track_project.add_track(first_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        track_project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );
    add_clip_node(
        &mut track_project,
        first_track_id,
        "red clip",
        solid_node("red", red.clone()),
    )?;
    let second_track = Track::new("blue track");
    let second_track_id = second_track.id;
    assert!(
        track_project.add_track(second_track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    track_project.attach_track_to_composition(composition_id, second_track_id)?;
    add_clip_node(
        &mut track_project,
        second_track_id,
        "blue clip",
        solid_node("blue", blue.clone()),
    )?;
    let before_track_move = preview_frame(&track_project, 0, &plugins)?;
    assert_eq!(&before_track_move.data[0..4], &[0, 0, 255, 255]);
    assert!(track_project.move_track_within_composition(composition_id, second_track_id, 0)?);
    let after_track_move = preview_frame(&track_project, 0, &plugins)?;
    assert_eq!(&after_track_move.data[0..4], &[255, 0, 0, 255]);

    let mut clip_project = Project::new("clip order");
    let (composition, track) = Composition::new("main", 4, 4, 1.0, 1.0);
    let track_id = track.id;
    assert!(
        clip_project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        clip_project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );
    let (red_clip_id, _) = add_clip_node(
        &mut clip_project,
        track_id,
        "red clip",
        solid_node("red", red),
    )?;
    let (blue_clip_id, _) = add_clip_node(
        &mut clip_project,
        track_id,
        "blue clip",
        solid_node("blue", blue),
    )?;
    let before_clip_move = preview_frame(&clip_project, 0, &plugins)?;
    assert_eq!(&before_clip_move.data[0..4], &[0, 0, 255, 255]);
    clip_project.attach_clip_to_track_at(track_id, red_clip_id, Some(1))?;
    assert_eq!(
        clip_project
            .get_track(track_id)
            .context("clip Track must exist")?
            .clip_ids,
        vec![blue_clip_id, red_clip_id]
    );
    let after_clip_move = preview_frame(&clip_project, 0, &plugins)?;
    assert_eq!(&after_clip_move.data[0..4], &[255, 0, 0, 255]);
    Ok(())
}
