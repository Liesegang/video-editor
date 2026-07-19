//! Opt-in, loopback-only QA bridge for inspecting rendered egui components and
//! injecting real pointer events. Set `RUVIE_QA_PORT` (for example `39091`) at
//! process startup to enable it.
//!
//! HTTP API:
//! - `GET /health`
//! - `GET /v1/components`
//! - `GET /v1/components/{percent-encoded-stable-id}`
//! - `POST /v1/input/{move|press|release|click|double-click}` with
//!   `{ "x": 10, "y": 20, "coordinate_space": "points" }`
//! - `POST /v1/input/drag` with
//!   `{ "from": {"x": 10, "y": 20}, "to": {"x": 50, "y": 20}, "steps": 8 }`
//! - `POST /v1/input/scroll` with
//!   `{ "x": 10, "y": 20, "delta_x": 0, "delta_y": -240 }`
//! - `GET /v1/actions/{action-id}`
//! - `POST /v1/captures`
//! - `GET /v1/captures/{capture-id}`
//! - `GET /v1/captures/{capture-id}.png`

mod capture;
mod fixture;
mod input;
mod registry;
mod server;
mod state;

pub use fixture::{install_from_env as install_fixture_from_env, FixtureInfo};
pub use registry::{begin_frame, end_frame, register_component, register_component_with_metadata};

/// True only while the opt-in QA runtime is alive. Preview rendering uses
/// this to avoid calculating an image checksum in normal application runs.
pub(crate) fn is_enabled() -> bool {
    registry::is_enabled()
}

use input::{ActionTracker, InputSequencer};
use server::QaServer;
use std::sync::mpsc;
use std::sync::Arc;

const INPUT_QUEUE_CAPACITY: usize = 256;
pub const QA_PORT_ENV: &str = "RUVIE_QA_PORT";
pub const QA_PORT_FILE_ENV: &str = "RUVIE_QA_PORT_FILE";

pub struct QaRuntime {
    sequencer: InputSequencer,
    state_receiver: mpsc::Receiver<state::StateQuery>,
    captures: Arc<capture::CaptureStore>,
    server: QaServer,
}

impl QaRuntime {
    pub fn from_env(context: &egui::Context) -> Result<Option<Self>, String> {
        let raw_port = match std::env::var(QA_PORT_ENV) {
            Ok(port) => port,
            Err(std::env::VarError::NotPresent) => return Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err(format!("{QA_PORT_ENV} is not valid Unicode"));
            }
        };
        let port = raw_port.parse::<u16>().map_err(|error| {
            format!(
                "{QA_PORT_ENV} must be a TCP port between 0 and 65535, got {raw_port:?}: {error}"
            )
        })?;

        let (sender, receiver) = mpsc::sync_channel(INPUT_QUEUE_CAPACITY);
        let (state_sender, state_receiver) = mpsc::sync_channel(16);
        let tracker = Arc::new(ActionTracker::default());
        let server = QaServer::start(
            port,
            sender,
            state_sender,
            Arc::clone(&tracker),
            context.clone(),
        )
        .map_err(|error| format!("failed to start QA HTTP server: {error}"))?;
        if let Some(path) = qa_port_file_from_env()? {
            let endpoint = serde_json::json!({
                "host": server.address().ip().to_string(),
                "port": server.address().port(),
            });
            std::fs::write(&path, endpoint.to_string()).map_err(|error| {
                format!(
                    "failed to publish the bound QA port to {}: {error}",
                    path.display()
                )
            })?;
        }
        let captures = server.capture_store();
        let sequencer = InputSequencer::new(receiver, tracker);

        registry::set_enabled(true);
        log::info!(
            "QA HTTP API enabled at http://{} (loopback only)",
            server.address()
        );
        Ok(Some(Self {
            sequencer,
            state_receiver,
            captures,
            server,
        }))
    }

    pub fn inject_for_frame(&mut self, context: &egui::Context, raw_input: &mut egui::RawInput) {
        let snapshot = registry::snapshot();
        self.captures.receive_events(raw_input, snapshot.frame);
        let pixels_per_point = if snapshot.frame == 0 {
            context.pixels_per_point()
        } else {
            snapshot.pixels_per_point
        };
        self.sequencer
            .inject_for_frame(context, raw_input, pixels_per_point);
    }

    /// Issue at most one real eframe screenshot command during this egui pass.
    /// The backend returns its pixels as `Event::Screenshot` on a later frame.
    pub fn issue_capture_for_frame(&self, context: &egui::Context) {
        let current_frame = registry::snapshot().frame.saturating_add(1);
        self.captures.issue_for_frame(context, current_frame);
    }

    pub fn answer_state_queries(
        &mut self,
        project: &std::sync::Arc<std::sync::RwLock<library::model::project::Project>>,
        editor_context: &crate::state::context::EditorContext,
        dock_state: &egui_dock::DockState<crate::model::ui_types::Tab>,
        history_manager: &crate::action::HistoryManager,
    ) {
        while let Ok(query) = self.state_receiver.try_recv() {
            let response = project
                .read()
                .map_err(|error| format!("Project lock is poisoned: {error}"))
                .and_then(|project| {
                    state::snapshot(
                        registry::snapshot().frame,
                        &project,
                        editor_context,
                        dock_state,
                        history_manager,
                    )
                });
            if query.response.try_send(response).is_err() {
                log::debug!("QA state query receiver was dropped before the UI replied");
            }
        }
    }
}

fn qa_port_file_from_env() -> Result<Option<std::path::PathBuf>, String> {
    match std::env::var_os(QA_PORT_FILE_ENV) {
        Some(path) if !path.is_empty() => Ok(Some(path.into())),
        Some(_) => Err(format!("{QA_PORT_FILE_ENV} must not be empty")),
        None => Ok(None),
    }
}

impl Drop for QaRuntime {
    fn drop(&mut self) {
        registry::set_enabled(false);
        // `server` then stops and joins through its own Drop implementation.
        let _ = &self.server;
    }
}
