use super::capture::{CaptureLookup, CapturePhase, CaptureRequestError, CaptureStore};
use super::input::{
    ActionPhase, ActionTracker, DragRequest, InputAction, InputCommand, KeyRequest, PointerRequest,
    ScrollRequest, TextRequest,
};
use super::registry;
use super::state::StateQuery;
use serde_json::{json, Value};
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

const MAX_REQUEST_BYTES: usize = 64 * 1024;
const CLIENT_TIMEOUT: Duration = Duration::from_secs(2);
const STATE_TIMEOUT: Duration = Duration::from_secs(2);

pub struct QaServer {
    address: SocketAddr,
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    captures: Arc<CaptureStore>,
}

impl QaServer {
    pub fn start(
        port: u16,
        input_sender: SyncSender<InputCommand>,
        state_sender: SyncSender<StateQuery>,
        tracker: Arc<ActionTracker>,
        repaint_context: egui::Context,
    ) -> io::Result<Self> {
        // The host is intentionally not configurable. QA input injection must
        // never become reachable from another machine by configuration alone.
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port))?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        if !address.ip().is_loopback() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "QA server refused a non-loopback address",
            ));
        }

        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_shutdown = Arc::clone(&shutdown);
        let captures = Arc::new(CaptureStore::default());
        let thread_captures = Arc::clone(&captures);
        let thread = thread::Builder::new()
            .name("ruvie-qa-http".to_string())
            .spawn(move || {
                serve(
                    listener,
                    thread_shutdown,
                    input_sender,
                    state_sender,
                    tracker,
                    repaint_context,
                    thread_captures,
                );
            })?;

        Ok(Self {
            address,
            shutdown,
            thread: Some(thread),
            captures,
        })
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }

    pub(super) fn capture_store(&self) -> Arc<CaptureStore> {
        Arc::clone(&self.captures)
    }

    fn stop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            if thread.join().is_err() {
                log::error!("QA HTTP server thread panicked during shutdown");
            }
        }
    }
}

impl Drop for QaServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn serve(
    listener: TcpListener,
    shutdown: Arc<AtomicBool>,
    input_sender: SyncSender<InputCommand>,
    state_sender: SyncSender<StateQuery>,
    tracker: Arc<ActionTracker>,
    repaint_context: egui::Context,
    captures: Arc<CaptureStore>,
) {
    let mut next_action_id = 1_u64;
    while !shutdown.load(Ordering::Acquire) {
        match listener.accept() {
            Ok((mut stream, peer)) => {
                if let Err(error) = stream.set_nonblocking(false) {
                    log::warn!("QA HTTP client stream setup failed: {error}");
                    continue;
                }
                if let Err(error) = stream.set_write_timeout(Some(CLIENT_TIMEOUT)) {
                    log::warn!("QA HTTP client write timeout setup failed: {error}");
                    continue;
                }
                if !peer.ip().is_loopback() {
                    if let Err(error) = write_response(
                        &mut stream,
                        HttpResponse::json(403, json!({"error": "loopback clients only"})),
                    ) {
                        log::warn!("QA HTTP response write failed: {error}");
                    }
                    continue;
                }
                let response = match read_request(&mut stream) {
                    Ok(request) => route(
                        request,
                        &input_sender,
                        &state_sender,
                        &tracker,
                        &repaint_context,
                        &captures,
                        &mut next_action_id,
                    ),
                    Err(error) => HttpResponse::json(error.status, json!({"error": error.message})),
                };
                if let Err(error) = write_response(&mut stream, response) {
                    log::warn!("QA HTTP response write failed: {error}");
                }
                // Finish the response half of the connection explicitly. On
                // macOS, dropping a socket immediately after a short error
                // response can otherwise race the client's read and surface
                // as ECONNRESET even though the complete response was written.
                if let Err(error) = stream.shutdown(Shutdown::Write) {
                    log::debug!("QA HTTP response shutdown failed: {error}");
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => {
                log::error!("QA HTTP accept failed: {error}");
                thread::sleep(Duration::from_millis(25));
            }
        }
    }
}

struct HttpRequest {
    method: String,
    target: String,
    body: Vec<u8>,
}

struct RequestError {
    status: u16,
    message: String,
}

impl RequestError {
    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: 400,
            message: message.into(),
        }
    }
}

fn read_request(stream: &mut TcpStream) -> Result<HttpRequest, RequestError> {
    stream
        .set_read_timeout(Some(CLIENT_TIMEOUT))
        .map_err(|error| RequestError::bad_request(error.to_string()))?;
    let mut bytes = Vec::with_capacity(1024);
    let mut buffer = [0_u8; 2048];
    let header_end = loop {
        let read = stream.read(&mut buffer).map_err(|error| RequestError {
            status: 408,
            message: format!("failed to read request: {error}"),
        })?;
        if read == 0 {
            return Err(RequestError::bad_request("incomplete HTTP request"));
        }
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.len() > MAX_REQUEST_BYTES {
            return Err(RequestError {
                status: 413,
                message: "request exceeds 64 KiB".to_string(),
            });
        }
        if let Some(index) = find_bytes(&bytes, b"\r\n\r\n") {
            break index + 4;
        }
    };

    let (method, target, content_length) = {
        let headers = std::str::from_utf8(&bytes[..header_end]).map_err(|error| {
            RequestError::bad_request(format!("HTTP headers must be UTF-8: {error}"))
        })?;
        let mut lines = headers.split("\r\n");
        let request_line = lines
            .next()
            .ok_or_else(|| RequestError::bad_request("missing request line"))?;
        let mut request_parts = request_line.split_whitespace();
        let method = request_parts
            .next()
            .ok_or_else(|| RequestError::bad_request("missing HTTP method"))?
            .to_string();
        let target = request_parts
            .next()
            .ok_or_else(|| RequestError::bad_request("missing request target"))?
            .to_string();
        let version = request_parts
            .next()
            .ok_or_else(|| RequestError::bad_request("missing HTTP version"))?;
        if request_parts.next().is_some() || !version.starts_with("HTTP/1.") {
            return Err(RequestError::bad_request("invalid request line"));
        }

        let mut content_length = 0_usize;
        for line in lines {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            if name.trim().eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse::<usize>().map_err(|error| {
                    RequestError::bad_request(format!("invalid Content-Length: {error}"))
                })?;
            }
        }
        (method, target, content_length)
    };
    if header_end.saturating_add(content_length) > MAX_REQUEST_BYTES {
        return Err(RequestError {
            status: 413,
            message: "request exceeds 64 KiB".to_string(),
        });
    }

    while bytes.len() < header_end + content_length {
        let read = stream.read(&mut buffer).map_err(|error| RequestError {
            status: 408,
            message: format!("failed to read request body: {error}"),
        })?;
        if read == 0 {
            return Err(RequestError::bad_request("incomplete request body"));
        }
        bytes.extend_from_slice(&buffer[..read]);
        if bytes.len() > MAX_REQUEST_BYTES {
            return Err(RequestError {
                status: 413,
                message: "request exceeds 64 KiB".to_string(),
            });
        }
    }

    Ok(HttpRequest {
        method,
        target,
        body: bytes[header_end..header_end + content_length].to_vec(),
    })
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

struct HttpResponse {
    status: u16,
    body: HttpBody,
}

enum HttpBody {
    Json(Value),
    Png(Arc<[u8]>),
}

impl HttpResponse {
    fn json(status: u16, body: Value) -> Self {
        Self {
            status,
            body: HttpBody::Json(body),
        }
    }

    fn png(body: Arc<[u8]>) -> Self {
        Self {
            status: 200,
            body: HttpBody::Png(body),
        }
    }
}

fn route(
    request: HttpRequest,
    input_sender: &SyncSender<InputCommand>,
    state_sender: &SyncSender<StateQuery>,
    tracker: &ActionTracker,
    repaint_context: &egui::Context,
    captures: &CaptureStore,
    next_action_id: &mut u64,
) -> HttpResponse {
    let path = request.target.split('?').next().unwrap_or(&request.target);

    if request.method == "GET" && path == "/health" {
        return HttpResponse::json(
            200,
            json!({
                "ok": true,
                "service": "ruvie-qa",
                "api_version": 1,
                "frame": registry::snapshot().frame,
            }),
        );
    }

    if request.method == "POST" && path == "/v1/captures" {
        if request.body.iter().any(|byte| !byte.is_ascii_whitespace()) {
            return HttpResponse::json(400, json!({"error": "capture request body must be empty"}));
        }
        return match captures.request(registry::snapshot().frame) {
            Ok(metadata) => {
                repaint_context.request_repaint();
                HttpResponse::json(
                    202,
                    json!({
                        "queued": true,
                        "capture_id": metadata.capture_id,
                        "phase": metadata.phase,
                    }),
                )
            }
            Err(CaptureRequestError::QueueFull) => {
                HttpResponse::json(429, json!({"error": "capture queue is full"}))
            }
            Err(CaptureRequestError::Unavailable) => {
                HttpResponse::json(503, json!({"error": "capture state is unavailable"}))
            }
        };
    }

    if request.method == "GET" && path == "/v1/components" {
        return match serde_json::to_value(registry::snapshot()) {
            Ok(snapshot) => HttpResponse::json(200, snapshot),
            Err(error) => HttpResponse::json(500, json!({"error": error.to_string()})),
        };
    }

    if request.method == "GET" && path == "/v1/state" {
        let (response_sender, response_receiver) = mpsc::sync_channel(1);
        match state_sender.try_send(StateQuery {
            response: response_sender,
        }) {
            Ok(()) => {
                repaint_context.request_repaint();
                return match response_receiver.recv_timeout(STATE_TIMEOUT) {
                    Ok(Ok(snapshot)) => HttpResponse::json(200, snapshot),
                    Ok(Err(message)) => HttpResponse::json(500, json!({"error": message})),
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        HttpResponse::json(503, json!({"error": "UI state query timed out"}))
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => HttpResponse::json(
                        503,
                        json!({"error": "UI state responder is unavailable"}),
                    ),
                };
            }
            Err(TrySendError::Full(_)) => {
                return HttpResponse::json(429, json!({"error": "state query queue is full"}));
            }
            Err(TrySendError::Disconnected(_)) => {
                return HttpResponse::json(
                    503,
                    json!({"error": "UI state responder is unavailable"}),
                );
            }
        }
    }

    if request.method == "GET" {
        if let Some(raw_capture_id) = path.strip_prefix("/v1/captures/") {
            let (raw_capture_id, wants_png) = raw_capture_id
                .strip_suffix(".png")
                .map_or((raw_capture_id, false), |capture_id| (capture_id, true));
            let capture_id = match raw_capture_id.parse::<u64>() {
                Ok(capture_id) => capture_id,
                Err(_) => {
                    return HttpResponse::json(400, json!({"error": "invalid capture id"}));
                }
            };
            let current_frame = registry::snapshot().frame;
            if wants_png {
                return match captures.png(capture_id, current_frame) {
                    Ok(CaptureLookup::Ready(png)) => HttpResponse::png(png),
                    Ok(CaptureLookup::Pending(phase)) => {
                        repaint_context.request_repaint();
                        HttpResponse::json(
                            409,
                            json!({
                                "error": "capture is not ready",
                                "capture_id": capture_id,
                                "phase": phase,
                            }),
                        )
                    }
                    Ok(CaptureLookup::Failed(error)) => HttpResponse::json(
                        409,
                        json!({
                            "error": error,
                            "capture_id": capture_id,
                            "phase": "failed",
                        }),
                    ),
                    Ok(CaptureLookup::NotFound) => {
                        HttpResponse::json(404, json!({"error": "capture not found"}))
                    }
                    Err(error) => HttpResponse::json(503, json!({"error": error.to_string()})),
                };
            }
            return match captures.metadata(capture_id, current_frame) {
                Ok(Some(metadata)) => {
                    if matches!(
                        metadata.phase,
                        CapturePhase::Queued | CapturePhase::Capturing
                    ) {
                        repaint_context.request_repaint();
                    }
                    match serde_json::to_value(metadata) {
                        Ok(metadata) => HttpResponse::json(200, metadata),
                        Err(error) => HttpResponse::json(500, json!({"error": error.to_string()})),
                    }
                }
                Ok(None) => HttpResponse::json(404, json!({"error": "capture not found"})),
                Err(error) => HttpResponse::json(503, json!({"error": error.to_string()})),
            };
        }

        if let Some(encoded_id) = path.strip_prefix("/v1/components/") {
            let id = match percent_decode(encoded_id) {
                Ok(id) => id,
                Err(message) => return HttpResponse::json(400, json!({"error": message})),
            };
            return match registry::component(&id) {
                Some(component) => match serde_json::to_value(component) {
                    Ok(component) => HttpResponse::json(200, component),
                    Err(error) => HttpResponse::json(500, json!({"error": error.to_string()})),
                },
                None => HttpResponse::json(404, json!({"error": "component not found"})),
            };
        }

        if let Some(raw_id) = path.strip_prefix("/v1/actions/") {
            let id = match raw_id.parse::<u64>() {
                Ok(id) => id,
                Err(_) => {
                    return HttpResponse::json(400, json!({"error": "invalid action id"}));
                }
            };
            return match tracker.get(id) {
                Some(status) => match serde_json::to_value(status) {
                    Ok(status) => HttpResponse::json(200, status),
                    Err(error) => HttpResponse::json(500, json!({"error": error.to_string()})),
                },
                None => HttpResponse::json(404, json!({"error": "action not found"})),
            };
        }
    }

    let action = if request.method == "POST" {
        match path {
            "/v1/input/move" => parse_pointer(&request.body).map(InputAction::Move),
            "/v1/input/press" => parse_pointer(&request.body).map(InputAction::Press),
            "/v1/input/release" => parse_pointer(&request.body).map(InputAction::Release),
            "/v1/input/click" => parse_pointer(&request.body).map(InputAction::Click),
            "/v1/input/drag" => serde_json::from_slice::<DragRequest>(&request.body)
                .map(InputAction::Drag)
                .map_err(|error| format!("invalid JSON body: {error}")),
            "/v1/input/key" => serde_json::from_slice::<KeyRequest>(&request.body)
                .map(InputAction::Key)
                .map_err(|error| format!("invalid JSON body: {error}")),
            "/v1/input/text" => serde_json::from_slice::<TextRequest>(&request.body)
                .map(InputAction::Text)
                .map_err(|error| format!("invalid JSON body: {error}")),
            "/v1/input/scroll" => serde_json::from_slice::<ScrollRequest>(&request.body)
                .map(InputAction::Scroll)
                .map_err(|error| format!("invalid JSON body: {error}")),
            _ => return HttpResponse::json(404, json!({"error": "endpoint not found"})),
        }
    } else {
        return HttpResponse::json(
            405,
            json!({"error": "method not allowed", "allowed": ["GET", "POST"]}),
        );
    };

    let action = match action {
        Ok(action) => action,
        Err(message) => return HttpResponse::json(400, json!({"error": message})),
    };
    if let Err(message) = action.validate() {
        return HttpResponse::json(422, json!({"error": message}));
    }

    let action_id = *next_action_id;
    *next_action_id = next_action_id.saturating_add(1);
    tracker.set(action_id, ActionPhase::Queued);
    match input_sender.try_send(InputCommand {
        id: action_id,
        action,
    }) {
        Ok(()) => {
            repaint_context.request_repaint();
            HttpResponse::json(
                202,
                json!({"queued": true, "action_id": action_id, "phase": "queued"}),
            )
        }
        Err(TrySendError::Full(_)) => {
            tracker.remove(action_id);
            HttpResponse::json(429, json!({"error": "input queue is full"}))
        }
        Err(TrySendError::Disconnected(_)) => {
            tracker.remove(action_id);
            HttpResponse::json(503, json!({"error": "UI input queue is unavailable"}))
        }
    }
}

fn parse_pointer(body: &[u8]) -> Result<PointerRequest, String> {
    serde_json::from_slice(body).map_err(|error| format!("invalid JSON body: {error}"))
}

fn percent_decode(value: &str) -> Result<String, &'static str> {
    let mut decoded = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                if index + 2 >= bytes.len() {
                    return Err("invalid percent-encoded component id");
                }
                let high =
                    hex_value(bytes[index + 1]).ok_or("invalid percent-encoded component id")?;
                let low =
                    hex_value(bytes[index + 2]).ok_or("invalid percent-encoded component id")?;
                decoded.push((high << 4) | low);
                index += 3;
            }
            byte => {
                decoded.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(decoded).map_err(|error| {
        log::debug!("invalid UTF-8 in percent-decoded component id: {error}");
        "component id must be UTF-8"
    })
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn write_response(stream: &mut TcpStream, response: HttpResponse) -> io::Result<()> {
    let (content_type, body): (&str, std::borrow::Cow<'_, [u8]>) = match &response.body {
        HttpBody::Json(value) => (
            "application/json",
            std::borrow::Cow::Owned(
                serde_json::to_vec(value)
                    .unwrap_or_else(|_| b"{\"error\":\"failed to serialize response\"}".to_vec()),
            ),
        ),
        HttpBody::Png(png) => ("image/png", std::borrow::Cow::Borrowed(png)),
    };
    let reason = match response.status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        408 => "Request Timeout",
        413 => "Payload Too Large",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Error",
    };
    write!(
        stream,
        "HTTP/1.1 {} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\r\n",
        response.status,
        body.len()
    )?;
    stream.write_all(&body)?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn request_bytes(address: SocketAddr, request: &str) -> Vec<u8> {
        let mut stream = TcpStream::connect(address).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        stream.shutdown(std::net::Shutdown::Write).unwrap();
        let mut response = Vec::new();
        stream.read_to_end(&mut response).unwrap();
        response
    }

    fn request(address: SocketAddr, request: &str) -> String {
        String::from_utf8(request_bytes(address, request)).unwrap()
    }

    #[test]
    fn server_binds_only_to_loopback_and_stops_cleanly() {
        let (sender, _receiver) = mpsc::sync_channel(4);
        let (state_sender, _state_receiver) = mpsc::sync_channel(1);
        let tracker = Arc::new(ActionTracker::default());
        let server =
            QaServer::start(0, sender, state_sender, tracker, egui::Context::default()).unwrap();
        assert!(server.address().ip().is_loopback());

        let response = request(
            server.address(),
            "GET /health HTTP/1.1\r\nHost: localhost\r\n\r\n",
        );
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.contains("\"service\":\"ruvie-qa\""));
        drop(server);
    }

    #[test]
    fn click_endpoint_queues_a_coordinate_action() {
        let (sender, receiver) = mpsc::sync_channel(4);
        let (state_sender, _state_receiver) = mpsc::sync_channel(1);
        let tracker = Arc::new(ActionTracker::default());
        let server = QaServer::start(
            0,
            sender,
            state_sender,
            Arc::clone(&tracker),
            egui::Context::default(),
        )
        .unwrap();
        let body = r#"{"x":12.5,"y":42.0,"button":"primary"}"#;
        let http = format!(
            "POST /v1/input/click HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let response = request(server.address(), &http);
        assert!(response.starts_with("HTTP/1.1 202 Accepted"));
        let command = receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(command.id, 1);
        assert!(matches!(command.action, InputAction::Click(_)));
        assert_eq!(tracker.get(1).unwrap().phase, ActionPhase::Queued);
    }

    #[test]
    fn capture_endpoints_return_real_screenshot_png_and_metadata() {
        let (sender, _receiver) = mpsc::sync_channel(4);
        let (state_sender, _state_receiver) = mpsc::sync_channel(1);
        let context = egui::Context::default();
        let server = QaServer::start(
            0,
            sender,
            state_sender,
            Arc::new(ActionTracker::default()),
            context.clone(),
        )
        .unwrap();
        let response = request(
            server.address(),
            "POST /v1/captures HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n",
        );
        assert!(response.starts_with("HTTP/1.1 202 Accepted"));
        assert!(response.contains("\"capture_id\":1"));
        assert!(response.contains("\"phase\":\"queued\""));

        let nonempty_body = "{}";
        let response = request(
            server.address(),
            &format!(
                "POST /v1/captures HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{nonempty_body}",
                nonempty_body.len()
            ),
        );
        assert!(response.starts_with("HTTP/1.1 400 Bad Request"));
        assert!(response.contains("capture request body must be empty"));

        let pending_png = request(
            server.address(),
            "GET /v1/captures/1.png HTTP/1.1\r\nHost: localhost\r\n\r\n",
        );
        assert!(pending_png.starts_with("HTTP/1.1 409 Conflict"));

        let captures = server.capture_store();
        let output = context.run(egui::RawInput::default(), |context| {
            captures.issue_for_frame(context, 1);
        });
        let user_data = output
            .viewport_output
            .values()
            .flat_map(|output| &output.commands)
            .find_map(|command| match command {
                egui::ViewportCommand::Screenshot(user_data) => Some(user_data.clone()),
                _ => None,
            })
            .expect("capture should issue eframe's screenshot command");
        captures.receive_events(
            &egui::RawInput {
                events: vec![egui::Event::Screenshot {
                    viewport_id: egui::ViewportId::ROOT,
                    user_data,
                    image: Arc::new(egui::ColorImage::filled(
                        [2, 1],
                        egui::Color32::from_rgb(5, 10, 15),
                    )),
                }],
                ..Default::default()
            },
            1,
        );

        let metadata = request(
            server.address(),
            "GET /v1/captures/1 HTTP/1.1\r\nHost: localhost\r\n\r\n",
        );
        assert!(metadata.starts_with("HTTP/1.1 200 OK"));
        assert!(metadata.contains("\"phase\":\"ready\""));
        assert!(metadata.contains("\"width\":2"));
        assert!(metadata.contains("\"height\":1"));
        assert!(metadata.contains("\"sha256\":"));

        let png = request_bytes(
            server.address(),
            "GET /v1/captures/1.png HTTP/1.1\r\nHost: localhost\r\n\r\n",
        );
        assert!(png.starts_with(b"HTTP/1.1 200 OK\r\n"));
        let header_end = find_bytes(&png, b"\r\n\r\n").unwrap() + 4;
        assert!(String::from_utf8_lossy(&png[..header_end]).contains("Content-Type: image/png"));
        assert!(png[header_end..].starts_with(b"\x89PNG\r\n\x1a\n"));
    }

    #[test]
    fn concurrent_capture_requests_receive_unique_ids() {
        let (sender, _receiver) = mpsc::sync_channel(4);
        let (state_sender, _state_receiver) = mpsc::sync_channel(1);
        let server = QaServer::start(
            0,
            sender,
            state_sender,
            Arc::new(ActionTracker::default()),
            egui::Context::default(),
        )
        .unwrap();
        let address = server.address();
        let requesters = (0..4)
            .map(|_| {
                std::thread::spawn(move || {
                    request(
                        address,
                        "POST /v1/captures HTTP/1.1\r\nHost: localhost\r\nContent-Length: 0\r\n\r\n",
                    )
                })
            })
            .collect::<Vec<_>>();
        let mut ids = requesters
            .into_iter()
            .map(|requester| {
                let response = requester.join().unwrap();
                assert!(response.starts_with("HTTP/1.1 202 Accepted"));
                let (_, body) = response.split_once("\r\n\r\n").unwrap();
                serde_json::from_str::<Value>(body).unwrap()["capture_id"]
                    .as_u64()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 4);
    }

    #[test]
    fn malformed_input_returns_an_error_without_queueing() {
        let (sender, receiver) = mpsc::sync_channel(4);
        let (state_sender, _state_receiver) = mpsc::sync_channel(1);
        let server = QaServer::start(
            0,
            sender,
            state_sender,
            Arc::new(ActionTracker::default()),
            egui::Context::default(),
        )
        .unwrap();
        let body = "{}";
        let http = format!(
            "POST /v1/input/click HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let response = request(server.address(), &http);
        assert!(response.starts_with("HTTP/1.1 400 Bad Request"));
        let (_, body) = response
            .split_once("\r\n\r\n")
            .expect("response should contain HTTP headers and a JSON body");
        let body: Value = serde_json::from_str(body).expect("error body should be valid JSON");
        let message = body["error"]
            .as_str()
            .expect("error response should contain an error message");
        assert!(message.starts_with("invalid JSON body:"));
        assert!(message.contains("missing field `x`"));
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn component_endpoint_returns_the_published_stable_id() {
        struct DisableRegistryOnDrop;
        impl Drop for DisableRegistryOnDrop {
            fn drop(&mut self) {
                registry::set_enabled(false);
            }
        }

        registry::set_enabled(true);
        let _guard = DisableRegistryOnDrop;
        let context = egui::Context::default();
        registry::begin_frame(&context);
        registry::register_component(
            "timeline.clip:test",
            "timeline_clip",
            egui::Rect::from_min_size(egui::pos2(2.0, 4.0), egui::vec2(20.0, 10.0)),
        );
        registry::end_frame();

        let (sender, _receiver) = mpsc::sync_channel(4);
        let (state_sender, _state_receiver) = mpsc::sync_channel(1);
        let server = QaServer::start(
            0,
            sender,
            state_sender,
            Arc::new(ActionTracker::default()),
            context,
        )
        .unwrap();
        let response = request(
            server.address(),
            "GET /v1/components/timeline.clip%3Atest HTTP/1.1\r\nHost: localhost\r\n\r\n",
        );
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.contains("\"id\":\"timeline.clip:test\""));
        assert!(response.contains("\"type\":\"timeline_clip\""));
    }

    #[test]
    fn occupied_port_is_reported_as_a_start_error() {
        let occupied = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = occupied.local_addr().unwrap().port();
        let (sender, _receiver) = mpsc::sync_channel(1);
        let (state_sender, _state_receiver) = mpsc::sync_channel(1);
        let result = QaServer::start(
            port,
            sender,
            state_sender,
            Arc::new(ActionTracker::default()),
            egui::Context::default(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn key_endpoint_queues_a_frame_spanning_keyboard_action() {
        let (sender, receiver) = mpsc::sync_channel(4);
        let (state_sender, _state_receiver) = mpsc::sync_channel(1);
        let server = QaServer::start(
            0,
            sender,
            state_sender,
            Arc::new(ActionTracker::default()),
            egui::Context::default(),
        )
        .unwrap();
        let body = r#"{"key":"space","pressed":true}"#;
        let http = format!(
            "POST /v1/input/key HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let response = request(server.address(), &http);
        assert!(response.starts_with("HTTP/1.1 202 Accepted"));
        assert!(matches!(
            receiver
                .recv_timeout(Duration::from_secs(1))
                .unwrap()
                .action,
            InputAction::Key(KeyRequest { pressed: true, .. })
        ));
    }

    #[test]
    fn state_endpoint_is_answered_on_demand_by_the_ui_side() {
        let (sender, _receiver) = mpsc::sync_channel(1);
        let (state_sender, state_receiver) = mpsc::sync_channel(1);
        let server = QaServer::start(
            0,
            sender,
            state_sender,
            Arc::new(ActionTracker::default()),
            egui::Context::default(),
        )
        .unwrap();
        let address = server.address();
        let requester = std::thread::spawn(move || {
            request(address, "GET /v1/state HTTP/1.1\r\nHost: localhost\r\n\r\n")
        });
        let query = state_receiver.recv_timeout(Duration::from_secs(1)).unwrap();
        query
            .response
            .send(Ok(
                json!({"frame": 42, "project": {"name": "authoritative"}}),
            ))
            .unwrap();
        let response = requester.join().unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.contains("\"frame\":42"));
        assert!(response.contains("\"name\":\"authoritative\""));
    }
}
