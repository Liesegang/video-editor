use anyhow::{Context, Result, anyhow, bail};
use library::LibraryError;
use library::cache::CacheManager;
use library::editor::project_service::ProjectManager;
use library::model::asset::{
    SourceColorDescription, SourceColorPrimaries, SourceColorRange, SourceMatrixCoefficients,
    SourceTransferCharacteristic,
};
use library::model::{AssetKind, Project};
use library::plugin::loaders::ffmpeg_video::{FfmpegVideoLoader, VideoReader};
use library::plugin::{LoadPlugin, LoadPluginError, LoadRequest, Plugin, PluginManager};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

fn get_test_file_path(filename: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // Go up to project root
    path.push("test_data");
    path.push(filename);
    path
}

fn get_media_fixture_path(filename: &str) -> PathBuf {
    let mut path = get_test_file_path("e2e_media");
    path.push(filename);
    path
}

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Result<Self> {
        let path = std::env::temp_dir().join(format!("ruvie-video-loader-{}", Uuid::new_v4()));
        fs::create_dir(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("failed to remove video-loader test directory: {error}");
        }
    }
}

fn require_error<T, E>(result: std::result::Result<T, E>, message: &str) -> Result<E> {
    match result {
        Ok(_) => bail!("{message}"),
        Err(error) => Ok(error),
    }
}

fn video_source_metadata(filename: &str) -> Result<SourceColorDescription> {
    let path = get_media_fixture_path(filename);
    let streams = FfmpegVideoLoader::new().open(
        path.to_str()
            .context("color fixture path must contain valid UTF-8")?,
    )?;
    streams
        .into_iter()
        .find(|stream| stream.kind == AssetKind::Video)
        .map(|stream| stream.source_color)
        .context("color fixture must expose a video stream")
}

#[test]
fn ffmpeg_probes_rec709_rec2020_pq_hlg_and_untagged_sources() -> Result<()> {
    let rec709 = video_source_metadata("color_rec709_limited.mp4")?;
    assert_eq!(rec709.primaries, Some(SourceColorPrimaries::Bt709));
    assert_eq!(rec709.transfer, Some(SourceTransferCharacteristic::Bt709));
    assert_eq!(rec709.matrix, Some(SourceMatrixCoefficients::Bt709));
    assert_eq!(rec709.range, Some(SourceColorRange::Limited));
    assert_eq!(rec709.bit_depth, Some(8));

    for (fixture, transfer) in [
        ("color_rec2020_pq.mp4", SourceTransferCharacteristic::Pq),
        ("color_rec2020_hlg.mp4", SourceTransferCharacteristic::Hlg),
    ] {
        let source = video_source_metadata(fixture)?;
        assert_eq!(source.primaries, Some(SourceColorPrimaries::Bt2020));
        assert_eq!(source.transfer, Some(transfer));
        assert_eq!(
            source.matrix,
            Some(SourceMatrixCoefficients::Bt2020NonConstantLuminance)
        );
        assert_eq!(source.range, Some(SourceColorRange::Limited));
        assert_eq!(source.bit_depth, Some(10));
    }

    let untagged = video_source_metadata("color_untagged.mp4")?;
    assert_eq!(
        untagged,
        SourceColorDescription {
            bit_depth: Some(8),
            ..SourceColorDescription::default()
        },
        "codec/pixel format must not be used to guess missing color tags"
    );
    Ok(())
}

#[test]
fn stream_codec_detection_is_persisted_during_asset_import() -> Result<()> {
    let path = get_media_fixture_path("color_rec2020_hlg.mp4");
    let path = path
        .to_str()
        .context("color fixture path must contain valid UTF-8")?;
    let expected = video_source_metadata("color_rec2020_hlg.mp4")?;

    let shared = Arc::new(RwLock::new(Project::new("source color import")));
    let manager = ProjectManager::new(Arc::clone(&shared), Arc::new(PluginManager::default()));
    let imported = manager.import_file(path)?;
    let project = shared
        .read()
        .map_err(|error| anyhow!("project lock poisoned: {error}"))?;
    let asset = project
        .assets
        .iter()
        .find(|asset| imported.contains(&asset.id) && asset.kind == AssetKind::Video)
        .context("imported video Asset must exist")?;
    assert_eq!(asset.source_color.detected(), &expected);
    assert!(asset.source_color.user_override().is_none());
    Ok(())
}

#[test]
fn test_video_reader_creation() -> Result<()> {
    let path = get_test_file_path("test.mp4");
    println!("Test file path: {:?}", path);
    assert!(path.exists(), "Test file test.mp4 does not exist");

    VideoReader::new(path.to_str().context("test video path must be UTF-8")?)
        .context("failed to create VideoReader")?;
    Ok(())
}

#[test]
fn test_video_reader_metadata() -> Result<()> {
    let path = get_test_file_path("test.mp4");
    let reader = VideoReader::new(path.to_str().context("test video path must be UTF-8")?)
        .context("failed to create VideoReader")?;

    // Check FPS
    let fps = reader.get_fps();
    println!("FPS: {}", fps);
    assert!(fps > 0.0, "FPS should be positive");
    // assert!((fps - 30.0).abs() < 1.0, "FPS should be around 30");

    // Check Dimensions
    let (width, height) = reader.get_dimensions();
    println!("Dimensions: {}x{}", width, height);
    assert!(width > 0);
    assert!(height > 0);

    // Check Duration
    let duration = reader.get_duration();
    println!("Duration: {:?}", duration);
    assert!(duration.context("video duration must be present")? > 0.0);
    assert_eq!(reader.get_stream_index(), 0);
    assert_eq!(reader.get_stream_time_base(), (1, 24));
    assert_eq!(reader.get_frame_count(), Some(14_315));
    Ok(())
}

#[test]
fn test_video_reader_decode_frame() -> Result<()> {
    let path = get_test_file_path("test.mp4");
    let mut reader = VideoReader::new(path.to_str().context("test video path must be UTF-8")?)
        .context("failed to create VideoReader")?;

    // Decode frame 0
    let img0 = reader.decode_frame(0).context("failed to decode frame 0")?;
    assert_eq!(img0.width, reader.get_dimensions().0);
    assert_eq!(img0.height, reader.get_dimensions().1);
    assert!(!img0.data.is_empty());

    // Decode frame 30 (1 sec in)
    let img30 = reader
        .decode_frame(30)
        .context("failed to decode frame 30")?;
    assert_eq!(img30.width, reader.get_dimensions().0);
    assert!(!img30.data.is_empty());
    Ok(())
}

#[test]
fn late_random_access_seeks_near_the_requested_frame_and_reuses_decoder_state() -> Result<()> {
    let path = get_test_file_path("test.mp4");
    let path = path.to_str().context("test video path must be UTF-8")?;
    let mut direct_reader = VideoReader::new(path).context("failed to create VideoReader")?;
    let fps = direct_reader.get_fps();
    let total_frames = direct_reader
        .get_frame_count()
        .context("fixture has an authoritative frame count")?;
    let target = total_frames.saturating_sub((fps * 5.0).ceil() as u64);
    assert!(
        target > (fps * 60.0) as u64,
        "fixture must have a late frame"
    );

    let direct = direct_reader
        .decode_frame(target)
        .context("late random-access decode should succeed")?;
    let random_access_stats = direct_reader.last_decode_stats();
    println!("late frame {target}: {random_access_stats:?}");
    let maximum_reasonable_preroll = (fps.ceil() as u64) * 4;
    assert_eq!(random_access_stats.seek_count, 1);
    assert!(
        random_access_stats.frames_decoded <= maximum_reasonable_preroll,
        "seek decoded {} frames to reach frame {target}; it likely started near the beginning",
        random_access_stats.frames_decoded
    );
    assert!(
        random_access_stats.video_packets_read <= maximum_reasonable_preroll * 2,
        "seek read {} video packets to reach frame {target}",
        random_access_stats.video_packets_read
    );
    for random_target in [target - 2_000, target - 400, target - 4_000] {
        direct_reader
            .decode_frame(random_target)
            .context("random tail frame should decode after a bounded seek")?;
        let stats = direct_reader.last_decode_stats();
        assert_eq!(stats.seek_count, 1);
        assert_eq!(stats.target_pts, random_target as i64);
        assert_eq!(
            stats.selected_pts,
            Some(random_target as i64),
            "a backward seek must return the requested CFR frame, not stale state"
        );
        assert!(stats.frames_decoded <= maximum_reasonable_preroll);
        assert!(stats.video_packets_read <= maximum_reasonable_preroll * 2);
    }

    let mut sequential_reader = VideoReader::new(path).context("failed to create VideoReader")?;
    sequential_reader
        .decode_frame(target - 1)
        .context("neighbor frame should decode")?;
    let through_reused_state = sequential_reader
        .decode_frame(target)
        .context("sequential late frame should decode")?;
    let sequential_stats = sequential_reader.last_decode_stats();
    assert_eq!(
        sequential_stats.seek_count, 0,
        "adjacent frame should reuse decoder state"
    );
    assert_eq!(direct.width, through_reused_state.width);
    assert_eq!(direct.height, through_reused_state.height);
    assert_eq!(direct.data, through_reused_state.data);
    for sequential_target in target + 1..=target + 4 {
        sequential_reader
            .decode_frame(sequential_target)
            .context("continuous tail access should keep advancing one decoder")?;
        assert_eq!(sequential_reader.last_decode_stats().seek_count, 0);
    }
    Ok(())
}

#[test]
fn video_loader_reuses_a_thread_safe_reader() -> Result<()> {
    let path = get_test_file_path("test.mp4")
        .to_string_lossy()
        .into_owned();
    let probe = VideoReader::new(&path).context("failed to create VideoReader")?;
    let fps = probe.get_fps();
    let total_frames = probe
        .get_frame_count()
        .context("fixture has an authoritative frame count")?;
    let target = total_frames.saturating_sub((fps * 5.0).ceil() as u64);

    let loader = Arc::new(FfmpegVideoLoader::new());
    let cache = Arc::new(CacheManager::new());
    let mut workers = Vec::new();
    for offset in 0..4 {
        let loader = Arc::clone(&loader);
        let cache = Arc::clone(&cache);
        let path = path.clone();
        workers.push(std::thread::spawn(move || {
            loader.load(
                &LoadRequest::VideoFrame {
                    path,
                    source_time: (target + offset) as f64 / fps,
                    stream_index: None,
                    input_color_space: None,
                    output_color_space: None,
                },
                &cache,
            )
        }));
    }

    for worker in workers {
        let response = worker
            .join()
            .map_err(|_| anyhow!("loader worker panicked"))?
            .context("concurrent frame decode failed")?;
        assert!(response.image.width > 0);
        assert!(response.image.height > 0);
    }
    assert_eq!(
        loader.cached_reader_count(),
        1,
        "requests for one path/stream should share one stateful decoder"
    );
    Ok(())
}

#[test]
fn reader_cache_is_bounded_and_file_replacement_never_returns_a_stale_frame() -> Result<()> {
    let directory = TestDirectory::new()?;
    let loader = FfmpegVideoLoader::with_reader_capacity(2);
    let cache = CacheManager::new();

    for index in 0..3 {
        let path = directory.0.join(format!("copy-{index}.mp4"));
        fs::copy(get_media_fixture_path("h264_24.mp4"), &path)?;
        loader
            .load(
                &LoadRequest::VideoFrame {
                    path: path.to_string_lossy().into_owned(),
                    source_time: 0.0,
                    stream_index: Some(0),
                    input_color_space: None,
                    output_color_space: None,
                },
                &cache,
            )
            .context("copied video fixture must decode")?;
    }
    assert_eq!(loader.reader_capacity(), 2);
    assert_eq!(loader.cached_reader_count(), 2);

    let replaceable = directory.0.join("replaceable.mkv");
    fs::copy(get_media_fixture_path("multistream.mkv"), &replaceable)?;
    let request = || LoadRequest::VideoFrame {
        path: replaceable.to_string_lossy().into_owned(),
        source_time: 0.0,
        stream_index: Some(0),
        input_color_space: None,
        output_color_space: None,
    };
    let original = loader.load(&request(), &cache)?.image;
    assert_eq!((original.width, original.height), (8, 6));

    fs::write(&replaceable, b"not a media file")?;
    let error = require_error(
        loader.load(&request(), &cache),
        "a changed, invalid file must not return the old cached frame",
    )?;
    assert!(matches!(error, LoadPluginError::Failed(_)));

    fs::copy(get_media_fixture_path("vp9_odd.webm"), &replaceable)?;
    let replacement = loader.load(&request(), &cache)?.image;
    assert_eq!((replacement.width, replacement.height), (9, 7));
    assert_ne!(original.data, replacement.data);
    assert!(loader.cached_reader_count() <= loader.reader_capacity());
    Ok(())
}

#[test]
fn default_plugin_manager_loads_video_frames() -> Result<()> {
    let path = get_test_file_path("test.mp4");
    let manager = PluginManager::default();
    assert!(
        manager
            .get_loader_plugins()
            .iter()
            .any(|(id, _)| id == "ffmpeg_video_loader")
    );

    let response = manager
        .load_resource(
            &LoadRequest::VideoFrame {
                path: path.to_string_lossy().into_owned(),
                source_time: 0.0,
                stream_index: None,
                input_color_space: None,
                output_color_space: None,
            },
            &library::cache::CacheManager::new(),
        )
        .context("default PluginManager should decode a video frame")?;
    assert!(response.image.width > 0);
    assert!(response.image.height > 0);
    Ok(())
}

#[test]
fn first_last_and_out_of_range_frames_have_distinct_results() -> Result<()> {
    let path = get_test_file_path("test.mp4")
        .to_string_lossy()
        .into_owned();
    let mut reader = VideoReader::new(&path).context("failed to create VideoReader")?;
    let frame_count = reader
        .get_frame_count()
        .context("fixture frame count must be present")?;
    let duration = reader
        .get_duration()
        .context("fixture stream duration must be present")?;
    let fps = reader.get_fps();

    reader.decode_frame(0).context("first frame must decode")?;
    reader
        .decode_frame(frame_count - 1)
        .context("last valid frame must decode")?;

    for source_time in [duration, duration + 1.0 / fps] {
        let error = require_error(
            reader.decode_at_time(source_time),
            "the stream end and later timestamps are outside the half-open range",
        )?;
        assert!(matches!(
            error,
            LibraryError::VideoTimestampOutOfRange {
                stream_index: 0,
                duration: Some(error_duration),
                ..
            } if (error_duration - duration).abs() < f64::EPSILON
        ));
        let stats = reader.last_decode_stats();
        assert_eq!(stats.seek_count, 0);
        assert_eq!(stats.video_packets_read, 0);
        assert_eq!(stats.frames_decoded, 0);
    }

    let error = require_error(
        reader.decode_frame(frame_count),
        "frame_count is outside the valid half-open range",
    )?;
    assert!(matches!(
        error,
        library::LibraryError::VideoFrameOutOfRange {
            frame_number,
            frame_count: count,
            stream_index: 0,
            ..
        } if frame_number == frame_count && count == frame_count
    ));

    let manager = PluginManager::default();
    for source_time in [duration, duration + 1.0 / fps] {
        let error = require_error(
            manager.load_resource(
                &LoadRequest::VideoFrame {
                    path: path.clone(),
                    source_time,
                    stream_index: Some(0),
                    input_color_space: None,
                    output_color_space: None,
                },
                &CacheManager::new(),
            ),
            "manager must preserve loader out-of-range errors",
        )?;
        assert!(matches!(
            error,
            LibraryError::VideoTimestampOutOfRange { .. }
        ));
    }
    Ok(())
}

struct ClaimingFailureLoader;

impl Plugin for ClaimingFailureLoader {
    fn id(&self) -> &str {
        "claiming_failure"
    }

    fn name(&self) -> String {
        "Claiming failure fixture".to_string()
    }

    fn category(&self) -> String {
        "Test".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 0, 0)
    }
}

impl LoadPlugin for ClaimingFailureLoader {
    fn open(&self, _path: &str) -> Result<Vec<library::plugin::AssetMetadata>, LoadPluginError> {
        Err(LoadPluginError::Unsupported)
    }

    fn load(
        &self,
        request: &LoadRequest,
        _cache: &CacheManager,
    ) -> Result<library::plugin::LoadResponse, LoadPluginError> {
        let LoadRequest::VideoFrame {
            path, source_time, ..
        } = request
        else {
            return Err(LoadPluginError::Unsupported);
        };
        Err(LibraryError::VideoTimestampDecode {
            path: path.clone(),
            stream_index: 0,
            source_time: *source_time,
            source: Box::new(LibraryError::FfmpegOther(
                "synthetic valid-frame decode failure".to_string(),
            )),
        }
        .into())
    }
}

#[test]
fn manager_preserves_a_claimed_decode_failure_instead_of_reporting_no_plugin() -> Result<()> {
    let manager = PluginManager::new();
    manager.register_load_plugin(Arc::new(ClaimingFailureLoader));
    let error = require_error(
        manager.load_resource(
            &LoadRequest::VideoFrame {
                path: "claimed.mp4".to_string(),
                source_time: 42.0,
                stream_index: Some(0),
                input_color_space: None,
                output_color_space: None,
            },
            &CacheManager::new(),
        ),
        "a loader-owned decode failure must remain an error",
    )?;
    let message = error.to_string();
    assert!(matches!(
        error,
        LibraryError::VideoTimestampDecode {
            stream_index: 0,
            source_time: 42.0,
            ..
        }
    ));
    assert!(message.contains("synthetic valid-frame decode failure"));
    assert!(!message.contains("No compatible load plugin"));
    Ok(())
}

#[test]
fn loaders_report_unsupported_separately_from_failures() {
    let cache = CacheManager::new();
    let video_loader = FfmpegVideoLoader::new();
    let result = video_loader.load(
        &LoadRequest::Image {
            path: "not-a-video-request.png".to_string(),
        },
        &cache,
    );
    assert!(matches!(result, Err(LoadPluginError::Unsupported)));
}

#[test]
fn ui_frame_evaluator_and_render_service_decode_the_real_late_frame() -> Result<()> {
    use library::core::framing::FrameEvaluator;
    use library::editor::project_service::MediaNodeRequest;
    use library::model::asset::{Asset, AssetKind};
    use library::model::frame::color::Color;
    use library::model::frame::entity::{FrameContent, FrameItem};
    use library::model::project::{Composition, NodeContainer};
    use library::model::{Clip, Project};
    use library::{RenderService, SkiaRenderer};
    use ordered_float::OrderedFloat;

    let path = get_test_file_path("test.mp4")
        .to_string_lossy()
        .into_owned();
    let source_fps = 24.0;
    let source_duration = 14_315.0 / source_fps;
    let mut project = Project::new("late video integration");
    let (mut composition, track) = Composition::new("main", 16, 16, 30.0, source_duration + 1.0);
    composition.background_color = Color::black();
    let track_id = track.id;
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );

    let mut asset = Asset::new("test.mp4", &path, AssetKind::Video);
    asset.duration = Some(source_duration);
    asset.fps = Some(source_fps);
    asset.width = Some(1280);
    asset.height = Some(720);
    asset.stream_index = Some(0);
    let asset_id = asset.id;
    project.assets.push(asset);

    let clip = Clip::new("video clip", 0.0, source_duration);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    let node = support::media_node_for_canvas(
        "video",
        MediaNodeRequest::Video {
            asset_id,
            file_path: path,
            // Exercise fallback to the authoritative Asset stream metadata.
            stream_index: None,
            audio_stream_index: None,
        },
        16,
        16,
        1280,
        720,
    );
    let node_id = node.id;
    project.add_node(node);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
        .map_err(|error| anyhow!(error))?;
    project
        .set_output_node(NodeContainer::Clip(clip_id), Some(node_id))
        .map_err(|error| anyhow!(error))?;

    let plugin_manager = Arc::new(PluginManager::default());
    let late_composition_frame = 17_893;
    let frame = FrameEvaluator::new(
        &project,
        &project.compositions[0],
        plugin_manager.get_property_evaluators(),
        plugin_manager.as_ref(),
    )
    .evaluate(late_composition_frame, 1.0, None)?;

    fn video_request(item: &FrameItem) -> Option<(f64, Option<usize>)> {
        match item {
            FrameItem::Object(object) => match &object.content {
                FrameContent::Video {
                    source_time,
                    stream_index,
                    ..
                } => Some((*source_time, *stream_index)),
                _ => None,
            },
            FrameItem::Group(group) => group.items.iter().find_map(video_request),
        }
    }

    let request = frame
        .items
        .iter()
        .find_map(video_request)
        .context("late timeline time must produce a video frame")?;
    assert!((request.0 - late_composition_frame as f64 / 30.0).abs() < 1.0e-9);
    assert_eq!(request.1, Some(0));

    let renderer = SkiaRenderer::new(16, 16, Color::black(), false, None, None)?;
    let mut render_service = RenderService::new(
        renderer,
        Arc::clone(&plugin_manager),
        Arc::new(CacheManager::new()),
    );
    render_service
        .render_from_frame_info(&frame)
        .context("UI-equivalent late frame must render through PluginManager")?;

    // Extending a Clip beyond the source does not silently clamp or ask the
    // loader for a fabricated ordinal. The media node becomes NoOutput at the
    // source duration's half-open end.
    project
        .get_clip_mut(clip_id)
        .context("video Clip must exist")?
        .duration = OrderedFloat(source_duration + 1.0);
    let out_of_range_frame = FrameEvaluator::new(
        &project,
        &project.compositions[0],
        plugin_manager.get_property_evaluators(),
        plugin_manager.as_ref(),
    )
    .evaluate(17_894, 1.0, None)?;
    assert!(
        out_of_range_frame.items.is_empty(),
        "timeline time past the source must become NoOutput"
    );
    render_service
        .render_from_frame_info(&out_of_range_frame)
        .context("NoOutput outside the source range renders harmlessly")?;
    Ok(())
}
mod support;
