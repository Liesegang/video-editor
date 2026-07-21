mod support;
#[path = "media_fixture_e2e_tests/text_overlay.rs"]
mod text_overlay;

use anyhow::{Context, Result, anyhow, bail};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use library::cache::CacheManager;
use library::core::audio::cache::{AudioChunkKey, AudioDecodeFormat, AudioSourceKey};
use library::core::audio::loader::AudioLoader;
use library::core::audio::mixer::{mix_samples, render_samples};
use library::editor::project_service::{GeneratorNodeRequest, MediaNodeRequest, ProjectManager};
use library::framing::get_frame_from_project;
use library::model::frame::Image;
use library::model::frame::color::Color;
use library::model::frame::entity::{FrameContent, FrameItem};
use library::model::project::{
    IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, NodeGraphBundle, PortAddress, PortOwner, ProjectConnection,
};
use library::model::property::{Property, PropertyValue, Vec2};
use library::model::{
    Asset, AssetKind, Clip, Composition, Node, NodeContainer, NodeContent, Project, Track,
};
use library::plugin::loaders::ffmpeg_video::{FfmpegVideoLoader, VideoReader};
use library::plugin::{
    ExportSettings, LoadPlugin, LoadPluginError, LoadRequest, NativeImageLoader, PluginManager,
};
use library::rendering::renderer::RenderOutput;
use library::{EditorService, ExportService, ProjectModel, RenderService, SkiaRenderer};
use ordered_float::OrderedFloat;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use support::{
    channel_energy, generator_node_for_canvas, media_node_for_canvas, positive_zero_crossings,
};
use text_overlay::text_overlay_graph;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
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

fn transformed_image_graph(
    plugin_manager: &PluginManager,
    source: Node,
    position: [f64; 2],
    anchor: [f64; 2],
) -> Result<(NodeGraphBundle, Uuid)> {
    let source_id = source.id;
    let mut transform = plugin_manager.create_image_transform_operation_node()?;
    set_declared_property(
        &mut transform,
        "position",
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(position[0]),
            y: OrderedFloat(position[1]),
        }),
    )?;
    set_declared_property(
        &mut transform,
        "anchor",
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(anchor[0]),
            y: OrderedFloat(anchor[1]),
        }),
    )?;
    let transform_id = transform.id;
    Ok((
        NodeGraphBundle::new(
            vec![source, transform],
            vec![ProjectConnection::new(
                PortAddress::new(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
                PortAddress::new(PortOwner::Node(transform_id), IMAGE_INPUT_PORT),
                0,
            )],
            Some(transform_id),
        ),
        transform_id,
    ))
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
    let RenderOutput::Image(image) = service.render_from_frame_info(&frame)? else {
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
                });
            }
            FrameItem::Group(group) => collect_content_kinds(&group.items, kinds),
        }
    }
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Result<Self> {
        let path = std::env::temp_dir().join(format!("video-editor-media-e2e-{}", Uuid::new_v4()));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("failed to remove media E2E test directory: {error}");
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
            .image)
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

#[test]
fn ffmpeg_loader_decodes_container_codec_dimension_alpha_and_stream_matrix() -> Result<()> {
    let decode_three = |name: &str,
                        expected_dimensions: (u32, u32),
                        expected_fps: f64,
                        frames: [u64; 3]|
     -> Result<Vec<Image>> {
        let mut reader = VideoReader::new(&fixture(name))?;
        assert_eq!(reader.get_dimensions(), expected_dimensions);
        assert!((reader.get_fps() - expected_fps).abs() < 0.001);
        let mut images = Vec::with_capacity(frames.len());
        for frame in frames {
            images.push(reader.decode_frame(frame)?);
        }
        Ok(images)
    };

    let mp4 = decode_three("h264_24.mp4", (12, 8), 24.0, [0, 36, 71])?;
    let mov = decode_three("h264_24.mov", (12, 8), 24.0, [0, 36, 71])?;
    assert_eq!(
        mp4.iter().map(rgba_hash).collect::<Vec<_>>(),
        mov.iter().map(rgba_hash).collect::<Vec<_>>(),
        "remuxing H.264 between MP4 and MOV must not change decoded pixels"
    );
    assert_ne!(rgba_hash(&mp4[0]), rgba_hash(&mp4[1]));
    assert_ne!(rgba_hash(&mp4[1]), rgba_hash(&mp4[2]));

    let webm = decode_three("vp9_odd.webm", (9, 7), 15.0, [0, 15, 29])?;
    assert!(
        webm.windows(2)
            .all(|pair| rgba_hash(&pair[0]) != rgba_hash(&pair[1]))
    );

    let ffv1 = decode_three("ffv1_alpha.mkv", (7, 5), 12.0, [0, 6, 11])?;
    assert!(
        ffv1.iter()
            .all(|image| image.data.chunks_exact(4).any(|pixel| pixel[3] < 255))
    );
    assert!(
        ffv1.windows(2)
            .all(|pair| rgba_hash(&pair[0]) != rgba_hash(&pair[1]))
    );

    let mut red = VideoReader::new_with_stream(&fixture("multistream.mkv"), Some(0))?;
    let mut blue = VideoReader::new_with_stream(&fixture("multistream.mkv"), Some(1))?;
    let red = red.decode_frame(0)?;
    let blue = blue.decode_frame(0)?;
    assert_eq!((red.width, red.height), (8, 6));
    assert_eq!((blue.width, blue.height), (8, 6));
    assert!(red.data[0] > red.data[2], "stream 0 must be red");
    assert!(blue.data[2] > blue.data[0], "stream 1 must be blue");
    assert_ne!(rgba_hash(&red), rgba_hash(&blue));
    Ok(())
}

#[test]
fn vfr_sampling_uses_pts_instead_of_advertised_fps_ordinals() -> Result<()> {
    let path = fixture("vfr_pts.mkv");
    let mut sequential = VideoReader::new(&path)?;
    assert_eq!(sequential.get_stream_time_base(), (1, 1000));
    assert!((sequential.get_fps() - 10.0).abs() < 0.001);
    assert_eq!(
        sequential.get_frame_count(),
        None,
        "duration multiplied by advertised FPS must not fabricate an ordinal bound"
    );

    let first = sequential.decode_at_time(0.0)?;
    assert_eq!(sequential.last_decode_stats().selected_pts, Some(0));
    let second = sequential.decode_at_time(0.1)?;
    assert_eq!(sequential.last_decode_stats().selected_pts, Some(100));
    assert_ne!(rgba_hash(&first), rgba_hash(&second));
    let at_half_second = sequential.decode_at_time(0.5)?;
    assert_eq!(sequential.last_decode_stats().selected_pts, Some(500));
    let held_at_one_second = sequential.decode_at_time(1.0)?;
    assert_eq!(sequential.last_decode_stats().selected_pts, Some(500));
    let tail = sequential.decode_at_time(1.85)?;
    assert_eq!(sequential.last_decode_stats().selected_pts, Some(1800));
    assert_eq!(
        at_half_second.data, held_at_one_second.data,
        "the 0.5s frame must remain displayed until the next PTS at 1.8s"
    );
    assert_ne!(rgba_hash(&held_at_one_second), rgba_hash(&tail));

    let mut random = VideoReader::new(&path)?;
    let random_at_one_second = random.decode_at_time(1.0)?;
    assert_eq!(random_at_one_second.data, held_at_one_second.data);
    let stats = random.last_decode_stats();
    assert_eq!(stats.target_pts, 1000);
    assert_eq!(stats.selected_pts, Some(500));
    assert_eq!(stats.seek_count, 1);
    assert!(stats.frames_decoded <= 4);
    Ok(())
}

#[test]
fn timestamp_range_errors_report_the_selected_stream_duration() -> Result<()> {
    let path = fixture("av_duration_mismatch.mp4");
    let mut reader = VideoReader::new_with_stream(&path, Some(0))?;
    let error = match reader.decode_at_time(1.0) {
        Ok(_) => bail!("the selected video stream unexpectedly reached the padded container"),
        Err(error) => error,
    };
    let library::LibraryError::VideoTimestampOutOfRange {
        stream_index,
        duration,
        ..
    } = error
    else {
        bail!("expected a timestamp range error, got {error}");
    };
    assert_eq!(stream_index, 0);
    let duration = duration.context("fixture video stream declares its own duration")?;
    assert!((duration - 1.0).abs() < f64::EPSILON);
    assert!(
        (duration - 2.0).abs() > f64::EPSILON,
        "the two-second container/audio duration must not leak into the video error"
    );
    Ok(())
}

#[test]
fn loader_stream_selection_and_audio_only_probe_do_not_share_the_best_video() -> Result<()> {
    let loader = FfmpegVideoLoader::new();
    let cache = CacheManager::new();
    let path = fixture("multistream.mkv");
    let load_stream = |stream_index| -> Result<Image> {
        Ok(loader
            .load(
                &LoadRequest::VideoFrame {
                    path: path.clone(),
                    source_time: 0.0,
                    stream_index: Some(stream_index),
                    input_color_space: None,
                    output_color_space: None,
                },
                &cache,
            )?
            .image)
    };
    let red = load_stream(0)?;
    let blue = load_stream(1)?;
    assert!(red.data[0] > red.data[2]);
    assert!(blue.data[2] > blue.data[0]);
    assert_ne!(rgba_hash(&red), rgba_hash(&blue));
    assert_eq!(loader.cached_reader_count(), 2);

    let audio_streams = loader.open(&fixture("tone.mp3"))?;
    assert_eq!(audio_streams.len(), 1);
    assert_eq!(audio_streams[0].kind, AssetKind::Audio);
    assert_eq!(
        loader.cached_reader_count(),
        2,
        "metadata probing must not require or cache a video decoder"
    );
    Ok(())
}

#[test]
fn dedicated_audio_loader_decodes_the_tiny_mp3_as_interleaved_stereo() -> Result<()> {
    let path = fixture("tone.mp3");
    assert!(AudioLoader::has_audio(&path));
    let format =
        AudioDecodeFormat::new(48_000, 2).context("48 kHz stereo decode format must be valid")?;
    let source = AudioSourceKey::read(&path, None, format)?;
    let chunk = AudioLoader::decode_chunk(&AudioChunkKey {
        source,
        chunk_index: 0,
    })?;
    let samples = chunk.samples();
    assert!(samples.len() > 90_000);
    assert_eq!(samples.len() % 2, 0);
    assert!(samples.iter().any(|sample| sample.abs() > 0.01));
    assert!(
        samples
            .chunks_exact(2)
            .take(1_000)
            .all(|frame| (frame[0] - frame[1]).abs() < f32::EPSILON)
    );
    Ok(())
}

#[test]
fn explicit_global_audio_stream_ordinals_decode_distinct_signals() -> Result<()> {
    let path = fixture("multi_audio.mkv");
    let format =
        AudioDecodeFormat::new(8_000, 2).context("8 kHz stereo decode format must be valid")?;
    let decode = |stream_index| -> Result<library::audio::cache::AudioChunk> {
        let source = AudioSourceKey::read(&path, stream_index, format)?;
        AudioLoader::decode_chunk(&AudioChunkKey {
            source,
            chunk_index: 0,
        })
    };

    assert!(
        AudioLoader::decode_chunk(&AudioChunkKey {
            source: AudioSourceKey::read(&path, Some(0), format)?,
            chunk_index: 0,
        })
        .is_err()
    );
    let default_audio = decode(None)?;
    let stream_one = decode(Some(1))?;
    let stream_two = decode(Some(2))?;

    assert_eq!(default_audio.samples(), stream_one.samples());
    assert!(channel_energy(stream_one.samples(), 0) > 0.001);
    assert!(channel_energy(stream_one.samples(), 1) < 0.000_001);
    assert!(channel_energy(stream_two.samples(), 1) > 0.001);
    assert!(channel_energy(stream_two.samples(), 0) < 0.000_001);
    let crossings_one = positive_zero_crossings(stream_one.samples(), 0);
    let crossings_two = positive_zero_crossings(stream_two.samples(), 1);
    assert!(
        (420..=460).contains(&crossings_one),
        "unexpected stream 1 frequency proxy: {crossings_one} crossings"
    );
    assert!(
        (850..=910).contains(&crossings_two),
        "unexpected stream 2 frequency proxy: {crossings_two} crossings"
    );
    Ok(())
}

#[test]
fn cold_render_survives_high_stretch_with_a_two_chunk_cache() -> Result<()> {
    let mut project = Project::new("bounded cold audio render");
    let (composition, track) = Composition::new("main", 8, 6, 12.0, 1.25);
    let composition_id = composition.id;
    let track_id = track.id;
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );

    let mut asset = Asset::new(
        "multi audio video",
        &fixture("multi_audio.mkv"),
        AssetKind::Video,
    );
    asset.stream_index = Some(0);
    let asset_id = asset.id;
    project.assets.push(asset);
    let mut clip = Clip::new("retimed audio", 0.0, 1.25);
    clip.time_stretch = OrderedFloat(2.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    let node = media_node_for_canvas(
        "explicit second audio stream",
        MediaNodeRequest::Video {
            asset_id,
            file_path: fixture("multi_audio.mkv"),
            stream_index: Some(0),
            audio_stream_index: Some(2),
        },
        8,
        6,
        8,
        6,
    );
    let node_id = node.id;
    project.add_node(node);
    support::attach_audio_output(&mut project, NodeContainer::Clip(clip_id), node_id)
        .map_err(|error| anyhow!(error))?;

    let cache = CacheManager::with_audio_chunk_capacity(2);
    let plugin_manager = PluginManager::default();
    let rendered = render_samples(
        &project.assets,
        &project,
        project
            .get_composition(composition_id)
            .context("audio composition must exist")?,
        &cache,
        0,
        10_000,
        8_000,
        2,
        &plugin_manager,
    );

    assert_eq!(rendered.len(), 20_000);
    assert!(cache.audio_chunk_cache_len() <= 2);
    assert!(cache.cached_audio_sample_count() <= 2 * 8_000 * 2);
    assert!(channel_energy(&rendered[..2_000], 1) > 0.001);
    assert!(channel_energy(&rendered[18_000..], 1) > 0.001);
    assert!(channel_energy(&rendered, 0) < 0.000_001);
    Ok(())
}

fn media_project_with_asset(asset: Asset) -> Result<(Project, Uuid)> {
    let mut project = Project::new("embedded audio integration");
    let (composition, track) = Composition::new("main", 12, 8, 12.0, 2.0);
    let track_id = track.id;
    let asset_id = asset.id;
    let file_path = asset.path.clone();
    let media_width = u64::from(asset.width.unwrap_or(12));
    let media_height = u64::from(asset.height.unwrap_or(8));
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );
    project.assets.push(asset);

    let clip = Clip::new("padded media clip", 0.0, 2.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    let node = media_node_for_canvas(
        "embedded audio video",
        MediaNodeRequest::Video {
            asset_id,
            file_path,
            stream_index: None,
            audio_stream_index: None,
        },
        12,
        8,
        media_width,
        media_height,
    );
    let node_id = node.id;
    project.add_node(node);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
        .map_err(|error| anyhow!(error))?;
    support::bind_av_output(&mut project, NodeContainer::Clip(clip_id), node_id)
        .map_err(|error| anyhow!(error))?;
    Ok((project, asset_id))
}

fn wait_for_audio(
    service: &EditorService,
    cache: &CacheManager,
    project: &Project,
    asset_id: Uuid,
    sample_rate: u32,
) -> Result<Arc<library::audio::cache::AudioChunk>> {
    let composition_id = project
        .compositions
        .first()
        .context("audio project must contain a composition")?
        .id;
    service.set_active_composition(Some(composition_id), 0.0);
    service.reset_audio_pump(0.0);
    let asset = project
        .get_asset(asset_id)
        .context("embedded-audio Asset must exist")?;
    let format = AudioDecodeFormat::new(sample_rate, 2)
        .context("editor sample rate must form a valid stereo decode format")?;
    let source = AudioSourceKey::read(&asset.path, None, format)?;
    let key = AudioChunkKey {
        source,
        chunk_index: 0,
    };
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        service.pump_audio();
        if let Some(audio) = cache.get_audio_chunk(&key) {
            return Ok(audio);
        }
        if Instant::now() >= deadline {
            bail!("timed out hydrating embedded audio for Asset {asset_id}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn assert_nonzero_mix(
    service: &EditorService,
    project: &Project,
    cache: &CacheManager,
    sample_rate: u32,
    asset_id: Uuid,
) -> Result<()> {
    let cached = wait_for_audio(service, cache, project, asset_id, sample_rate)?;
    assert!(cached.samples().iter().any(|sample| sample.abs() > 0.001));
    let composition = project
        .compositions
        .first()
        .context("audio project must contain a composition")?;
    let mixed = mix_samples(
        &project.assets,
        project,
        composition,
        cache,
        0,
        (sample_rate / 4) as usize,
        sample_rate,
        2,
        service.get_plugin_manager().as_ref(),
    );
    assert!(
        mixed.iter().any(|sample| sample.abs() > 0.001),
        "hydrated Video Asset must contribute embedded audio under its own ID"
    );
    Ok(())
}

#[test]
fn import_and_load_hydrate_embedded_audio_under_the_video_asset_id() -> Result<()> {
    let path = fixture("av_duration_mismatch.mp4");
    assert!(AudioLoader::has_audio(&path));
    assert!(
        !AudioLoader::has_audio(&fixture("h264_24.mp4")),
        "a video codec must not be mistaken for an audio track"
    );

    let plugins = Arc::new(PluginManager::default());
    let shared = Arc::new(RwLock::new(Project::new("import target")));
    let cache = Arc::new(CacheManager::new());
    let service = EditorService::new(
        Arc::clone(&shared),
        Arc::clone(&plugins),
        Arc::clone(&cache),
    )?;
    let sample_rate = service.get_audio_engine().get_sample_rate();

    let imported_ids = service.import_file(&path)?;
    let imported_video = shared
        .read()
        .map_err(|error| anyhow!("project lock poisoned: {error}"))?
        .assets
        .iter()
        .find(|asset| imported_ids.contains(&asset.id) && asset.kind == AssetKind::Video)
        .cloned()
        .context("import must produce a Video Asset")?;
    assert_eq!(imported_video.frame_count, Some(12));
    assert_eq!(imported_video.stream_index, Some(0));

    let (imported_project, imported_video_id) = media_project_with_asset(imported_video)?;
    service.set_project(imported_project.clone())?;
    assert_nonzero_mix(
        &service,
        &imported_project,
        &cache,
        sample_rate,
        imported_video_id,
    )?;

    let mut loaded_asset = Asset::new("loaded AV", &path, AssetKind::Video);
    loaded_asset.duration = Some(2.0);
    loaded_asset.fps = Some(12.0);
    loaded_asset.frame_count = Some(12);
    loaded_asset.width = Some(12);
    loaded_asset.height = Some(8);
    loaded_asset.stream_index = Some(0);
    let (loaded_project, loaded_video_id) = media_project_with_asset(loaded_asset)?;
    service.load_project(&loaded_project.save()?)?;
    let loaded_snapshot = shared
        .read()
        .map_err(|error| anyhow!("project lock poisoned: {error}"))?
        .clone();
    assert_eq!(
        loaded_snapshot
            .get_asset(loaded_video_id)
            .context("loaded Video Asset must exist")?
            .frame_count,
        Some(12)
    );
    assert_nonzero_mix(
        &service,
        &loaded_snapshot,
        &cache,
        sample_rate,
        loaded_video_id,
    )?;
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
    render_service.render_from_frame_info(&last_valid)?;
    render_service.render_from_frame_info(&first_invalid)?;
    Ok(())
}

#[test]
fn mixed_media_preview_and_png_export_have_identical_first_middle_late_and_last_pixels()
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

    let output = TestDirectory::new()?;
    let project_model = ProjectModel::new(Arc::new(project), 0)?;
    let renderer = SkiaRenderer::new(12, 8, Color::black(), false, None, None)?;
    let mut render_service = RenderService::new(
        renderer,
        Arc::clone(&plugins),
        Arc::new(CacheManager::new()),
    );
    let settings = Arc::new(ExportSettings::for_dimensions(12, 8, 24.0));
    let mut exporter = ExportService::new(Arc::clone(&plugins), "png_export".into(), settings, 2);
    let mut exported_paths = Vec::new();
    let export_result = (|| -> Result<()> {
        for frame_number in frame_numbers {
            let stem = output.0.join(format!("frame_{frame_number}"));
            exporter.render_range(
                &mut render_service,
                &project_model,
                frame_number..frame_number + 1,
                stem.to_str().context("export path must be UTF-8")?,
            )?;
            exported_paths.push(PathBuf::from(format!(
                "{}_{frame_number:03}.png",
                stem.to_string_lossy()
            )));
        }
        Ok(())
    })();
    let shutdown_result = exporter.shutdown();
    export_result.context("PNG frames must export")?;
    shutdown_result.context("PNG exporter must shut down cleanly")?;

    let loader = NativeImageLoader::new();
    let cache = CacheManager::new();
    for ((frame_number, preview), exported_path) in
        frame_numbers.into_iter().zip(previews).zip(exported_paths)
    {
        let exported = loader
            .load(
                &LoadRequest::Image {
                    path: exported_path.to_string_lossy().into_owned(),
                },
                &cache,
            )?
            .image;
        assert_eq!((exported.width, exported.height), (12, 8));
        assert_eq!(
            rgba_hash(&exported),
            rgba_hash(&preview),
            "Preview and PNG export diverged at frame {frame_number}"
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
