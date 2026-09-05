use super::export_tests::{add_audio_item, write_stereo_wave};
use super::{RenderRequestId, RenderServer};
use crate::cache::CacheManager;
use crate::core::render_plan::{RenderPlan, RenderPlanCompiler};
use crate::error::LibraryError;
use crate::model::authoring::{AuthoringProject, MediaTime, RationalRate};
use crate::plugin::{
    ExportDestination, ExportFrame, ExportPlugin, ExportSettings, Plugin, PluginManager,
};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::TryRecvError;
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

#[path = "export_atomic_tests/audio_cleanup_tests.rs"]
mod audio_cleanup_tests;
#[path = "export_atomic_tests/cancellation_tests.rs"]
mod cancellation_tests;
#[path = "export_atomic_tests/publication_failure_tests.rs"]
mod publication_failure_tests;
#[path = "export_atomic_tests/render_failure_tests.rs"]
mod render_failure_tests;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ExportFault {
    #[default]
    None,
    Frame(usize),
    Finish,
    DeleteBeforePublish,
    PanicFrameOnce(usize),
    PanicFinishOnce,
}

#[derive(Debug, Default)]
struct ExportProbe {
    logical_paths: Vec<String>,
    writable_paths: Vec<String>,
    frames: usize,
    finishes: usize,
    frame_panics: usize,
    finish_panics: usize,
    runtime_audio_paths: Vec<String>,
}

struct StagingProbeExporter {
    fault: ExportFault,
    probe: Arc<Mutex<ExportProbe>>,
    replacement: Option<ReplacementRegistration>,
}

struct ReplacementRegistration {
    manager: Weak<PluginManager>,
    calls: Arc<AtomicUsize>,
}

struct RejectingReplacementExporter {
    calls: Arc<AtomicUsize>,
}

impl Plugin for StagingProbeExporter {
    fn id(&self) -> &str {
        "ffmpeg_export"
    }

    fn name(&self) -> String {
        "Atomic staging probe".to_string()
    }

    fn category(&self) -> String {
        "Export".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 0, 1)
    }
}

impl ExportPlugin for StagingProbeExporter {
    fn export_frame(
        &self,
        destination: &ExportDestination,
        frame: &ExportFrame,
        settings: &ExportSettings,
    ) -> Result<(), LibraryError> {
        settings.require_matching_color_authority(frame)?;
        let (frame_index, should_panic) = {
            let mut probe = self.probe.lock().map_err(|_| {
                LibraryError::Runtime("atomic export probe lock poisoned".to_string())
            })?;
            let frame_index = probe.frames;
            probe.frames += 1;
            probe
                .logical_paths
                .push(destination.logical_path().to_string());
            probe
                .writable_paths
                .push(destination.writable_path().to_string());
            if let Some((path, _, _)) = settings.runtime_audio_source()
                && probe.runtime_audio_paths.last().map(String::as_str) != Some(path)
            {
                probe.runtime_audio_paths.push(path.to_string());
            }
            let should_panic =
                self.fault == ExportFault::PanicFrameOnce(frame_index) && probe.frame_panics == 0;
            if should_panic {
                probe.frame_panics += 1;
            }
            (frame_index, should_panic)
        };

        let mut options = OpenOptions::new();
        options.write(true);
        if frame_index == 0 {
            options.truncate(true);
        } else {
            options.append(true);
        }
        let mut output = options.open(destination.writable_path())?;
        writeln!(output, "frame:{frame_index}")?;
        output.flush()?;
        if frame_index == 0
            && let Some(replacement) = &self.replacement
        {
            let manager = replacement.manager.upgrade().ok_or_else(|| {
                LibraryError::Runtime("atomic export PluginManager was dropped".to_string())
            })?;
            manager.register_export_plugin(Arc::new(RejectingReplacementExporter {
                calls: Arc::clone(&replacement.calls),
            }));
        }
        if should_panic {
            std::panic::panic_any(format!("injected exporter frame {frame_index} panic"));
        }
        if self.fault == ExportFault::Frame(frame_index) {
            return Err(LibraryError::Plugin(format!(
                "injected frame {frame_index} failure"
            )));
        }
        Ok(())
    }

    fn finish_export(
        &self,
        destination: &ExportDestination,
        _settings: &ExportSettings,
    ) -> Result<(), LibraryError> {
        let should_panic = {
            let mut probe = self.probe.lock().map_err(|_| {
                LibraryError::Runtime("atomic export probe lock poisoned".to_string())
            })?;
            probe.finishes += 1;
            let should_panic =
                self.fault == ExportFault::PanicFinishOnce && probe.finish_panics == 0;
            if should_panic {
                probe.finish_panics += 1;
            }
            should_panic
        };
        if should_panic {
            std::panic::panic_any("injected exporter finish panic".to_string());
        }
        if self.fault == ExportFault::DeleteBeforePublish {
            fs::remove_file(destination.writable_path())?;
            return Ok(());
        }
        let mut output = OpenOptions::new()
            .append(true)
            .open(destination.writable_path())?;
        writeln!(output, "finished")?;
        output.flush()?;
        if self.fault == ExportFault::Finish {
            return Err(LibraryError::Plugin(
                "injected exporter finish failure".to_string(),
            ));
        }
        Ok(())
    }
}

impl Plugin for RejectingReplacementExporter {
    fn id(&self) -> &str {
        "ffmpeg_export"
    }

    fn name(&self) -> String {
        "Rejecting replacement exporter".to_string()
    }

    fn category(&self) -> String {
        "Export".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 0, 1)
    }
}

impl ExportPlugin for RejectingReplacementExporter {
    fn export_frame(
        &self,
        _destination: &ExportDestination,
        _frame: &ExportFrame,
        _settings: &ExportSettings,
    ) -> Result<(), LibraryError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(LibraryError::Plugin(
            "replacement exporter must not enter an active job".to_string(),
        ))
    }

    fn finish_export(
        &self,
        _destination: &ExportDestination,
        _settings: &ExportSettings,
    ) -> Result<(), LibraryError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Err(LibraryError::Plugin(
            "replacement exporter must not finalize an active job".to_string(),
        ))
    }
}

fn base_export_project() -> AuthoringProject {
    AuthoringProject::new(
        "atomic video export",
        4,
        2,
        RationalRate::new(24, 1).unwrap(),
        MediaTime::new(2, 24).unwrap(),
    )
    .unwrap()
}

fn export_project() -> Arc<AuthoringProject> {
    Arc::new(base_export_project())
}

fn export_project_with_audio(directory: &Path) -> (Arc<AuthoringProject>, PathBuf) {
    let wave_path = directory.join("source.wav");
    let source = [[0.25, -0.25], [0.5, -0.5], [0.75, -0.75], [1.0, -1.0]];
    write_stereo_wave(&wave_path, &source);
    let mut project = base_export_project();
    add_audio_item(&mut project, &wave_path, source.len());
    project.validate().unwrap();
    (Arc::new(project), wave_path)
}

fn run_export(
    output_path: &Path,
    fault: ExportFault,
) -> (super::AuthoringExportResult, Arc<Mutex<ExportProbe>>) {
    let (server, project, plan, probe) = export_server(fault);
    let result = request_export(&server, &project, &plan, 9_000, output_path);
    (result, probe)
}

fn export_server(
    fault: ExportFault,
) -> (
    RenderServer,
    Arc<AuthoringProject>,
    Arc<RenderPlan>,
    Arc<Mutex<ExportProbe>>,
) {
    export_server_for_project(fault, export_project())
}

fn export_server_for_project(
    fault: ExportFault,
    project: Arc<AuthoringProject>,
) -> (
    RenderServer,
    Arc<AuthoringProject>,
    Arc<RenderPlan>,
    Arc<Mutex<ExportProbe>>,
) {
    export_server_for_project_with_plugins(fault, project, Arc::new(PluginManager::new()))
}

fn export_server_for_project_with_plugins(
    fault: ExportFault,
    project: Arc<AuthoringProject>,
    plugins: Arc<PluginManager>,
) -> (
    RenderServer,
    Arc<AuthoringProject>,
    Arc<RenderPlan>,
    Arc<Mutex<ExportProbe>>,
) {
    let plan = Arc::new(RenderPlanCompiler::compile(project.as_ref()).unwrap());
    let probe = Arc::new(Mutex::new(ExportProbe::default()));
    plugins.register_export_plugin(Arc::new(StagingProbeExporter {
        fault,
        probe: Arc::clone(&probe),
        replacement: None,
    }));
    let server = RenderServer::new_with_cpu_preview(plugins, Arc::new(CacheManager::new()));
    (server, project, plan, probe)
}

fn request_export(
    server: &RenderServer,
    project: &Arc<AuthoringProject>,
    plan: &Arc<RenderPlan>,
    request_id: u64,
    output_path: &Path,
) -> super::AuthoringExportResult {
    assert!(server.send_authoring_video_export_request(
        RenderRequestId::new(request_id),
        Arc::clone(project),
        Arc::clone(plan),
        project.root_timeline_id,
        output_path.to_string_lossy().into_owned(),
    ));
    server
        .rx_authoring_export_result
        .recv_timeout(Duration::from_secs(10))
        .unwrap()
}

fn assert_no_additional_completion(server: &RenderServer) {
    assert!(matches!(
        server.rx_authoring_export_result.try_recv(),
        Err(TryRecvError::Empty)
    ));
}

fn assert_temporary_audio_cleaned(
    server: &RenderServer,
    expected_created: usize,
    expected_explicit_attempts: usize,
    expected_drop_attempts: usize,
    expected_injected_failures: usize,
) {
    let (paths, explicit_attempts, drop_attempts, injected_failures) =
        server.temporary_audio_test_observation();
    assert_eq!(paths.len(), expected_created);
    assert_eq!(explicit_attempts, expected_explicit_attempts);
    assert_eq!(drop_attempts, expected_drop_attempts);
    assert_eq!(injected_failures, expected_injected_failures);
    for path in paths {
        assert!(
            !path.exists(),
            "temporary Audio remains at {}",
            path.display()
        );
    }
}

fn sibling_paths(directory: &Path, final_path: &Path) -> Vec<PathBuf> {
    let mut paths = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path != final_path)
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

#[test]
fn video_export_atomically_replaces_an_existing_destination() {
    let directory = tempfile::tempdir().unwrap();
    let final_path = directory.path().join("movie.mp4");
    fs::write(&final_path, b"existing destination").unwrap();

    let (result, probe) = run_export(&final_path, ExportFault::None);
    result.output.unwrap();
    assert!(result.published);
    assert_eq!(result.frames_exported, 2);
    assert_eq!(
        fs::read(&final_path).unwrap(),
        b"frame:0\nframe:1\nfinished\n"
    );
    assert!(sibling_paths(directory.path(), &final_path).is_empty());

    let probe = probe.lock().unwrap();
    assert_eq!(probe.finishes, 1);
    assert_eq!(
        probe.logical_paths,
        vec![final_path.to_string_lossy().into_owned(); 2]
    );
    assert_eq!(probe.writable_paths.len(), 2);
    assert_eq!(probe.writable_paths[0], probe.writable_paths[1]);
    let writable = Path::new(&probe.writable_paths[0]);
    assert_ne!(writable, final_path);
    assert_eq!(
        fs::canonicalize(writable.parent().unwrap()).unwrap(),
        fs::canonicalize(final_path.parent().unwrap()).unwrap()
    );
    assert_eq!(writable.extension(), final_path.extension());
}

#[test]
fn video_export_creates_a_previously_missing_destination() {
    let directory = tempfile::tempdir().unwrap();
    let final_path = directory.path().join("new-movie.mkv");

    let (result, _) = run_export(&final_path, ExportFault::None);
    result.output.unwrap();
    assert!(result.published);
    assert_eq!(
        fs::read(&final_path).unwrap(),
        b"frame:0\nframe:1\nfinished\n"
    );
    assert!(sibling_paths(directory.path(), &final_path).is_empty());
}

#[test]
fn frame_failure_preserves_existing_destination_and_removes_staging() {
    let directory = tempfile::tempdir().unwrap();
    let final_path = directory.path().join("movie.mp4");
    let sentinel = b"keep this existing video";
    fs::write(&final_path, sentinel).unwrap();

    let (result, probe) = run_export(&final_path, ExportFault::Frame(1));
    assert!(result.output.unwrap_err().to_string().contains("frame 1"));
    assert!(!result.published);
    assert_eq!(result.frames_exported, 1);
    assert_eq!(fs::read(&final_path).unwrap(), sentinel);
    assert!(sibling_paths(directory.path(), &final_path).is_empty());
    assert_eq!(probe.lock().unwrap().finishes, 1);
}

#[test]
fn finish_failure_preserves_existing_destination_and_removes_staging() {
    let directory = tempfile::tempdir().unwrap();
    let final_path = directory.path().join("movie.mp4");
    let sentinel = b"keep this existing video";
    fs::write(&final_path, sentinel).unwrap();

    let (result, probe) = run_export(&final_path, ExportFault::Finish);
    assert!(result.output.unwrap_err().to_string().contains("finish"));
    assert!(!result.published);
    assert_eq!(result.frames_exported, 2);
    assert_eq!(fs::read(&final_path).unwrap(), sentinel);
    assert!(sibling_paths(directory.path(), &final_path).is_empty());
    assert_eq!(probe.lock().unwrap().finishes, 1);
}

#[test]
fn missing_staging_output_fails_closed_without_touching_destination() {
    let directory = tempfile::tempdir().unwrap();
    let final_path = directory.path().join("movie.mp4");
    let sentinel = b"keep this existing video";
    fs::write(&final_path, sentinel).unwrap();

    let (result, probe) = run_export(&final_path, ExportFault::DeleteBeforePublish);
    assert!(
        result
            .output
            .unwrap_err()
            .to_string()
            .contains("staging file")
    );
    assert!(!result.published);
    assert_eq!(result.frames_exported, 2);
    assert_eq!(fs::read(&final_path).unwrap(), sentinel);
    assert!(sibling_paths(directory.path(), &final_path).is_empty());
    assert_eq!(probe.lock().unwrap().finishes, 1);
}

fn assert_panic_is_terminal_and_worker_recovers(
    fault: ExportFault,
    expected_frames_exported: u64,
    expected_message: &str,
) {
    let directory = tempfile::tempdir().unwrap();
    let failed_path = directory.path().join("failed.mp4");
    let sentinel = b"keep this existing video";
    fs::write(&failed_path, sentinel).unwrap();
    let (project, wave_path) = export_project_with_audio(directory.path());
    let (server, project, plan, probe) = export_server_for_project(fault, project);

    let failed = request_export(&server, &project, &plan, 9_100, &failed_path);
    let error = failed.output.unwrap_err().to_string();
    assert!(error.contains("panicked"), "{error}");
    assert!(error.contains(expected_message), "{error}");
    assert_eq!(failed.request_id, RenderRequestId::new(9_100));
    assert_eq!(failed.frames_exported, expected_frames_exported);
    assert!(!failed.published);
    assert_eq!(fs::read(&failed_path).unwrap(), sentinel);
    assert_eq!(
        sibling_paths(directory.path(), &failed_path),
        vec![wave_path.clone()]
    );
    assert_eq!(probe.lock().unwrap().finishes, 1);
    assert_no_additional_completion(&server);
    assert_temporary_audio_cleaned(&server, 1, 1, 0, 0);
    assert_runtime_audio_cleaned(&probe);

    let recovered = request_export(&server, &project, &plan, 9_101, &failed_path);
    recovered.output.unwrap();
    assert_eq!(recovered.request_id, RenderRequestId::new(9_101));
    assert_eq!(recovered.frames_exported, 2);
    assert!(recovered.published);
    assert_ne!(fs::read(&failed_path).unwrap(), sentinel);
    assert_eq!(
        sibling_paths(directory.path(), &failed_path),
        vec![wave_path]
    );
    assert_eq!(probe.lock().unwrap().finishes, 2);
    assert_no_additional_completion(&server);
    assert_temporary_audio_cleaned(&server, 2, 2, 0, 0);
    assert_runtime_audio_cleaned(&probe);
}

fn assert_runtime_audio_cleaned(probe: &Arc<Mutex<ExportProbe>>) {
    let paths = probe.lock().unwrap().runtime_audio_paths.clone();
    assert!(!paths.is_empty());
    for path in paths {
        assert!(
            !Path::new(&path).exists(),
            "temporary Audio remains at {path}"
        );
    }
}

#[test]
fn exporter_frame_panic_returns_failure_cleans_up_and_worker_recovers() {
    assert_panic_is_terminal_and_worker_recovers(
        ExportFault::PanicFrameOnce(1),
        1,
        "injected exporter frame 1 panic",
    );
}

#[test]
fn exporter_finish_panic_returns_failure_cleans_up_and_worker_recovers() {
    assert_panic_is_terminal_and_worker_recovers(
        ExportFault::PanicFinishOnce,
        2,
        "injected exporter finish panic",
    );
}

#[test]
fn active_video_job_pins_one_exporter_across_registry_replacement() {
    let directory = tempfile::tempdir().unwrap();
    let final_path = directory.path().join("pinned.mp4");
    let project = export_project();
    let plan = Arc::new(RenderPlanCompiler::compile(project.as_ref()).unwrap());
    let probe = Arc::new(Mutex::new(ExportProbe::default()));
    let replacement_calls = Arc::new(AtomicUsize::new(0));
    let plugins = Arc::new(PluginManager::new());
    plugins.register_export_plugin(Arc::new(StagingProbeExporter {
        fault: ExportFault::None,
        probe: Arc::clone(&probe),
        replacement: Some(ReplacementRegistration {
            manager: Arc::downgrade(&plugins),
            calls: Arc::clone(&replacement_calls),
        }),
    }));
    let server = RenderServer::new_with_cpu_preview(plugins, Arc::new(CacheManager::new()));

    let result = request_export(&server, &project, &plan, 9_200, &final_path);
    result.output.unwrap();
    assert!(result.published);
    assert_eq!(result.frames_exported, 2);
    assert_eq!(replacement_calls.load(Ordering::SeqCst), 0);
    {
        let probe = probe.lock().unwrap();
        assert_eq!(probe.frames, 2);
        assert_eq!(probe.finishes, 1);
    }
    let first_artifact = fs::read(&final_path).unwrap();
    assert_eq!(first_artifact, b"frame:0\nframe:1\nfinished\n");

    let replacement_result = request_export(&server, &project, &plan, 9_201, &final_path);
    let error = replacement_result.output.unwrap_err().to_string();
    assert!(error.contains("replacement exporter"), "{error}");
    assert!(!replacement_result.published);
    assert_eq!(replacement_result.frames_exported, 0);
    assert_eq!(replacement_calls.load(Ordering::SeqCst), 2);
    assert_eq!(fs::read(&final_path).unwrap(), first_artifact);
    assert!(sibling_paths(directory.path(), &final_path).is_empty());
}
