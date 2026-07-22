use super::probe::MetadataOutputProbeRequest;
use super::server::HttpResponse;
use serde_json::{json, Value};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::time::{Duration, Instant};

const UI_QUERY_TIMEOUT: Duration = Duration::from_secs(2);

/// One bounded request from the loopback HTTP thread to the UI thread.
///
/// Responses are produced on demand from the authoritative Project and
/// transient editor state. The QA bridge keeps no synchronized model cache.
pub enum UiQueryKind {
    Snapshot,
    MetadataOutput(MetadataOutputProbeRequest),
}

pub struct UiQuery {
    pub kind: UiQueryKind,
    pub deadline: Instant,
    pub response: SyncSender<Result<Value, String>>,
}

pub(super) fn snapshot_response(
    sender: &SyncSender<UiQuery>,
    repaint_context: &egui::Context,
) -> HttpResponse {
    query_response(UiQueryKind::Snapshot, sender, repaint_context, "state")
}

pub(super) fn query_response(
    kind: UiQueryKind,
    sender: &SyncSender<UiQuery>,
    repaint_context: &egui::Context,
    label: &str,
) -> HttpResponse {
    let (response_sender, response_receiver) = mpsc::sync_channel(1);
    let query = UiQuery {
        kind,
        deadline: Instant::now() + UI_QUERY_TIMEOUT,
        response: response_sender,
    };
    match sender.try_send(query) {
        Ok(()) => {
            repaint_context.request_repaint();
            match response_receiver.recv_timeout(UI_QUERY_TIMEOUT) {
                Ok(Ok(value)) => HttpResponse::json(200, value),
                Ok(Err(message)) => HttpResponse::json(500, json!({"error": message})),
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    HttpResponse::json(503, json!({"error": format!("UI {label} query timed out")}))
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    HttpResponse::json(503, json!({"error": "UI query responder is unavailable"}))
                }
            }
        }
        Err(TrySendError::Full(_)) => {
            HttpResponse::json(429, json!({"error": "UI query queue is full"}))
        }
        Err(TrySendError::Disconnected(_)) => {
            HttpResponse::json(503, json!({"error": "UI query responder is unavailable"}))
        }
    }
}
