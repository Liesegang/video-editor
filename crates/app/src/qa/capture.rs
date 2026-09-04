use image::codecs::png::{CompressionType, FilterType, PngEncoder};
use image::ImageEncoder;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const CAPTURE_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_ACTIVE_CAPTURES: usize = 16;
const MAX_CAPTURE_RECORDS: usize = 64;
const MAX_RGBA_BYTES: usize = 128 * 1024 * 1024;
const MAX_PNG_BYTES: usize = 64 * 1024 * 1024;
const MAX_RETAINED_PNG_BYTES: usize = 128 * 1024 * 1024;

static NEXT_RUNTIME_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CapturePhase {
    Queued,
    Capturing,
    Ready,
    Failed,
}

impl CapturePhase {
    fn is_terminal(self) -> bool {
        matches!(self, Self::Ready | Self::Failed)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CaptureMetadata {
    pub capture_id: u64,
    pub phase: CapturePhase,
    pub requested_frame: u64,
    pub completed_frame: Option<u64>,
    pub viewport: Option<String>,
    pub pixels_per_point: Option<f32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub sha256: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Copy)]
struct CaptureLimits {
    timeout: Duration,
    max_active: usize,
    max_records: usize,
    max_rgba_bytes: usize,
    max_png_bytes: usize,
    max_retained_png_bytes: usize,
}

impl Default for CaptureLimits {
    fn default() -> Self {
        Self {
            timeout: CAPTURE_TIMEOUT,
            max_active: MAX_ACTIVE_CAPTURES,
            max_records: MAX_CAPTURE_RECORDS,
            max_rgba_bytes: MAX_RGBA_BYTES,
            max_png_bytes: MAX_PNG_BYTES,
            max_retained_png_bytes: MAX_RETAINED_PNG_BYTES,
        }
    }
}

struct CaptureRecord {
    metadata: CaptureMetadata,
    requested_at: Instant,
    png: Option<Arc<[u8]>>,
}

struct CaptureState {
    next_id: u64,
    records: BTreeMap<u64, CaptureRecord>,
    order: VecDeque<u64>,
    queue: VecDeque<u64>,
    active: Option<u64>,
    retained_png_bytes: usize,
}

impl Default for CaptureState {
    fn default() -> Self {
        Self {
            next_id: 1,
            records: BTreeMap::new(),
            order: VecDeque::new(),
            queue: VecDeque::new(),
            active: None,
            retained_png_bytes: 0,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CaptureToken {
    runtime_id: u64,
    capture_id: u64,
}

pub(super) struct CaptureCommand {
    pub user_data: egui::UserData,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) enum CaptureRequestError {
    QueueFull,
    Unavailable,
}

impl fmt::Display for CaptureRequestError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::QueueFull => formatter.write_str("capture queue is full"),
            Self::Unavailable => formatter.write_str("capture state is unavailable"),
        }
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub(super) enum CaptureLookup {
    Ready(Arc<[u8]>),
    Pending(CapturePhase),
    Failed(String),
    NotFound,
}

#[derive(Debug, Clone, Eq, PartialEq)]
enum CaptureEventOutcome {
    Ignored,
    Ready(u64),
    Failed(u64, String),
    Rejected(String),
}

/// Shared state between the loopback HTTP thread and the egui UI thread.
///
/// It keeps the HTTP side model-free: only eframe's actual screenshot command
/// and reply event can transition a capture to `ready`.
pub(super) struct CaptureStore {
    runtime_id: u64,
    limits: CaptureLimits,
    state: Mutex<CaptureState>,
}

impl Default for CaptureStore {
    fn default() -> Self {
        Self::new(CaptureLimits::default())
    }
}

impl CaptureStore {
    fn new(limits: CaptureLimits) -> Self {
        Self {
            runtime_id: NEXT_RUNTIME_ID.fetch_add(1, Ordering::Relaxed),
            limits,
            state: Mutex::new(CaptureState::default()),
        }
    }

    pub fn request(&self, requested_frame: u64) -> Result<CaptureMetadata, CaptureRequestError> {
        self.request_at(requested_frame, Instant::now())
    }

    fn request_at(
        &self,
        requested_frame: u64,
        now: Instant,
    ) -> Result<CaptureMetadata, CaptureRequestError> {
        let mut state = self.state.lock().map_err(|error| {
            log::error!("QA capture state lock poisoned while queueing: {error}");
            CaptureRequestError::Unavailable
        })?;
        self.expire_locked(&mut state, now, requested_frame);

        let active_count = state.queue.len() + usize::from(state.active.is_some());
        if active_count >= self.limits.max_active {
            return Err(CaptureRequestError::QueueFull);
        }
        while state.records.len() >= self.limits.max_records {
            if !evict_oldest_terminal(&mut state) {
                return Err(CaptureRequestError::QueueFull);
            }
        }

        let capture_id = state.next_id;
        state.next_id = state
            .next_id
            .checked_add(1)
            .ok_or(CaptureRequestError::QueueFull)?;
        let metadata = CaptureMetadata {
            capture_id,
            phase: CapturePhase::Queued,
            requested_frame,
            completed_frame: None,
            viewport: None,
            pixels_per_point: None,
            width: None,
            height: None,
            sha256: None,
            error: None,
        };
        state.records.insert(
            capture_id,
            CaptureRecord {
                metadata: metadata.clone(),
                requested_at: now,
                png: None,
            },
        );
        state.order.push_back(capture_id);
        state.queue.push_back(capture_id);
        drop(state);
        Ok(metadata)
    }

    pub fn metadata(
        &self,
        capture_id: u64,
        current_frame: u64,
    ) -> Result<Option<CaptureMetadata>, CaptureRequestError> {
        let mut state = self.state.lock().map_err(|error| {
            log::error!("QA capture state lock poisoned while reading metadata: {error}");
            CaptureRequestError::Unavailable
        })?;
        self.expire_locked(&mut state, Instant::now(), current_frame);
        let metadata = state
            .records
            .get(&capture_id)
            .map(|record| record.metadata.clone());
        drop(state);
        Ok(metadata)
    }

    pub fn png(
        &self,
        capture_id: u64,
        current_frame: u64,
    ) -> Result<CaptureLookup, CaptureRequestError> {
        let mut state = self.state.lock().map_err(|error| {
            log::error!("QA capture state lock poisoned while reading PNG: {error}");
            CaptureRequestError::Unavailable
        })?;
        self.expire_locked(&mut state, Instant::now(), current_frame);
        let Some(record) = state.records.get(&capture_id) else {
            return Ok(CaptureLookup::NotFound);
        };
        let lookup = match record.metadata.phase {
            CapturePhase::Ready => record.png.as_ref().map_or_else(
                || CaptureLookup::Failed("capture PNG is unavailable".to_string()),
                |png| CaptureLookup::Ready(Arc::clone(png)),
            ),
            CapturePhase::Failed => CaptureLookup::Failed(
                record
                    .metadata
                    .error
                    .clone()
                    .unwrap_or_else(|| "capture failed".to_string()),
            ),
            phase => CaptureLookup::Pending(phase),
        };
        drop(state);
        Ok(lookup)
    }

    pub fn receive_events(&self, raw_input: &egui::RawInput, completed_frame: u64) {
        for event in &raw_input.events {
            let egui::Event::Screenshot {
                viewport_id,
                user_data,
                image,
            } = event
            else {
                continue;
            };
            match self.handle_event_at(
                user_data,
                *viewport_id,
                image,
                completed_frame,
                Instant::now(),
            ) {
                CaptureEventOutcome::Ignored | CaptureEventOutcome::Ready(_) => {}
                CaptureEventOutcome::Failed(capture_id, error) => {
                    log::warn!("QA capture {capture_id} failed: {error}");
                }
                CaptureEventOutcome::Rejected(error) => {
                    log::warn!("Rejected stale or mismatched QA screenshot event: {error}");
                }
            }
        }
        self.expire(Instant::now(), completed_frame);
    }

    pub fn issue_for_frame(&self, context: &egui::Context, current_frame: u64) {
        let viewport_id = context.viewport_id();
        let pixels_per_point = context.pixels_per_point();
        match self.begin_next_at(viewport_id, pixels_per_point, current_frame, Instant::now()) {
            Ok(Some(command)) => {
                context.send_viewport_cmd(egui::ViewportCommand::Screenshot(command.user_data))
            }
            Ok(None) => {}
            Err(error) => log::warn!("QA capture scheduling failed: {error}"),
        }
    }

    fn begin_next_at(
        &self,
        viewport_id: egui::ViewportId,
        pixels_per_point: f32,
        current_frame: u64,
        now: Instant,
    ) -> Result<Option<CaptureCommand>, CaptureRequestError> {
        let mut state = self.state.lock().map_err(|error| {
            log::error!("QA capture state lock poisoned while scheduling: {error}");
            CaptureRequestError::Unavailable
        })?;
        self.expire_locked(&mut state, now, current_frame);
        if state.active.is_some() {
            drop(state);
            return Ok(None);
        }

        while let Some(capture_id) = state.queue.pop_front() {
            let Some(record) = state.records.get_mut(&capture_id) else {
                continue;
            };
            if record.metadata.phase != CapturePhase::Queued {
                continue;
            }
            record.metadata.phase = CapturePhase::Capturing;
            record.metadata.viewport = Some(viewport_name(viewport_id));
            record.metadata.pixels_per_point = Some(pixels_per_point);
            state.active = Some(capture_id);
            let token = CaptureToken {
                runtime_id: self.runtime_id,
                capture_id,
            };
            let command = CaptureCommand {
                user_data: egui::UserData::new(token),
            };
            drop(state);
            return Ok(Some(command));
        }
        drop(state);
        Ok(None)
    }

    fn handle_event_at(
        &self,
        user_data: &egui::UserData,
        viewport_id: egui::ViewportId,
        image: &egui::ColorImage,
        completed_frame: u64,
        now: Instant,
    ) -> CaptureEventOutcome {
        let Some(token) = user_data
            .data
            .as_deref()
            .and_then(|data| data.downcast_ref::<CaptureToken>())
        else {
            return CaptureEventOutcome::Ignored;
        };
        if token.runtime_id != self.runtime_id {
            return CaptureEventOutcome::Rejected("capture runtime does not match".to_string());
        }

        let validation = self.validate_event(token, viewport_id, completed_frame, now);
        if let Err(outcome) = validation {
            return outcome;
        }

        let encoded = encode_png(image, self.limits);
        let mut state = match self.state.lock() {
            Ok(state) => state,
            Err(_) => {
                return CaptureEventOutcome::Rejected(
                    "capture state is unavailable while completing event".to_string(),
                );
            }
        };
        self.expire_locked(&mut state, now, completed_frame);
        if state.active != Some(token.capture_id)
            || state
                .records
                .get(&token.capture_id)
                .is_none_or(|record| record.metadata.phase != CapturePhase::Capturing)
        {
            drop(state);
            return CaptureEventOutcome::Rejected(
                "capture is no longer awaiting this screenshot".to_string(),
            );
        }

        let outcome = match encoded {
            Ok(encoded) => {
                while state.retained_png_bytes.saturating_add(encoded.png.len())
                    > self.limits.max_retained_png_bytes
                {
                    if !evict_oldest_terminal(&mut state) {
                        let error = "capture PNG retention limit exceeded".to_string();
                        fail_capture(
                            &mut state,
                            token.capture_id,
                            completed_frame,
                            Some(encoded.width),
                            Some(encoded.height),
                            error.clone(),
                        );
                        drop(state);
                        return CaptureEventOutcome::Failed(token.capture_id, error);
                    }
                }

                let png: Arc<[u8]> = encoded.png.into();
                state.retained_png_bytes = state.retained_png_bytes.saturating_add(png.len());
                if let Some(record) = state.records.get_mut(&token.capture_id) {
                    record.metadata.phase = CapturePhase::Ready;
                    record.metadata.completed_frame = Some(completed_frame);
                    record.metadata.width = Some(encoded.width);
                    record.metadata.height = Some(encoded.height);
                    record.metadata.sha256 = Some(encoded.sha256);
                    record.metadata.error = None;
                    record.png = Some(png);
                }
                state.active = None;
                CaptureEventOutcome::Ready(token.capture_id)
            }
            Err(error) => {
                fail_capture(
                    &mut state,
                    token.capture_id,
                    completed_frame,
                    u32::try_from(image.size[0]).ok(),
                    u32::try_from(image.size[1]).ok(),
                    error.clone(),
                );
                CaptureEventOutcome::Failed(token.capture_id, error)
            }
        };
        drop(state);
        outcome
    }

    fn validate_event(
        &self,
        token: &CaptureToken,
        viewport_id: egui::ViewportId,
        completed_frame: u64,
        now: Instant,
    ) -> Result<(), CaptureEventOutcome> {
        let mut state = self.state.lock().map_err(|error| {
            CaptureEventOutcome::Rejected(format!(
                "capture state is unavailable while validating event: {error}"
            ))
        })?;
        self.expire_locked(&mut state, now, completed_frame);
        if state.active != Some(token.capture_id) {
            return Err(CaptureEventOutcome::Rejected(
                "capture id is stale or not active".to_string(),
            ));
        }
        let Some(record) = state.records.get(&token.capture_id) else {
            return Err(CaptureEventOutcome::Rejected(
                "capture id is not retained".to_string(),
            ));
        };
        if record.metadata.phase != CapturePhase::Capturing {
            return Err(CaptureEventOutcome::Rejected(
                "capture is not in the capturing phase".to_string(),
            ));
        }
        let actual_viewport = viewport_name(viewport_id);
        if record.metadata.viewport.as_deref() != Some(actual_viewport.as_str()) {
            let error = format!(
                "screenshot viewport mismatch: expected {:?}, got {actual_viewport:?}",
                record.metadata.viewport
            );
            fail_capture(
                &mut state,
                token.capture_id,
                completed_frame,
                None,
                None,
                error.clone(),
            );
            return Err(CaptureEventOutcome::Failed(token.capture_id, error));
        }
        drop(state);
        Ok(())
    }

    fn expire(&self, now: Instant, current_frame: u64) {
        if let Ok(mut state) = self.state.lock() {
            self.expire_locked(&mut state, now, current_frame);
        }
    }

    fn expire_locked(&self, state: &mut CaptureState, now: Instant, current_frame: u64) {
        let expired = state
            .records
            .iter()
            .filter_map(|(capture_id, record)| {
                let elapsed = now
                    .checked_duration_since(record.requested_at)
                    .unwrap_or_default();
                (!record.metadata.phase.is_terminal() && elapsed >= self.limits.timeout)
                    .then_some(*capture_id)
            })
            .collect::<Vec<_>>();
        for capture_id in expired {
            fail_capture(
                state,
                capture_id,
                current_frame,
                None,
                None,
                format!(
                    "capture timed out after {} ms",
                    self.limits.timeout.as_millis()
                ),
            );
        }
    }
}

struct EncodedCapture {
    png: Vec<u8>,
    width: u32,
    height: u32,
    sha256: String,
}

fn encode_png(image: &egui::ColorImage, limits: CaptureLimits) -> Result<EncodedCapture, String> {
    let width = u32::try_from(image.size[0])
        .map_err(|error| format!("screenshot width cannot be represented as u32: {error}"))?;
    let height = u32::try_from(image.size[1])
        .map_err(|error| format!("screenshot height cannot be represented as u32: {error}"))?;
    if width == 0 || height == 0 {
        return Err("screenshot has zero width or height".to_string());
    }
    let pixel_count = image.size[0]
        .checked_mul(image.size[1])
        .ok_or_else(|| "screenshot dimensions overflow".to_string())?;
    if pixel_count != image.pixels.len() {
        return Err(format!(
            "screenshot pixel count mismatch: expected {pixel_count}, got {}",
            image.pixels.len()
        ));
    }
    let rgba_bytes = pixel_count
        .checked_mul(4)
        .ok_or_else(|| "screenshot byte count overflow".to_string())?;
    if rgba_bytes > limits.max_rgba_bytes {
        return Err(format!(
            "screenshot RGBA data exceeds {} bytes",
            limits.max_rgba_bytes
        ));
    }

    let mut rgba = Vec::with_capacity(rgba_bytes);
    for pixel in &image.pixels {
        rgba.extend_from_slice(&pixel.to_srgba_unmultiplied());
    }
    let mut png = Vec::new();
    PngEncoder::new_with_quality(&mut png, CompressionType::Fast, FilterType::NoFilter)
        .write_image(&rgba, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|error| format!("failed to encode screenshot as PNG: {error}"))?;
    if png.len() > limits.max_png_bytes {
        return Err(format!(
            "encoded screenshot exceeds {} bytes",
            limits.max_png_bytes
        ));
    }
    let sha256 = format!("{:x}", Sha256::digest(&png));
    Ok(EncodedCapture {
        png,
        width,
        height,
        sha256,
    })
}

fn fail_capture(
    state: &mut CaptureState,
    capture_id: u64,
    completed_frame: u64,
    width: Option<u32>,
    height: Option<u32>,
    error: String,
) {
    if let Some(record) = state.records.get_mut(&capture_id) {
        if let Some(png) = record.png.take() {
            state.retained_png_bytes = state.retained_png_bytes.saturating_sub(png.len());
        }
        record.metadata.phase = CapturePhase::Failed;
        record.metadata.completed_frame = Some(completed_frame);
        record.metadata.width = width;
        record.metadata.height = height;
        record.metadata.sha256 = None;
        record.metadata.error = Some(error);
    }
    if state.active == Some(capture_id) {
        state.active = None;
    }
    state.queue.retain(|queued_id| *queued_id != capture_id);
}

fn evict_oldest_terminal(state: &mut CaptureState) -> bool {
    let Some(index) = state.order.iter().position(|capture_id| {
        state
            .records
            .get(capture_id)
            .is_some_and(|record| record.metadata.phase.is_terminal())
    }) else {
        return false;
    };
    let Some(capture_id) = state.order.remove(index) else {
        return false;
    };
    state.queue.retain(|queued_id| *queued_id != capture_id);
    if let Some(record) = state.records.remove(&capture_id) {
        if let Some(png) = record.png {
            state.retained_png_bytes = state.retained_png_bytes.saturating_sub(png.len());
        }
    }
    true
}

fn viewport_name(viewport_id: egui::ViewportId) -> String {
    if viewport_id == egui::ViewportId::ROOT {
        "root".to_string()
    } else {
        format!("{viewport_id:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command_user_data(command: CaptureCommand) -> egui::UserData {
        command.user_data
    }

    #[test]
    fn real_viewport_command_and_matching_event_produce_png_metadata() {
        let store = CaptureStore::default();
        let now = Instant::now();
        let requested = store.request_at(7, now).unwrap();
        let context = egui::Context::default();
        let mut command = None;
        let output = context.run(egui::RawInput::default(), |context| {
            command = store
                .begin_next_at(egui::ViewportId::ROOT, 2.0, 8, now)
                .unwrap();
            if let Some(command) = command.as_ref() {
                context.send_viewport_cmd(egui::ViewportCommand::Screenshot(
                    command.user_data.clone(),
                ));
            }
        });
        assert!(output.viewport_output.values().any(|output| {
            output
                .commands
                .iter()
                .any(|command| matches!(command, egui::ViewportCommand::Screenshot(_)))
        }));

        let image = egui::ColorImage::filled([2, 1], egui::Color32::from_rgb(10, 20, 30));
        let outcome = store.handle_event_at(
            &command_user_data(command.unwrap()),
            egui::ViewportId::ROOT,
            &image,
            8,
            now,
        );
        assert_eq!(outcome, CaptureEventOutcome::Ready(requested.capture_id));

        let metadata = store.metadata(requested.capture_id, 8).unwrap().unwrap();
        assert_eq!(metadata.phase, CapturePhase::Ready);
        assert_eq!(metadata.requested_frame, 7);
        assert_eq!(metadata.completed_frame, Some(8));
        assert_eq!(metadata.viewport.as_deref(), Some("root"));
        assert_eq!(metadata.pixels_per_point, Some(2.0));
        assert_eq!((metadata.width, metadata.height), (Some(2), Some(1)));

        let CaptureLookup::Ready(png) = store.png(requested.capture_id, 8).unwrap() else {
            panic!("capture should retain ready PNG bytes");
        };
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_eq!(
            metadata.sha256.as_deref(),
            Some(format!("{:x}", Sha256::digest(&png)).as_str())
        );
    }

    #[test]
    fn queues_concurrent_requests_and_schedules_one_per_frame() {
        let store = CaptureStore::default();
        let now = Instant::now();
        let first = store.request_at(3, now).unwrap();
        let second = store.request_at(3, now).unwrap();
        assert_ne!(first.capture_id, second.capture_id);

        let first_command = store
            .begin_next_at(egui::ViewportId::ROOT, 1.0, 4, now)
            .unwrap()
            .unwrap();
        assert!(store
            .begin_next_at(egui::ViewportId::ROOT, 1.0, 4, now)
            .unwrap()
            .is_none());
        let image = egui::ColorImage::filled([1, 1], egui::Color32::WHITE);
        assert_eq!(
            store.handle_event_at(
                &first_command.user_data,
                egui::ViewportId::ROOT,
                &image,
                4,
                now,
            ),
            CaptureEventOutcome::Ready(first.capture_id)
        );
        let second_command = store
            .begin_next_at(egui::ViewportId::ROOT, 1.0, 5, now)
            .unwrap()
            .unwrap();
        assert_eq!(
            store.handle_event_at(
                &second_command.user_data,
                egui::ViewportId::ROOT,
                &image,
                5,
                now,
            ),
            CaptureEventOutcome::Ready(second.capture_id)
        );
    }

    #[test]
    fn rejects_mismatched_and_stale_user_data() {
        let first_store = CaptureStore::default();
        let second_store = CaptureStore::default();
        let now = Instant::now();
        let request = first_store.request_at(1, now).unwrap();
        let command = first_store
            .begin_next_at(egui::ViewportId::ROOT, 1.0, 2, now)
            .unwrap()
            .unwrap();
        let image = egui::ColorImage::filled([1, 1], egui::Color32::WHITE);
        assert!(matches!(
            second_store.handle_event_at(
                &command.user_data,
                egui::ViewportId::ROOT,
                &image,
                2,
                now,
            ),
            CaptureEventOutcome::Rejected(_)
        ));
        assert_eq!(
            first_store
                .handle_event_at(&command.user_data, egui::ViewportId::ROOT, &image, 2, now,),
            CaptureEventOutcome::Ready(request.capture_id)
        );
        assert!(matches!(
            first_store
                .handle_event_at(&command.user_data, egui::ViewportId::ROOT, &image, 3, now,),
            CaptureEventOutcome::Rejected(_)
        ));
        assert_eq!(
            first_store.handle_event_at(
                &egui::UserData::new("another screenshot owner"),
                egui::ViewportId::ROOT,
                &image,
                3,
                now,
            ),
            CaptureEventOutcome::Ignored
        );
    }

    #[test]
    fn timeout_and_image_size_errors_become_failed_metadata() {
        let limits = CaptureLimits {
            timeout: Duration::from_millis(10),
            max_rgba_bytes: 4,
            ..CaptureLimits::default()
        };
        let store = CaptureStore::new(limits);
        let now = Instant::now();
        let timed_out = store.request_at(10, now).unwrap();
        store.expire(now + Duration::from_millis(10), 11);
        let metadata = store.metadata(timed_out.capture_id, 11).unwrap().unwrap();
        assert_eq!(metadata.phase, CapturePhase::Failed);
        assert!(metadata.error.unwrap().contains("timed out"));

        let oversized = store.request_at(12, now).unwrap();
        let command = store
            .begin_next_at(egui::ViewportId::ROOT, 1.0, 12, now)
            .unwrap()
            .unwrap();
        let image = egui::ColorImage::filled([2, 2], egui::Color32::WHITE);
        assert!(matches!(
            store.handle_event_at(
                &command.user_data,
                egui::ViewportId::ROOT,
                &image,
                12,
                now,
            ),
            CaptureEventOutcome::Failed(id, _) if id == oversized.capture_id
        ));
        let metadata = store.metadata(oversized.capture_id, 12).unwrap().unwrap();
        assert_eq!(metadata.phase, CapturePhase::Failed);
        assert!(metadata.error.unwrap().contains("RGBA data exceeds"));

        let png_limited_store = CaptureStore::new(CaptureLimits {
            max_png_bytes: 1,
            ..CaptureLimits::default()
        });
        let png_limited = png_limited_store.request_at(13, now).unwrap();
        let command = png_limited_store
            .begin_next_at(egui::ViewportId::ROOT, 1.0, 13, now)
            .unwrap()
            .unwrap();
        let image = egui::ColorImage::filled([1, 1], egui::Color32::WHITE);
        assert!(matches!(
            png_limited_store.handle_event_at(
                &command.user_data,
                egui::ViewportId::ROOT,
                &image,
                13,
                now,
            ),
            CaptureEventOutcome::Failed(id, _) if id == png_limited.capture_id
        ));
        let metadata = png_limited_store
            .metadata(png_limited.capture_id, 13)
            .unwrap()
            .unwrap();
        assert!(metadata
            .error
            .unwrap()
            .contains("encoded screenshot exceeds"));

        let retention_limited_store = CaptureStore::new(CaptureLimits {
            max_retained_png_bytes: 1,
            ..CaptureLimits::default()
        });
        let retention_limited = retention_limited_store.request_at(14, now).unwrap();
        let command = retention_limited_store
            .begin_next_at(egui::ViewportId::ROOT, 1.0, 14, now)
            .unwrap()
            .unwrap();
        assert!(matches!(
            retention_limited_store.handle_event_at(
                &command.user_data,
                egui::ViewportId::ROOT,
                &image,
                14,
                now,
            ),
            CaptureEventOutcome::Failed(id, _) if id == retention_limited.capture_id
        ));
        let metadata = retention_limited_store
            .metadata(retention_limited.capture_id, 14)
            .unwrap()
            .unwrap();
        assert!(metadata.error.unwrap().contains("retention limit"));
    }

    #[test]
    fn active_queue_and_terminal_retention_are_bounded() {
        let limits = CaptureLimits {
            max_active: 1,
            max_records: 1,
            ..CaptureLimits::default()
        };
        let store = CaptureStore::new(limits);
        let now = Instant::now();
        let first = store.request_at(1, now).unwrap();
        assert_eq!(
            store.request_at(1, now),
            Err(CaptureRequestError::QueueFull)
        );
        store.expire(now + limits.timeout, 2);
        let second = store.request_at(3, now + limits.timeout).unwrap();
        assert_ne!(first.capture_id, second.capture_id);
        assert!(store.metadata(first.capture_id, 3).unwrap().is_none());
    }
}
