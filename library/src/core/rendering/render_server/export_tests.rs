use super::export::authoring_video_frame_count;
use super::{RenderRequestId, RenderServer};
use crate::cache::CacheManager;
use crate::core::render_plan::RenderPlanCompiler;
use crate::error::LibraryError;
use crate::model::authoring::{
    AuthoringProject, MediaTime, RationalRate, SourceRef, TimeMap, TimelineInterval, TimelineItem,
    TimelineItemId,
};
use crate::model::project::asset::{Asset, AssetKind};
use crate::model::project::property::PropertyMap;
use crate::plugin::{ExportFrame, ExportPlugin, ExportSettings, Plugin, PluginManager};
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

struct TemporaryPng(PathBuf);

impl TemporaryPng {
    fn new() -> Self {
        Self(std::env::temp_dir().join(format!("ruvie-authoring-export-{}.png", Uuid::new_v4())))
    }
}

impl Drop for TemporaryPng {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_file(&self.0)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            eprintln!("failed to remove authoring export test PNG: {error}");
        }
    }
}

#[derive(Default)]
struct MockVideoState {
    frame_dimensions: Vec<(u32, u32)>,
    fps: Vec<f64>,
    containers: Vec<String>,
    finishes: usize,
    runtime_audio: Vec<Option<RuntimeAudioCapture>>,
}

#[derive(Debug)]
struct RuntimeAudioCapture {
    path: PathBuf,
    channels: u16,
    sample_rate: u32,
    bytes: Vec<u8>,
}

struct MockVideoExporter {
    state: Arc<Mutex<MockVideoState>>,
}

impl Plugin for MockVideoExporter {
    fn id(&self) -> &str {
        "ffmpeg_export"
    }

    fn name(&self) -> String {
        "Mock video export".to_string()
    }

    fn category(&self) -> String {
        "Export".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 0, 1)
    }
}

impl ExportPlugin for MockVideoExporter {
    fn export_frame(
        &self,
        _path: &str,
        frame: &ExportFrame,
        settings: &ExportSettings,
    ) -> Result<(), LibraryError> {
        settings.require_matching_color_authority(frame)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| LibraryError::Runtime("Mock video state lock poisoned".to_string()))?;
        state
            .frame_dimensions
            .push((frame.image().width, frame.image().height));
        state.fps.push(settings.fps);
        state.containers.push(settings.container.clone());
        state.runtime_audio.push(
            settings
                .runtime_audio_source()
                .map(
                    |(path, channels, sample_rate)| -> Result<RuntimeAudioCapture, LibraryError> {
                        Ok(RuntimeAudioCapture {
                            path: PathBuf::from(path),
                            channels,
                            sample_rate,
                            bytes: fs::read(path)?,
                        })
                    },
                )
                .transpose()?,
        );
        Ok(())
    }

    fn finish_export(&self, _path: &str, _settings: &ExportSettings) -> Result<(), LibraryError> {
        self.state
            .lock()
            .map_err(|_| LibraryError::Runtime("Mock video state lock poisoned".to_string()))?
            .finishes += 1;
        Ok(())
    }
}

#[test]
fn authoring_png_export_is_full_frame_and_independent_from_preview() {
    let project = Arc::new(
        AuthoringProject::new(
            "authoring PNG",
            5,
            3,
            RationalRate::new(24, 1).unwrap(),
            MediaTime::new(1, 1).unwrap(),
        )
        .unwrap(),
    );
    let timeline_id = project.root_timeline_id;
    let plan = Arc::new(RenderPlanCompiler::compile(project.as_ref()).unwrap());
    let output = TemporaryPng::new();
    let output_path = output.0.to_string_lossy().into_owned();
    let server = RenderServer::new(
        Arc::new(PluginManager::default()),
        Arc::new(CacheManager::new()),
    );

    assert!(server.send_authoring_request(
        RenderRequestId::new(44),
        Arc::clone(&project),
        Arc::clone(&plan),
        timeline_id,
        0,
        1.0,
        None,
    ));
    assert!(server.send_authoring_png_export_request(
        RenderRequestId::new(45),
        project,
        plan,
        timeline_id,
        0,
        output_path.clone(),
    ));

    let exported = server
        .rx_authoring_export_result
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    exported.output.unwrap();
    assert_eq!(exported.request_id, RenderRequestId::new(45));
    assert_eq!(exported.timeline_id, timeline_id);
    assert_eq!(exported.output_path, output_path);
    assert_eq!(
        (exported.frame_info.width, exported.frame_info.height),
        (5, 3)
    );
    assert!(exported.frame_info.region.is_none());
    assert_eq!(exported.frame_info.render_scale.into_inner(), 1.0);

    let preview = server
        .rx_authoring_result
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    assert_eq!(preview.request_id, RenderRequestId::new(44));

    let decoder = png::Decoder::new(std::io::BufReader::new(
        std::fs::File::open(&output.0).unwrap(),
    ));
    let reader = decoder.read_info().unwrap();
    assert_eq!((reader.info().width, reader.info().height), (5, 3));
}

#[test]
fn authoring_png_export_refuses_to_overwrite_an_asset() {
    let protected = TemporaryPng::new();
    fs::write(&protected.0, b"source bytes must survive").unwrap();
    let protected_path = protected.0.to_string_lossy().into_owned();
    let mut project = AuthoringProject::new(
        "safe authoring PNG",
        2,
        2,
        RationalRate::new(24, 1).unwrap(),
        MediaTime::new(1, 1).unwrap(),
    )
    .unwrap();
    project.assets.push(Asset::new(
        "protected input",
        &protected_path,
        AssetKind::Image,
    ));
    let timeline_id = project.root_timeline_id;
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let server = RenderServer::new(
        Arc::new(PluginManager::default()),
        Arc::new(CacheManager::new()),
    );

    assert!(server.send_authoring_png_export_request(
        RenderRequestId::new(46),
        Arc::new(project),
        Arc::new(plan),
        timeline_id,
        0,
        protected_path,
    ));
    let exported = server
        .rx_authoring_export_result
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    let error = exported.output.unwrap_err();

    assert!(error.to_string().contains("refusing to overwrite"));
    assert_eq!(
        fs::read(&protected.0).unwrap(),
        b"source bytes must survive"
    );
}

#[test]
fn authoring_video_frame_count_uses_exact_ceil_for_partial_last_frame() {
    let project = AuthoringProject::new(
        "exact video range",
        2,
        2,
        RationalRate::new(24, 1).unwrap(),
        MediaTime::new(5, 48).unwrap(),
    )
    .unwrap();

    assert_eq!(
        authoring_video_frame_count(&project, project.root_timeline_id).unwrap(),
        3
    );
}

#[test]
fn authoring_video_export_streams_the_complete_timeline_then_finishes() {
    let project = Arc::new(
        AuthoringProject::new(
            "authoring video",
            5,
            3,
            RationalRate::new(24, 1).unwrap(),
            MediaTime::new(5, 48).unwrap(),
        )
        .unwrap(),
    );
    let timeline_id = project.root_timeline_id;
    let plan = Arc::new(RenderPlanCompiler::compile(project.as_ref()).unwrap());
    let state = Arc::new(Mutex::new(MockVideoState::default()));
    let plugins = Arc::new(PluginManager::new());
    plugins.register_export_plugin(Arc::new(MockVideoExporter {
        state: Arc::clone(&state),
    }));
    let server = RenderServer::new(plugins, Arc::new(CacheManager::new()));
    let output_path = std::env::temp_dir()
        .join(format!("ruvie-authoring-video-{}.mp4", Uuid::new_v4()))
        .to_string_lossy()
        .into_owned();

    assert!(server.send_authoring_video_export_request(
        RenderRequestId::new(47),
        project,
        plan,
        timeline_id,
        output_path.clone(),
    ));
    let exported = server
        .rx_authoring_export_result
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    exported.output.unwrap();

    assert_eq!(exported.request_id, RenderRequestId::new(47));
    assert_eq!(exported.output_path, output_path);
    assert_eq!(exported.frames_exported, 3);
    assert_eq!(exported.frame_count, 3);
    assert_eq!(exported.frame_number, 2);
    let state = state.lock().unwrap();
    assert_eq!(state.frame_dimensions, vec![(5, 3); 3]);
    assert_eq!(state.fps, vec![24.0; 3]);
    assert_eq!(state.containers, vec!["mp4"; 3]);
    assert!(state.runtime_audio.iter().all(Option::is_none));
    assert_eq!(state.finishes, 1);
}

fn write_stereo_wave(path: &std::path::Path, frames: &[[f32; 2]]) {
    use std::io::Write as _;

    let channels = 2_u16;
    let bits_per_sample = 16_u16;
    let block_align = channels * (bits_per_sample / 8);
    let byte_rate = 48_000_u32 * u32::from(block_align);
    let data_len = u32::try_from(frames.len() * usize::from(block_align)).unwrap();
    let mut file = fs::File::create(path).unwrap();
    file.write_all(b"RIFF").unwrap();
    file.write_all(&(36_u32 + data_len).to_le_bytes()).unwrap();
    file.write_all(b"WAVEfmt ").unwrap();
    file.write_all(&16_u32.to_le_bytes()).unwrap();
    file.write_all(&1_u16.to_le_bytes()).unwrap();
    file.write_all(&channels.to_le_bytes()).unwrap();
    file.write_all(&48_000_u32.to_le_bytes()).unwrap();
    file.write_all(&byte_rate.to_le_bytes()).unwrap();
    file.write_all(&block_align.to_le_bytes()).unwrap();
    file.write_all(&bits_per_sample.to_le_bytes()).unwrap();
    file.write_all(b"data").unwrap();
    file.write_all(&data_len.to_le_bytes()).unwrap();
    for frame in frames {
        for sample in frame {
            let pcm = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16;
            file.write_all(&pcm.to_le_bytes()).unwrap();
        }
    }
    file.flush().unwrap();
}

fn add_audio_item(project: &mut AuthoringProject, path: &std::path::Path, source_frames: usize) {
    let mut asset = Asset::new("audio", path.to_str().unwrap(), AssetKind::Audio);
    asset.duration = Some(source_frames as f64 / 48_000.0);
    let asset_id = asset.id;
    project.assets.push(asset);
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    let item_id = TimelineItemId::new();
    project.items.insert(
        item_id,
        TimelineItem {
            id: item_id,
            track_id,
            name: "BGM".to_string(),
            source: SourceRef::Asset { asset_id },
            interval: TimelineInterval::new(
                MediaTime::zero(),
                MediaTime::new(source_frames as i64, 48_000).unwrap(),
            )
            .unwrap(),
            time_map: TimeMap::default(),
            layer: 0,
            parent: None,
            authored_properties: PropertyMap::new(),
        },
    );
}

#[test]
fn authoring_video_export_binds_exact_timeline_audio_then_cleans_it_up() {
    let directory = tempfile::tempdir().unwrap();
    let wave_path = directory.path().join("timeline.wav");
    let source = [[0.25, -0.25], [0.5, -0.5], [0.75, -0.75], [1.0, -1.0]];
    write_stereo_wave(&wave_path, &source);
    let mut project = AuthoringProject::new(
        "video with authoring audio",
        2,
        2,
        RationalRate::new(24, 1).unwrap(),
        MediaTime::new(source.len() as i64, 48_000).unwrap(),
    )
    .unwrap();
    add_audio_item(&mut project, &wave_path, source.len());
    project.validate().unwrap();
    let timeline_id = project.root_timeline_id;
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let state = Arc::new(Mutex::new(MockVideoState::default()));
    let plugins = Arc::new(PluginManager::new());
    plugins.register_export_plugin(Arc::new(MockVideoExporter {
        state: Arc::clone(&state),
    }));
    let server = RenderServer::new(plugins, Arc::new(CacheManager::new()));
    let output_path = directory
        .path()
        .join("with-audio.mp4")
        .to_string_lossy()
        .into_owned();

    assert!(server.send_authoring_video_export_request(
        RenderRequestId::new(49),
        Arc::new(project),
        Arc::new(plan),
        timeline_id,
        output_path,
    ));
    let exported = server
        .rx_authoring_export_result
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    exported.output.unwrap();
    assert_eq!(exported.frames_exported, 1);

    let state = state.lock().unwrap();
    assert_eq!(state.runtime_audio.len(), 1);
    let captured = state.runtime_audio[0].as_ref().unwrap();
    assert_eq!(captured.channels, 2);
    assert_eq!(captured.sample_rate, 48_000);
    assert_eq!(captured.bytes.len(), source.len() * 2 * size_of::<f32>());
    let decoded = captured
        .bytes
        .chunks_exact(size_of::<f32>())
        .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
        .collect::<Vec<_>>();
    for (actual, expected) in decoded.into_iter().zip(source.into_iter().flatten()) {
        assert!((actual - expected).abs() < 0.0002, "{actual} != {expected}");
    }
    assert!(
        !captured.path.exists(),
        "temporary audio must outlive finalize but be removed before completion"
    );
    assert_eq!(state.finishes, 1);
}

#[test]
fn authoring_audio_failure_stops_before_video_frame_zero() {
    let mut project = AuthoringProject::new(
        "broken authoring audio",
        2,
        2,
        RationalRate::new(24, 1).unwrap(),
        MediaTime::new(1, 24).unwrap(),
    )
    .unwrap();
    let missing = std::env::temp_dir().join(format!("missing-{}.wav", Uuid::new_v4()));
    add_audio_item(&mut project, &missing, 2_000);
    project.validate().unwrap();
    let timeline_id = project.root_timeline_id;
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let state = Arc::new(Mutex::new(MockVideoState::default()));
    let plugins = Arc::new(PluginManager::new());
    plugins.register_export_plugin(Arc::new(MockVideoExporter {
        state: Arc::clone(&state),
    }));
    let server = RenderServer::new(plugins, Arc::new(CacheManager::new()));
    let output_path = std::env::temp_dir()
        .join(format!("broken-audio-{}.mp4", Uuid::new_v4()))
        .to_string_lossy()
        .into_owned();

    assert!(server.send_authoring_video_export_request(
        RenderRequestId::new(50),
        Arc::new(project),
        Arc::new(plan),
        timeline_id,
        output_path,
    ));
    let exported = server
        .rx_authoring_export_result
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    let error = exported.output.unwrap_err();
    assert!(error.to_string().contains("authoring audio render failed"));
    assert_eq!(exported.frames_exported, 0);
    let state = state.lock().unwrap();
    assert!(state.frame_dimensions.is_empty());
    assert!(state.runtime_audio.is_empty());
    assert_eq!(state.finishes, 0);
}

#[test]
fn authoring_video_export_never_opens_an_asset_as_its_destination() {
    let protected = TemporaryPng::new();
    fs::write(&protected.0, b"video source bytes must survive").unwrap();
    let protected_path = protected.0.with_extension("mp4");
    fs::rename(&protected.0, &protected_path).unwrap();
    let protected_path = protected_path.to_string_lossy().into_owned();
    let mut project = AuthoringProject::new(
        "safe authoring video",
        2,
        2,
        RationalRate::new(24, 1).unwrap(),
        MediaTime::new(1, 24).unwrap(),
    )
    .unwrap();
    project.assets.push(Asset::new(
        "protected video input",
        &protected_path,
        AssetKind::Video,
    ));
    let timeline_id = project.root_timeline_id;
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let state = Arc::new(Mutex::new(MockVideoState::default()));
    let plugins = Arc::new(PluginManager::new());
    plugins.register_export_plugin(Arc::new(MockVideoExporter {
        state: Arc::clone(&state),
    }));
    let server = RenderServer::new(plugins, Arc::new(CacheManager::new()));

    assert!(server.send_authoring_video_export_request(
        RenderRequestId::new(48),
        Arc::new(project),
        Arc::new(plan),
        timeline_id,
        protected_path.clone(),
    ));
    let exported = server
        .rx_authoring_export_result
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    let error = exported.output.unwrap_err();

    assert!(error.to_string().contains("refusing to overwrite"));
    assert_eq!(exported.frames_exported, 0);
    assert_eq!(
        fs::read(&protected_path).unwrap(),
        b"video source bytes must survive"
    );
    let state = state.lock().unwrap();
    assert!(state.frame_dimensions.is_empty());
    assert_eq!(state.finishes, 0);
    drop(state);
    fs::remove_file(protected_path).unwrap();
}
