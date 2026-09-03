use std::collections::{HashMap, VecDeque};
use std::io::{Cursor, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use eframe::egui;
use serde::Serialize;
use serde_json::{json, Value};

static ACTIVE: OnceLock<Arc<Mutex<Shared>>> = OnceLock::new();

#[derive(Clone, Serialize)]
pub struct Component {
    pub id: String,
    pub kind: String,
    pub rect: [f32; 4],
    pub metadata: Value,
}

#[derive(Default)]
struct Shared {
    input_frames: VecDeque<Vec<egui::Event>>,
    components: HashMap<String, Component>,
    next_components: HashMap<String, Component>,
    state: Value,
    capture_requested: bool,
    capture_issued: bool,
    capture_png: Option<Vec<u8>>,
    frame: u64,
}

pub struct QaBridge {
    shared: Arc<Mutex<Shared>>,
}

impl QaBridge {
    pub fn from_env(ctx: &egui::Context) -> Option<Self> {
        let port = std::env::var("RUVIE_QA_PORT").ok()?.parse::<u16>().ok()?;
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).ok()?;
        listener.set_nonblocking(true).ok()?;
        let shared = Arc::new(Mutex::new(Shared::default()));
        drop(ACTIVE.set(Arc::clone(&shared)));
        let server_shared = Arc::clone(&shared);
        let repaint = ctx.clone();
        thread::Builder::new()
            .name("ruvie-qa-http".to_string())
            .spawn(move || serve(listener, server_shared, repaint))
            .ok()?;
        Some(Self { shared })
    }

    pub fn inject_for_frame(&self, raw_input: &mut egui::RawInput, ctx: &egui::Context) {
        let screenshots = raw_input
            .events
            .iter()
            .filter_map(|event| match event {
                egui::Event::Screenshot { image, .. } => Some(Arc::clone(image)),
                _ => None,
            })
            .collect::<Vec<_>>();
        if let Ok(mut shared) = self.shared.lock() {
            if let Some(events) = shared.input_frames.pop_front() {
                raw_input.events.extend(events);
            }
            if !shared.input_frames.is_empty() {
                ctx.request_repaint();
            }
            if let Some(image) = screenshots.last() {
                shared.capture_png = encode_png(image);
                shared.capture_issued = false;
            }
        }
    }

    pub fn begin_frame(&self) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.frame = shared.frame.wrapping_add(1);
            shared.next_components.clear();
        }
    }

    pub fn issue_capture(&self, ctx: &egui::Context) {
        if let Ok(mut shared) = self.shared.lock() {
            if shared.capture_requested && !shared.capture_issued {
                shared.capture_issued = true;
                shared.capture_requested = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
        }
    }

    pub fn publish_state(&self, state: Value) {
        if let Ok(mut shared) = self.shared.lock() {
            shared.state = state;
            shared.components = std::mem::take(&mut shared.next_components);
        }
    }
}

pub fn register_component(id: impl Into<String>, kind: &str, rect: egui::Rect, metadata: Value) {
    let Some(shared) = ACTIVE.get() else {
        return;
    };
    let id = id.into();
    if let Ok(mut shared) = shared.lock() {
        shared.next_components.insert(
            id.clone(),
            Component {
                id,
                kind: kind.to_string(),
                rect: [rect.min.x, rect.min.y, rect.max.x, rect.max.y],
                metadata,
            },
        );
    }
}

fn serve(listener: TcpListener, shared: Arc<Mutex<Shared>>, repaint: egui::Context) {
    loop {
        match listener.accept() {
            Ok((mut stream, _)) => {
                // On Windows an accepted socket can inherit the listener's non-blocking
                // mode. Requests with a body may then be read before the body arrives,
                // making otherwise valid drag/scroll QA input fail with WSAEWOULDBLOCK.
                if stream.set_nonblocking(false).is_err() {
                    continue;
                }
                let response = read_request(&mut stream)
                    .map(|request| route(request, &shared, &repaint))
                    .unwrap_or_else(|error| Response::json(400, json!({"error": error})));
                drop(write_response(&mut stream, response));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10));
            }
            Err(_) => thread::sleep(Duration::from_millis(25)),
        }
    }
}

struct Request {
    method: String,
    target: String,
    body: Vec<u8>,
}

fn read_request(stream: &mut TcpStream) -> Result<Request, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let count = stream
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            return Err("incomplete request".to_string());
        }
        bytes.extend_from_slice(&buffer[..count]);
        if bytes.len() > 64 * 1024 {
            return Err("request too large".to_string());
        }
        if let Some(index) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break index + 4;
        }
    };
    let header = std::str::from_utf8(&bytes[..header_end]).map_err(|error| error.to_string())?;
    let mut lines = header.lines();
    let mut request_line = lines
        .next()
        .ok_or_else(|| "missing request line".to_string())?
        .split_whitespace();
    let method = request_line.next().unwrap_or_default().to_string();
    let target = request_line.next().unwrap_or_default().to_string();
    let content_length = lines
        .find_map(|line| {
            line.split_once(':').and_then(|(name, value)| {
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().ok())
                    .flatten()
            })
        })
        .unwrap_or(0);
    while bytes.len() < header_end + content_length {
        let count = stream
            .read(&mut buffer)
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&buffer[..count]);
    }
    Ok(Request {
        method,
        target,
        body: bytes[header_end..bytes.len().min(header_end + content_length)].to_vec(),
    })
}

fn route(request: Request, shared: &Arc<Mutex<Shared>>, repaint: &egui::Context) -> Response {
    match (request.method.as_str(), request.target.as_str()) {
        ("GET", "/health") => Response::json(200, json!({"status": "ok"})),
        ("GET", "/v1/components") => {
            let components = shared
                .lock()
                .ok()
                .map(|state| state.components.values().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            Response::json(200, json!({"components": components}))
        }
        ("GET", "/v1/state") => Response::json(
            200,
            shared
                .lock()
                .ok()
                .map(|state| state.state.clone())
                .unwrap_or(Value::Null),
        ),
        ("POST", "/v1/input/click") => input_click(&request.body, shared, repaint),
        ("POST", "/v1/input/drag") => input_drag(&request.body, shared, repaint),
        ("POST", "/v1/input/scroll") => input_scroll(&request.body, shared, repaint),
        ("POST", "/v1/captures") => {
            if let Ok(mut state) = shared.lock() {
                state.capture_requested = true;
                state.capture_png = None;
            }
            repaint.request_repaint();
            Response::json(202, json!({"capture_id": 1, "phase": "queued"}))
        }
        ("GET", "/v1/captures/1") => {
            let ready = shared
                .lock()
                .ok()
                .is_some_and(|state| state.capture_png.is_some());
            Response::json(
                200,
                json!({"capture_id": 1, "phase": if ready {"ready"} else {"queued"}}),
            )
        }
        ("GET", "/v1/captures/1.png") => shared
            .lock()
            .ok()
            .and_then(|state| state.capture_png.clone())
            .map(Response::png)
            .unwrap_or_else(|| Response::json(409, json!({"error": "capture not ready"}))),
        _ => Response::json(404, json!({"error": "not found"})),
    }
}

fn point(value: &Value) -> Option<egui::Pos2> {
    Some(egui::pos2(
        value.get("x")?.as_f64()? as f32,
        value.get("y")?.as_f64()? as f32,
    ))
}

fn pointer_button(value: &Value) -> egui::PointerButton {
    match value.get("button").and_then(Value::as_str) {
        Some("middle") => egui::PointerButton::Middle,
        Some("secondary") | Some("right") => egui::PointerButton::Secondary,
        _ => egui::PointerButton::Primary,
    }
}

fn input_click(body: &[u8], shared: &Arc<Mutex<Shared>>, repaint: &egui::Context) -> Response {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return Response::json(400, json!({"error": "invalid JSON"}));
    };
    let Some(pos) = point(&value) else {
        return Response::json(400, json!({"error": "x and y are required"}));
    };
    let button = pointer_button(&value);
    queue(
        shared,
        vec![
            vec![egui::Event::PointerMoved(pos)],
            vec![egui::Event::PointerButton {
                pos,
                button,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
            vec![egui::Event::PointerButton {
                pos,
                button,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ],
        repaint,
    );
    Response::json(202, json!({"phase": "queued"}))
}

fn input_drag(body: &[u8], shared: &Arc<Mutex<Shared>>, repaint: &egui::Context) -> Response {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return Response::json(400, json!({"error": "invalid JSON"}));
    };
    let (Some(from), Some(to)) = (
        value.get("from").and_then(point),
        value.get("to").and_then(point),
    ) else {
        return Response::json(400, json!({"error": "from and to are required"}));
    };
    let button = pointer_button(&value);
    let steps = value
        .get("steps")
        .and_then(Value::as_u64)
        .unwrap_or(8)
        .clamp(1, 64);
    let mut frames = vec![
        vec![egui::Event::PointerMoved(from)],
        vec![egui::Event::PointerButton {
            pos: from,
            button,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }],
    ];
    for step in 1..=steps {
        let factor = step as f32 / steps as f32;
        frames.push(vec![egui::Event::PointerMoved(from.lerp(to, factor))]);
    }
    frames.push(vec![egui::Event::PointerButton {
        pos: to,
        button,
        pressed: false,
        modifiers: egui::Modifiers::NONE,
    }]);
    queue(shared, frames, repaint);
    Response::json(202, json!({"phase": "queued"}))
}

fn input_scroll(body: &[u8], shared: &Arc<Mutex<Shared>>, repaint: &egui::Context) -> Response {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return Response::json(400, json!({"error": "invalid JSON"}));
    };
    let Some(pos) = point(&value) else {
        return Response::json(400, json!({"error": "x and y are required"}));
    };
    let delta = egui::vec2(
        value.get("delta_x").and_then(Value::as_f64).unwrap_or(0.0) as f32,
        value.get("delta_y").and_then(Value::as_f64).unwrap_or(0.0) as f32,
    );
    let command = value
        .get("modifiers")
        .and_then(|modifiers| modifiers.get("command"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    queue(
        shared,
        vec![vec![
            egui::Event::PointerMoved(pos),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta,
                modifiers: egui::Modifiers {
                    command,
                    ctrl: command,
                    ..egui::Modifiers::NONE
                },
            },
        ]],
        repaint,
    );
    Response::json(202, json!({"phase": "queued"}))
}

fn queue(shared: &Arc<Mutex<Shared>>, frames: Vec<Vec<egui::Event>>, repaint: &egui::Context) {
    if let Ok(mut state) = shared.lock() {
        state.input_frames.extend(frames);
    }
    repaint.request_repaint();
}

fn encode_png(image: &egui::ColorImage) -> Option<Vec<u8>> {
    let mut bytes = Vec::with_capacity(image.pixels.len() * 4);
    for pixel in &image.pixels {
        bytes.extend_from_slice(&pixel.to_array());
    }
    let buffer = image::RgbaImage::from_raw(image.width() as u32, image.height() as u32, bytes)?;
    let mut png = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(buffer)
        .write_to(&mut png, image::ImageFormat::Png)
        .ok()?;
    Some(png.into_inner())
}

struct Response {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
}

impl Response {
    fn json(status: u16, value: Value) -> Self {
        Self {
            status,
            content_type: "application/json",
            body: serde_json::to_vec(&value).unwrap_or_default(),
        }
    }

    fn png(body: Vec<u8>) -> Self {
        Self {
            status: 200,
            content_type: "image/png",
            body,
        }
    }
}

fn write_response(stream: &mut TcpStream, response: Response) -> std::io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        202 => "Accepted",
        400 => "Bad Request",
        404 => "Not Found",
        409 => "Conflict",
        _ => "Error",
    };
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\nAccess-Control-Allow-Origin: *\r\n\r\n",
        response.status,
        reason,
        response.content_type,
        response.body.len()
    )?;
    stream.write_all(&response.body)
}
