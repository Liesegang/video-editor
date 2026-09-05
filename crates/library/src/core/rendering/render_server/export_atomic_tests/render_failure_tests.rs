use super::*;

use crate::editor::TimelineEditorService;
use crate::model::AssetKind;
use crate::model::authoring::{AttachmentOwner, AttachmentStage, SourceRef, TimelineInterval};
use crate::model::frame::color::Color;
use crate::model::property::PropertyValue;

const SENTINEL: &[u8] = b"keep this existing video";
const VIDEO_LOADER_ID: &str = "ffmpeg_video_loader";

struct ProductionRenderFixture {
    server: RenderServer,
    project: Arc<AuthoringProject>,
    plan: Arc<RenderPlan>,
    probe: Arc<Mutex<ExportProbe>>,
    wave_path: Option<PathBuf>,
}

fn base_project_with_audio(directory: Option<&Path>) -> (AuthoringProject, Option<PathBuf>) {
    let mut project = base_export_project();
    let wave_path = directory.map(|directory| directory.join("source.wav"));
    if let Some(wave_path) = &wave_path {
        let source = vec![[0.25_f32, -0.25_f32]; 4_000];
        write_stereo_wave(wave_path, &source);
        add_audio_item(&mut project, wave_path, source.len());
    }
    (project, wave_path)
}

fn blur_fixture(directory: Option<&Path>) -> (ProductionRenderFixture, Arc<PluginManager>) {
    let plugins = Arc::new(PluginManager::default());
    let (project, wave_path) = base_project_with_audio(directory);
    let service = TimelineEditorService::new(project).unwrap();
    let snapshot = service.snapshot().unwrap();
    let track_id = snapshot.timelines[&snapshot.root_timeline_id].track_order[0];
    drop(snapshot);
    let (item_id, _) = service
        .add_item(
            track_id,
            "Production blur host".to_string(),
            SourceRef::Solid {
                color: Color::white(),
            },
            TimelineInterval::new(MediaTime::zero(), MediaTime::new(2, 24).unwrap()).unwrap(),
            1,
        )
        .unwrap();
    let (attachment_id, _) = service
        .add_builtin_effect_by_id(
            &plugins,
            AttachmentOwner::Item { item_id },
            AttachmentStage::ItemPostTransform,
            "blur",
        )
        .unwrap();
    for parameter in ["sigma_x", "sigma_y"] {
        service
            .set_builtin_effect_parameter_constant(
                attachment_id,
                parameter,
                PropertyValue::from(1.0),
            )
            .unwrap();
    }
    let project = service.snapshot().unwrap();
    project.validate().unwrap();
    let (server, project, plan, probe) =
        export_server_for_project_with_plugins(ExportFault::None, project, Arc::clone(&plugins));
    (
        ProductionRenderFixture {
            server,
            project,
            plan,
            probe,
            wave_path,
        },
        plugins,
    )
}

fn video_fixture(
    directory: Option<&Path>,
) -> (ProductionRenderFixture, Arc<PluginManager>, PathBuf) {
    let plugins = Arc::new(PluginManager::default());
    let (project, wave_path) = base_project_with_audio(directory);
    let service = TimelineEditorService::new(project).unwrap();
    let media_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("test_data/e2e_media/h264_24.mp4")
        .canonicalize()
        .unwrap();
    let (asset_ids, _) = service.import_file(&media_path, &plugins).unwrap();
    let snapshot = service.snapshot().unwrap();
    let asset = snapshot
        .assets
        .iter()
        .find(|asset| asset_ids.contains(&asset.id) && asset.kind == AssetKind::Video)
        .expect("the production FFmpeg probe must import a Video Asset");
    let asset_id = asset.id;
    let loaded_path = PathBuf::from(&asset.path);
    let track_id = snapshot.timelines[&snapshot.root_timeline_id].track_order[0];
    drop(snapshot);
    service
        .add_item(
            track_id,
            "Production FFmpeg video".to_string(),
            SourceRef::Asset { asset_id },
            TimelineInterval::new(MediaTime::zero(), MediaTime::new(2, 24).unwrap()).unwrap(),
            1,
        )
        .unwrap();
    let project = service.snapshot().unwrap();
    project.validate().unwrap();
    let (server, project, plan, probe) =
        export_server_for_project_with_plugins(ExportFault::None, project, Arc::clone(&plugins));
    (
        ProductionRenderFixture {
            server,
            project,
            plan,
            probe,
            wave_path,
        },
        plugins,
        loaded_path,
    )
}

fn assert_failure_then_recovery(
    fixture: ProductionRenderFixture,
    directory: &Path,
    request_id: u64,
    expected_frames_before_failure: u64,
    expected_error: &str,
) {
    let final_path = directory.join("movie.mp4");
    fs::write(&final_path, SENTINEL).unwrap();
    let expected_siblings = fixture.wave_path.iter().cloned().collect::<Vec<_>>();
    let expected_finishes = usize::from(expected_frames_before_failure > 0);

    let failed = request_export(
        &fixture.server,
        &fixture.project,
        &fixture.plan,
        request_id,
        &final_path,
    );
    let error = failed.output.unwrap_err().to_string();
    assert!(error.contains(expected_error), "{error}");
    assert_eq!(failed.request_id, RenderRequestId::new(request_id));
    assert_eq!(failed.frame_count, 2);
    assert_eq!(failed.frames_exported, expected_frames_before_failure);
    assert!(!failed.published);
    assert_eq!(fs::read(&final_path).unwrap(), SENTINEL);
    assert_eq!(
        sibling_paths(directory, &final_path),
        expected_siblings,
        "the failed export must remove its sibling staging artifact"
    );
    {
        let probe = fixture.probe.lock().unwrap();
        assert_eq!(probe.frames, expected_frames_before_failure as usize);
        assert_eq!(probe.finishes, expected_finishes);
    }
    assert_no_additional_completion(&fixture.server);
    if fixture.wave_path.is_some() {
        assert_temporary_audio_cleaned(&fixture.server, 1, 1, 0, 0);
    }
    if fixture.wave_path.is_some() && expected_frames_before_failure > 0 {
        assert_runtime_audio_cleaned(&fixture.probe);
    }

    let recovered = request_export(
        &fixture.server,
        &fixture.project,
        &fixture.plan,
        request_id + 1,
        &final_path,
    );
    recovered.output.unwrap();
    assert_eq!(recovered.request_id, RenderRequestId::new(request_id + 1));
    assert_eq!(recovered.frame_count, 2);
    assert_eq!(recovered.frames_exported, 2);
    assert!(recovered.published);
    let first_recovery_frame = expected_frames_before_failure;
    let expected_artifact = format!(
        "frame:{first_recovery_frame}\nframe:{}\nfinished\n",
        first_recovery_frame + 1
    );
    assert_eq!(fs::read(&final_path).unwrap(), expected_artifact.as_bytes());
    assert_eq!(sibling_paths(directory, &final_path), expected_siblings);
    {
        let probe = fixture.probe.lock().unwrap();
        assert_eq!(probe.frames, expected_frames_before_failure as usize + 2);
        assert_eq!(probe.finishes, expected_finishes + 1);
    }
    assert_no_additional_completion(&fixture.server);
    if fixture.wave_path.is_some() {
        assert_temporary_audio_cleaned(&fixture.server, 2, 2, 0, 0);
        assert_runtime_audio_cleaned(&fixture.probe);
    }
}

#[test]
fn production_blur_frame_zero_failure_is_atomic_and_the_same_server_recovers() {
    let directory = tempfile::tempdir().unwrap();
    let (fixture, plugins) = blur_fixture(Some(directory.path()));
    plugins
        .fail_effect_once_after_success("blur", MediaTime::zero().to_seconds_f64())
        .unwrap();
    assert_failure_then_recovery(
        fixture,
        directory.path(),
        9_300,
        0,
        "injected Effect failure after 'blur' succeeded",
    );
}

#[test]
fn production_blur_frame_one_failure_finishes_audio_and_the_same_server_recovers() {
    let directory = tempfile::tempdir().unwrap();
    let (fixture, plugins) = blur_fixture(Some(directory.path()));
    plugins
        .fail_effect_once_after_success("blur", MediaTime::new(1, 24).unwrap().to_seconds_f64())
        .unwrap();
    assert_failure_then_recovery(
        fixture,
        directory.path(),
        9_310,
        1,
        "injected Effect failure after 'blur' succeeded",
    );
}

#[test]
fn production_video_loader_frame_zero_failure_is_atomic_and_the_same_server_recovers() {
    let directory = tempfile::tempdir().unwrap();
    let (fixture, plugins, video_path) = video_fixture(None);
    plugins
        .fail_video_loader_once_after_success(VIDEO_LOADER_ID, &video_path, 0.0)
        .unwrap();
    assert_failure_then_recovery(
        fixture,
        directory.path(),
        9_320,
        0,
        "injected Asset Loader failure after 'ffmpeg_video_loader' succeeded",
    );
}

#[test]
fn production_video_loader_frame_one_failure_finishes_audio_and_the_same_server_recovers() {
    let directory = tempfile::tempdir().unwrap();
    let (fixture, plugins, video_path) = video_fixture(Some(directory.path()));
    plugins
        .fail_video_loader_once_after_success(
            VIDEO_LOADER_ID,
            &video_path,
            MediaTime::new(1, 24).unwrap().to_seconds_f64(),
        )
        .unwrap();
    assert_failure_then_recovery(
        fixture,
        directory.path(),
        9_330,
        1,
        "injected Asset Loader failure after 'ffmpeg_video_loader' succeeded",
    );
}

#[test]
fn production_video_loader_panic_is_terminal_and_the_worker_recovers() {
    let directory = tempfile::tempdir().unwrap();
    let (fixture, plugins, video_path) = video_fixture(Some(directory.path()));
    plugins
        .panic_video_loader_once_after_success(
            VIDEO_LOADER_ID,
            &video_path,
            MediaTime::new(1, 24).unwrap().to_seconds_f64(),
        )
        .unwrap();
    assert_failure_then_recovery(
        fixture,
        directory.path(),
        9_340,
        1,
        "panicked: injected Asset Loader failure after 'ffmpeg_video_loader' succeeded",
    );
}
