use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use crate::error::LibraryError;
use crate::model::authoring::{AuthoringProject, MediaTime, RationalRate};
use crate::model::frame::Image;
use crate::plugin::{ExportFrame, ExportPlugin, ExportSettings, Plugin};

use super::PluginManager;

const EXPORTER_ID: &str = "callback-lock-exporter";
const CALLBACK_TIMEOUT: Duration = Duration::from_secs(2);

struct NoopExporter;

impl Plugin for NoopExporter {
    fn id(&self) -> &str {
        EXPORTER_ID
    }

    fn name(&self) -> String {
        "No-op Exporter".to_string()
    }

    fn category(&self) -> String {
        "Tests".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl ExportPlugin for NoopExporter {
    fn export_frame(
        &self,
        _path: &str,
        _frame: &ExportFrame,
        _settings: &ExportSettings,
    ) -> Result<(), LibraryError> {
        Ok(())
    }
}

struct ReentrantIdentityExporter {
    manager: Weak<PluginManager>,
    callback_completed: Arc<AtomicBool>,
}

impl Plugin for ReentrantIdentityExporter {
    fn id(&self) -> &str {
        if let Some(manager) = self.manager.upgrade() {
            let _inventory = manager.get_available_exporters();
            self.callback_completed.store(true, Ordering::SeqCst);
        }
        EXPORTER_ID
    }

    fn name(&self) -> String {
        "Reentrant Identity Exporter".to_string()
    }

    fn category(&self) -> String {
        "Tests".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl ExportPlugin for ReentrantIdentityExporter {
    fn export_frame(
        &self,
        _path: &str,
        _frame: &ExportFrame,
        _settings: &ExportSettings,
    ) -> Result<(), LibraryError> {
        Ok(())
    }
}

struct ReentrantFinishExporter {
    manager: Weak<PluginManager>,
}

impl Plugin for ReentrantFinishExporter {
    fn id(&self) -> &str {
        EXPORTER_ID
    }

    fn name(&self) -> String {
        "Reentrant Finish Exporter".to_string()
    }

    fn category(&self) -> String {
        "Tests".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl ExportPlugin for ReentrantFinishExporter {
    fn export_frame(
        &self,
        _path: &str,
        _frame: &ExportFrame,
        _settings: &ExportSettings,
    ) -> Result<(), LibraryError> {
        Ok(())
    }

    fn finish_export(&self, _path: &str, _settings: &ExportSettings) -> Result<(), LibraryError> {
        let manager = self.manager.upgrade().ok_or_else(|| {
            LibraryError::Plugin("test PluginManager was dropped during callback".to_string())
        })?;
        manager.register_export_plugin(Arc::new(NoopExporter));
        Ok(())
    }
}

struct BlockingExporter {
    entered: Mutex<Option<Sender<()>>>,
    release: Mutex<Receiver<()>>,
}

impl Plugin for BlockingExporter {
    fn id(&self) -> &str {
        EXPORTER_ID
    }

    fn name(&self) -> String {
        "Blocking Exporter".to_string()
    }

    fn category(&self) -> String {
        "Tests".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl ExportPlugin for BlockingExporter {
    fn export_frame(
        &self,
        _path: &str,
        _frame: &ExportFrame,
        _settings: &ExportSettings,
    ) -> Result<(), LibraryError> {
        let sender = self
            .entered
            .lock()
            .map_err(|_| LibraryError::Plugin("test entered lock was poisoned".to_string()))?
            .take()
            .ok_or_else(|| {
                LibraryError::Plugin("blocking test exporter was invoked twice".to_string())
            })?;
        sender.send(()).map_err(|error| {
            LibraryError::Plugin(format!("cannot report callback entry: {error}"))
        })?;
        self.release
            .lock()
            .map_err(|_| LibraryError::Plugin("test release lock was poisoned".to_string()))?
            .recv_timeout(CALLBACK_TIMEOUT)
            .map_err(|error| LibraryError::Plugin(format!("callback release failed: {error}")))?;
        Ok(())
    }
}

fn export_fixture() -> Result<(ExportFrame, ExportSettings), LibraryError> {
    let project = AuthoringProject::new(
        "plugin callback lock fixture",
        1,
        1,
        RationalRate::new(30, 1).map_err(LibraryError::Validation)?,
        MediaTime::new(1, 1).map_err(LibraryError::Validation)?,
    )
    .map_err(LibraryError::Validation)?;
    let timeline = project
        .timelines
        .get(&project.root_timeline_id)
        .ok_or_else(|| LibraryError::Validation("fixture root Timeline is missing".to_string()))?;
    let settings = ExportSettings::from_authoring_project(&project, timeline)?;
    let frame = ExportFrame::from_authoring_render(&project, Image::new(1, 1, vec![0, 0, 0, 255]))?;
    Ok((frame, settings))
}

#[test]
fn registration_resolves_plugin_identity_before_taking_write_lock()
-> Result<(), Box<dyn std::error::Error>> {
    let manager = Arc::new(PluginManager::new());
    let callback_completed = Arc::new(AtomicBool::new(false));
    let plugin = ReentrantIdentityExporter {
        manager: Arc::downgrade(&manager),
        callback_completed: Arc::clone(&callback_completed),
    };
    let (completed_tx, completed_rx) = mpsc::channel();
    let worker_manager = Arc::clone(&manager);
    let worker = std::thread::spawn(move || {
        worker_manager.register_export_plugin(Arc::new(plugin));
        completed_tx.send(()).map_err(|error| error.to_string())
    });

    completed_rx.recv_timeout(CALLBACK_TIMEOUT)?;
    worker
        .join()
        .map_err(|_| "registration worker panicked")?
        .map_err(std::io::Error::other)?;
    assert!(callback_completed.load(Ordering::SeqCst));
    Ok(())
}

#[test]
fn exporter_callback_can_replace_its_endpoint_reentrantly() -> Result<(), Box<dyn std::error::Error>>
{
    let manager = Arc::new(PluginManager::new());
    manager.register_export_plugin(Arc::new(ReentrantFinishExporter {
        manager: Arc::downgrade(&manager),
    }));
    let (completed_tx, completed_rx) = mpsc::channel();
    let worker_manager = Arc::clone(&manager);
    let worker = std::thread::spawn(move || {
        let settings = ExportSettings::for_dimensions(1, 1, 30.0);
        let result = worker_manager
            .finish_export(EXPORTER_ID, "unused", &settings)
            .map_err(|error| error.to_string());
        completed_tx.send(result).map_err(|error| error.to_string())
    });

    completed_rx.recv_timeout(CALLBACK_TIMEOUT)??;
    worker
        .join()
        .map_err(|_| "export callback worker panicked")?
        .map_err(std::io::Error::other)?;
    assert!(
        manager
            .get_export_plugin_properties(EXPORTER_ID)
            .is_some_and(|properties| properties.is_empty())
    );
    Ok(())
}

#[test]
fn concurrent_registration_completes_while_export_callback_is_running()
-> Result<(), Box<dyn std::error::Error>> {
    let manager = Arc::new(PluginManager::new());
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    manager.register_export_plugin(Arc::new(BlockingExporter {
        entered: Mutex::new(Some(entered_tx)),
        release: Mutex::new(release_rx),
    }));
    let (frame, settings) = export_fixture()?;
    let callback_manager = Arc::clone(&manager);
    let callback = std::thread::spawn(move || {
        callback_manager.export_frame(EXPORTER_ID, "unused", &frame, &settings)
    });
    entered_rx.recv_timeout(CALLBACK_TIMEOUT)?;

    let (registered_tx, registered_rx) = mpsc::channel();
    let registration_manager = Arc::clone(&manager);
    let registration = std::thread::spawn(move || {
        registration_manager.register_export_plugin(Arc::new(NoopExporter));
        registered_tx.send(()).map_err(|error| error.to_string())
    });
    registered_rx.recv_timeout(CALLBACK_TIMEOUT)?;
    release_tx.send(())?;

    callback
        .join()
        .map_err(|_| "export callback worker panicked")??;
    registration
        .join()
        .map_err(|_| "registration worker panicked")?
        .map_err(std::io::Error::other)?;
    assert!(manager.get_export_plugin(EXPORTER_ID).is_some());
    Ok(())
}
