use super::{RenderRequestId, RenderServer};
use crate::cache::CacheManager;
use crate::core::render_plan::RenderPlanCompiler;
use crate::error::LibraryError;
use crate::model::authoring::{AuthoringProject, MediaTime, RationalRate};
use crate::plugin::{
    ExportDestination, ExportFrame, ExportPlugin, ExportSettings, Plugin, PluginManager,
};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
enum ExportFault {
    #[default]
    None,
    Frame(usize),
    Finish,
    DeleteBeforePublish,
}

#[derive(Debug, Default)]
struct ExportProbe {
    logical_paths: Vec<String>,
    writable_paths: Vec<String>,
    frames: usize,
    finishes: usize,
}

struct StagingProbeExporter {
    fault: ExportFault,
    probe: Arc<Mutex<ExportProbe>>,
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
        let frame_index = {
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
            frame_index
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
        self.probe
            .lock()
            .map_err(|_| LibraryError::Runtime("atomic export probe lock poisoned".to_string()))?
            .finishes += 1;
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

fn export_project() -> Arc<AuthoringProject> {
    Arc::new(
        AuthoringProject::new(
            "atomic video export",
            4,
            2,
            RationalRate::new(24, 1).unwrap(),
            MediaTime::new(2, 24).unwrap(),
        )
        .unwrap(),
    )
}

fn run_export(
    output_path: &Path,
    fault: ExportFault,
) -> (super::AuthoringExportResult, Arc<Mutex<ExportProbe>>) {
    let project = export_project();
    let timeline_id = project.root_timeline_id;
    let plan = Arc::new(RenderPlanCompiler::compile(project.as_ref()).unwrap());
    let probe = Arc::new(Mutex::new(ExportProbe::default()));
    let plugins = Arc::new(PluginManager::new());
    plugins.register_export_plugin(Arc::new(StagingProbeExporter {
        fault,
        probe: Arc::clone(&probe),
    }));
    let server = RenderServer::new(plugins, Arc::new(CacheManager::new()));
    assert!(server.send_authoring_video_export_request(
        RenderRequestId::new(9_000),
        project,
        plan,
        timeline_id,
        output_path.to_string_lossy().into_owned(),
    ));
    let result = server
        .rx_authoring_export_result
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
    (result, probe)
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
