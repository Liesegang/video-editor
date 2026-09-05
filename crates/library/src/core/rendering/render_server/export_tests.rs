use super::export::{authoring_video_frame_count, preflight_authoring_video_requires_gpu};
use super::{RenderRequestId, RenderServer};
use crate::cache::CacheManager;
use crate::core::render_plan::{RenderCapability, RenderPlanCompiler};
use crate::editor::{ParticleNodeClipPlacement, TimelineEditorService};
use crate::error::LibraryError;
use crate::model::authoring::{
    AuthoringProject, CompositionInstance, DurationPolicy, InstanceLocator, ItemOutputStage,
    MediaInputBinding, MediaOutputKind, MediaTime, ModuleDefinition, ModuleDefinitionSharing,
    ModuleInstance, ModuleInstanceId, ModuleInvocation, ModulePortAddress, PublishedMediaInput,
    PublishedMediaInputId, RationalRate, SourceRef, TimeMap, TimelineInterval, TimelineItem,
    TimelineItemId,
};
use crate::model::frame::color::Color;
use crate::model::project::asset::{Asset, AssetKind};
use crate::model::project::property::PropertyMap;
use crate::model::project::{IMAGE_INPUT_PORT, PortDataType};
use crate::plugin::{
    ExportDestination, ExportFrame, ExportPlugin, ExportSettings, Plugin, PluginManager,
};
#[cfg(all(feature = "gl", target_os = "windows"))]
use crate::rendering::renderer::RenderOutput;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

#[path = "export_capability_tests.rs"]
mod capability_tests;

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

fn particle_export_project() -> Arc<AuthoringProject> {
    let mut project = AuthoringProject::new(
        "Particle export parity",
        256,
        144,
        RationalRate::new(30, 1).unwrap(),
        MediaTime::new(5, 1).unwrap(),
    )
    .unwrap();
    let timeline_id = project.root_timeline_id;
    project
        .timelines
        .get_mut(&timeline_id)
        .unwrap()
        .background_color = Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
    let service = TimelineEditorService::new(project).unwrap();
    let project = service.snapshot().unwrap();
    let track_id = project.timelines[&timeline_id].track_order[0];
    drop(project);
    service
        .create_particle_node_clip(ParticleNodeClipPlacement {
            track_id,
            name: "GPU Particles".to_string(),
            interval: TimelineInterval::new(MediaTime::zero(), MediaTime::new(5, 1).unwrap())
                .unwrap(),
            layer: 0,
        })
        .unwrap();
    service.snapshot().unwrap()
}

#[cfg(not(all(feature = "gl", target_os = "windows")))]
fn assert_explicit_particle_gpu_diagnostic(error: &LibraryError) {
    let diagnostic = error.to_string();
    assert!(
        diagnostic.contains("GPU Particle") && diagnostic.contains("OpenGL"),
        "expected explicit GPU Particle/OpenGL diagnosis, got: {diagnostic}"
    );
}

#[test]
fn export_gpu_requirement_comes_from_the_requested_render_plan() {
    let ordinary = AuthoringProject::new(
        "CPU export",
        64,
        36,
        RationalRate::new(24, 1).unwrap(),
        MediaTime::new(1, 1).unwrap(),
    )
    .unwrap();
    let ordinary_plan = RenderPlanCompiler::compile(&ordinary).unwrap();
    assert!(
        !ordinary_plan
            .timeline_may_require_capability(
                &ordinary,
                ordinary.root_timeline_id,
                None,
                RenderCapability::Gpu,
            )
            .unwrap(),
        "ordinary Timeline export must keep the CPU renderer"
    );

    let particle = particle_export_project();
    let particle_plan = RenderPlanCompiler::compile(particle.as_ref()).unwrap();
    assert!(
        particle_plan
            .timeline_may_require_capability(
                particle.as_ref(),
                particle.root_timeline_id,
                None,
                RenderCapability::Gpu,
            )
            .unwrap(),
        "the selected Output reaches a compiled Particle renderer"
    );
}

#[test]
fn export_gpu_requirement_ignores_nested_placement_outside_export_range() {
    let project = AuthoringProject::new(
        "Nested export range",
        64,
        36,
        RationalRate::new(24, 1).unwrap(),
        MediaTime::new(5, 1).unwrap(),
    )
    .unwrap();
    let timeline_id = project.root_timeline_id;
    let root_track_id = project.timelines[&timeline_id].track_order[0];
    let service = TimelineEditorService::new(project).unwrap();
    let (nested_timeline_id, nested_track_id, _) = service
        .add_timeline(
            "Nested Particle".to_string(),
            64,
            36,
            RationalRate::new(24, 1).unwrap(),
            MediaTime::new(5, 1).unwrap(),
        )
        .unwrap();
    service
        .create_particle_node_clip(ParticleNodeClipPlacement {
            track_id: nested_track_id,
            name: "Nested GPU Particles".to_string(),
            interval: TimelineInterval::new(MediaTime::zero(), MediaTime::new(5, 1).unwrap())
                .unwrap(),
            layer: 0,
        })
        .unwrap();
    service
        .add_item(
            root_track_id,
            "Inactive nested placement".to_string(),
            SourceRef::Composition(CompositionInstance {
                timeline_id: nested_timeline_id,
                duration_policy: DurationPolicy::Fixed,
                parameter_overrides: HashMap::new(),
                transition_module_overrides: Vec::new(),
            }),
            TimelineInterval::new(MediaTime::new(6, 1).unwrap(), MediaTime::new(1, 1).unwrap())
                .unwrap(),
            0,
        )
        .unwrap();
    let project = service.snapshot().unwrap();
    let plan = RenderPlanCompiler::compile(project.as_ref()).unwrap();

    assert!(
        !plan
            .timeline_may_require_capability(
                project.as_ref(),
                timeline_id,
                None,
                RenderCapability::Gpu,
            )
            .unwrap(),
        "a nested Particle placement outside the exported Timeline range is unreachable"
    );
}

#[test]
fn export_gpu_preflight_resolves_inactive_nested_time_range_without_false_positive() {
    let project = AuthoringProject::new(
        "Nested fixed range",
        64,
        36,
        RationalRate::new(24, 1).unwrap(),
        MediaTime::new(2, 1).unwrap(),
    )
    .unwrap();
    let timeline_id = project.root_timeline_id;
    let root_track_id = project.timelines[&timeline_id].track_order[0];
    let service = TimelineEditorService::new(project).unwrap();
    let (nested_timeline_id, nested_track_id, _) = service
        .add_timeline(
            "Nested fixed Particle".to_string(),
            64,
            36,
            RationalRate::new(24, 1).unwrap(),
            MediaTime::new(5, 1).unwrap(),
        )
        .unwrap();
    service
        .create_particle_node_clip(ParticleNodeClipPlacement {
            track_id: nested_track_id,
            name: "Late nested Particles".to_string(),
            interval: TimelineInterval::new(
                MediaTime::new(4, 1).unwrap(),
                MediaTime::new(1, 1).unwrap(),
            )
            .unwrap(),
            layer: 0,
        })
        .unwrap();
    service
        .add_item(
            root_track_id,
            "One-second fixed placement".to_string(),
            SourceRef::Composition(CompositionInstance {
                timeline_id: nested_timeline_id,
                duration_policy: DurationPolicy::Fixed,
                parameter_overrides: HashMap::new(),
                transition_module_overrides: Vec::new(),
            }),
            TimelineInterval::new(MediaTime::zero(), MediaTime::new(1, 1).unwrap()).unwrap(),
            0,
        )
        .unwrap();
    let project = service.snapshot().unwrap();
    let plan = RenderPlanCompiler::compile(project.as_ref()).unwrap();
    assert!(
        plan.timeline_may_require_capability(
            project.as_ref(),
            timeline_id,
            None,
            RenderCapability::Gpu,
        )
        .unwrap(),
        "the cheap hierarchical query may conservatively see the nested definition"
    );
    let frame_count = authoring_video_frame_count(project.as_ref(), timeline_id).unwrap();

    assert!(
        !preflight_authoring_video_requires_gpu(
            project.as_ref(),
            &plan,
            &PluginManager::default(),
            timeline_id,
            None,
            frame_count,
        )
        .unwrap(),
        "production frame evaluation must resolve the Fixed placement's actual reachable range"
    );
}

#[cfg(all(feature = "gl", target_os = "windows"))]
fn read_rgba8_png(path: &std::path::Path) -> Vec<u8> {
    let decoder = png::Decoder::new(std::io::BufReader::new(fs::File::open(path).unwrap()));
    let mut reader = decoder.read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
    let output = reader.next_frame(&mut pixels).unwrap();
    assert_eq!(output.color_type, png::ColorType::Rgba);
    assert_eq!(output.bit_depth, png::BitDepth::Eight);
    pixels.truncate(output.buffer_size());
    pixels
}

/// Exercises the production Preview and PNG export workers with independent
/// real GL contexts and compares their terminal straight-RGBA8 pixels.
#[cfg(all(feature = "gl", target_os = "windows"))]
#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn authoring_particle_png_export_matches_preview_and_is_nontransparent() {
    let project = particle_export_project();
    let timeline_id = project.root_timeline_id;
    let plan = Arc::new(RenderPlanCompiler::compile(project.as_ref()).unwrap());
    let output = TemporaryPng::new();
    let output_path = output.0.to_string_lossy().into_owned();
    let server = RenderServer::new(
        Arc::new(PluginManager::default()),
        Arc::new(CacheManager::new()),
    );
    let frame_number = 60;

    assert!(server.send_authoring_request(
        RenderRequestId::new(80),
        Arc::clone(&project),
        Arc::clone(&plan),
        timeline_id,
        frame_number,
        1.0,
        None,
    ));
    assert!(server.send_authoring_png_export_request(
        RenderRequestId::new(81),
        project,
        plan,
        timeline_id,
        frame_number,
        output_path,
    ));

    let exported = server
        .rx_authoring_export_result
        .recv_timeout(Duration::from_secs(30))
        .unwrap();
    let preview = server
        .rx_authoring_result
        .recv_timeout(Duration::from_secs(30))
        .unwrap();
    exported
        .output
        .unwrap_or_else(|error| panic!("Particle export preflight/render failed: {error}"));
    assert_eq!(exported.frames_exported, 1);
    let preview_image = match preview.output {
        Ok(RenderOutput::Image(image)) => image,
        Ok(other) => panic!("expected terminal Preview image, got {other:?}"),
        Err(error) => panic!("Particle Preview failed while export succeeded: {error}"),
    };
    let exported_pixels = read_rgba8_png(&output.0);
    assert_eq!(exported_pixels, preview_image.data);
    assert!(
        exported_pixels.chunks_exact(4).any(|pixel| pixel[3] != 0),
        "Particle PNG must contain at least one nontransparent pixel"
    );
}

#[cfg(not(all(feature = "gl", target_os = "windows")))]
#[test]
fn authoring_particle_png_export_reports_unsupported_gpu_without_writing() {
    let project = particle_export_project();
    let timeline_id = project.root_timeline_id;
    let plan = Arc::new(RenderPlanCompiler::compile(project.as_ref()).unwrap());
    let output = TemporaryPng::new();
    let server = RenderServer::new_with_cpu_preview(
        Arc::new(PluginManager::default()),
        Arc::new(CacheManager::new()),
    );
    assert!(server.send_authoring_png_export_request(
        RenderRequestId::new(82),
        project,
        plan,
        timeline_id,
        60,
        output.0.to_string_lossy().into_owned(),
    ));
    let exported = server
        .rx_authoring_export_result
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    let error = exported.output.unwrap_err();
    assert_explicit_particle_gpu_diagnostic(&error);
    assert_eq!(exported.frames_exported, 0);
    assert!(!output.0.exists());
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
        destination: &ExportDestination,
        frame: &ExportFrame,
        settings: &ExportSettings,
    ) -> Result<(), LibraryError> {
        settings.require_matching_color_authority(frame)?;
        fs::OpenOptions::new()
            .append(true)
            .open(destination.writable_path())?
            .write_all(b"frame")?;
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

    fn finish_export(
        &self,
        _destination: &ExportDestination,
        _settings: &ExportSettings,
    ) -> Result<(), LibraryError> {
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
    let server = RenderServer::new_with_cpu_preview(
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
    let server = RenderServer::new_with_cpu_preview(
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
    let server = RenderServer::new_with_cpu_preview(plugins, Arc::new(CacheManager::new()));
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

#[cfg(not(all(feature = "gl", target_os = "windows")))]
#[test]
fn late_particle_video_fails_gpu_preflight_before_frame_zero() {
    let mut project = particle_export_project().as_ref().clone();
    let item = project
        .items
        .values_mut()
        .find(|item| matches!(&item.source, SourceRef::Module(_)))
        .expect("Particle Node Clip");
    item.interval =
        TimelineInterval::new(MediaTime::new(4, 1).unwrap(), MediaTime::new(1, 1).unwrap())
            .unwrap();
    project.validate().unwrap();
    let timeline_id = project.root_timeline_id;
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let state = Arc::new(Mutex::new(MockVideoState::default()));
    let plugins = Arc::new(PluginManager::default());
    plugins.register_export_plugin(Arc::new(MockVideoExporter {
        state: Arc::clone(&state),
    }));
    let server = RenderServer::new_with_cpu_preview(plugins, Arc::new(CacheManager::new()));
    let output_path = std::env::temp_dir()
        .join(format!("late-particle-{}.mp4", Uuid::new_v4()))
        .to_string_lossy()
        .into_owned();

    assert!(server.send_authoring_video_export_request(
        RenderRequestId::new(83),
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
    assert_explicit_particle_gpu_diagnostic(&error);
    assert_eq!(exported.frames_exported, 0);
    let state = state.lock().unwrap();
    assert!(state.frame_dimensions.is_empty());
    assert!(state.runtime_audio.is_empty());
    assert_eq!(state.finishes, 0);
}

pub(super) fn write_stereo_wave(path: &std::path::Path, frames: &[[f32; 2]]) {
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

pub(super) fn add_audio_item(
    project: &mut AuthoringProject,
    path: &std::path::Path,
    source_frames: usize,
) {
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
            blend_mode: crate::model::BlendMode::Normal,
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
    let server = RenderServer::new_with_cpu_preview(plugins, Arc::new(CacheManager::new()));
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
    let server = RenderServer::new_with_cpu_preview(plugins, Arc::new(CacheManager::new()));
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
    let server = RenderServer::new_with_cpu_preview(plugins, Arc::new(CacheManager::new()));

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
