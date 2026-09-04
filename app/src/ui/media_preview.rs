//! Asynchronous, color-managed Asset frames shared by editor surfaces.
//!
//! Decoding remains in the production loader and `CacheManager`. This service
//! only deduplicates background requests and owns their shared egui textures.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::{mpsc, Arc};

use library::cache::CacheManager;
use library::editor::load_asset_preview_frame;
use library::model::asset::{Asset, AssetKind};
use library::model::authoring::AuthoringProject;
use library::plugin::PluginManager;

const MAX_CONCURRENT_FRAME_REQUESTS: usize = 2;
const MAX_COMPLETED_FRAMES: usize = 16;
const MAX_RESIDENT_TEXTURES: usize = 128;

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct MediaPreviewKey {
    asset_id: uuid::Uuid,
    path: String,
    stream_index: Option<usize>,
    imported_content_sha256: Option<String>,
    source_color_fingerprint: String,
    source_time_bits: u64,
    requested_size: [u32; 2],
}

impl MediaPreviewKey {
    fn new(asset: &Asset, source_time: f64, evaluation_fps: f64, requested_size: [u32; 2]) -> Self {
        let source_time = canonical_source_time(asset, source_time, evaluation_fps);
        Self {
            asset_id: asset.id,
            path: asset.path.clone(),
            stream_index: asset.stream_index,
            imported_content_sha256: asset.imported_content_sha256().map(str::to_owned),
            source_color_fingerprint: serde_json::to_string(&asset.source_color)
                .unwrap_or_else(|_| format!("{:?}", asset.source_color)),
            source_time_bits: source_time.to_bits(),
            requested_size,
        }
    }

    fn source_time(&self) -> f64 {
        f64::from_bits(self.source_time_bits)
    }

    fn source_key(&self) -> MediaPreviewSourceKey {
        MediaPreviewSourceKey {
            asset_id: self.asset_id,
            path: self.path.clone(),
            stream_index: self.stream_index,
            imported_content_sha256: self.imported_content_sha256.clone(),
            source_color_fingerprint: self.source_color_fingerprint.clone(),
            source_time_bits: self.source_time_bits,
        }
    }
}

/// Identity of a decoded source frame, independent of the surface size asking
/// for it. This lets a resize keep painting the last resident texture while a
/// size-specific request is pending.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct MediaPreviewSourceKey {
    asset_id: uuid::Uuid,
    path: String,
    stream_index: Option<usize>,
    imported_content_sha256: Option<String>,
    source_color_fingerprint: String,
    source_time_bits: u64,
}

/// Converts a logical UI extent into a bounded, quantized texture request.
/// Quantizing avoids cache churn from sub-pixel layout jitter while still
/// separating materially different thumbnail sizes.
pub(crate) fn preview_request_size(context: &egui::Context, logical_size: egui::Vec2) -> [u32; 2] {
    let pixels_per_point = context.pixels_per_point().max(0.01);
    [logical_size.x, logical_size.y].map(|points| {
        let pixels = (points.max(1.0) * pixels_per_point).ceil() as u32;
        pixels.clamp(8, 2_048).div_ceil(8) * 8
    })
}

/// Chooses the stable representative frame used by non-timeline Asset cards.
/// Timeline clips provide their own source-mapped time instead.
pub(crate) fn representative_source_time(asset: &Asset) -> f64 {
    if !matches!(asset.kind, AssetKind::Video) {
        return 0.0;
    }
    asset
        .duration
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .map(|duration| (duration * 0.1).min(1.0))
        .unwrap_or(0.0)
}

fn canonical_source_time(asset: &Asset, source_time: f64, evaluation_fps: f64) -> f64 {
    if asset.kind != AssetKind::Video {
        return 0.0;
    }
    let fps = asset
        .fps
        .filter(|fps| fps.is_finite() && *fps > 0.0)
        .or_else(|| (evaluation_fps.is_finite() && evaluation_fps > 0.0).then_some(evaluation_fps))
        .unwrap_or(30.0);
    let mut frame = asset
        .source_frame_number_at(source_time.max(0.0), evaluation_fps)
        .unwrap_or_default();
    if let Some(frame_count) = asset.frame_count.filter(|count| *count > 0) {
        frame = frame.min(frame_count - 1);
    }
    frame as f64 / fps
}

struct CompletedFrame {
    key: MediaPreviewKey,
    result: Result<library::Image, String>,
}

#[derive(Clone, Default)]
pub(crate) struct MediaPreviewFrame {
    pub texture: Option<egui::TextureHandle>,
    pub texture_size: Option<[u32; 2]>,
    pub requested_size: Option<[u32; 2]>,
    pub content_hash: Option<String>,
    pub error: Option<String>,
    pub pending: bool,
    pub fallback: bool,
}

pub(crate) struct AuthoringMediaPreviewService {
    plugins: Arc<PluginManager>,
    cache: Arc<CacheManager>,
    sender: mpsc::Sender<CompletedFrame>,
    receiver: mpsc::Receiver<CompletedFrame>,
    pending: HashSet<MediaPreviewKey>,
    completed: HashMap<MediaPreviewKey, Result<library::Image, String>>,
    resident: HashMap<MediaPreviewKey, MediaPreviewFrame>,
    resident_order: VecDeque<MediaPreviewKey>,
    last_ready_source: HashMap<MediaPreviewSourceKey, MediaPreviewKey>,
}

impl AuthoringMediaPreviewService {
    pub(crate) fn new(plugins: Arc<PluginManager>, cache: Arc<CacheManager>) -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            plugins,
            cache,
            sender,
            receiver,
            pending: HashSet::new(),
            completed: HashMap::new(),
            resident: HashMap::new(),
            resident_order: VecDeque::new(),
            last_ready_source: HashMap::new(),
        }
    }

    /// Poll a cached preview or schedule its canonical loader on a background
    /// worker. Texture upload happens here on the UI thread; media decode does
    /// not.
    pub(crate) fn request(
        &mut self,
        context: &egui::Context,
        project: Arc<AuthoringProject>,
        asset: &Asset,
        source_time: f64,
        evaluation_fps: f64,
        requested_size: [u32; 2],
    ) -> MediaPreviewFrame {
        self.collect_completed();
        let key = MediaPreviewKey::new(asset, source_time, evaluation_fps, requested_size);
        if let Some(frame) = self.resident.get(&key).cloned() {
            self.touch_resident(&key);
            return frame;
        }
        if let Some(result) = self.completed.remove(&key) {
            let frame = match result {
                Ok(image) => {
                    let size = [image.width as usize, image.height as usize];
                    let content_hash = image_content_hash(&image);
                    let texture = context.load_texture(
                        format!(
                            "authoring.media_preview:{}:{:016x}:{}x{}",
                            key.asset_id,
                            key.source_time_bits,
                            key.requested_size[0],
                            key.requested_size[1]
                        ),
                        egui::ColorImage::from_rgba_unmultiplied(size, &image.data),
                        egui::TextureOptions::LINEAR,
                    );
                    MediaPreviewFrame {
                        texture: Some(texture),
                        texture_size: Some([image.width, image.height]),
                        requested_size: Some(key.requested_size),
                        content_hash: Some(content_hash),
                        error: None,
                        pending: false,
                        fallback: false,
                    }
                }
                Err(error) => MediaPreviewFrame {
                    requested_size: Some(key.requested_size),
                    error: Some(error),
                    ..MediaPreviewFrame::default()
                },
            };
            self.insert_resident(key, frame.clone());
            return frame;
        }
        if !self.pending.contains(&key) && self.pending.len() < MAX_CONCURRENT_FRAME_REQUESTS {
            self.pending.insert(key.clone());
            let plugins = Arc::clone(&self.plugins);
            let cache = Arc::clone(&self.cache);
            let sender = self.sender.clone();
            let worker_key = key.clone();
            std::thread::spawn(move || {
                let result = load_asset_preview_frame(
                    &project,
                    worker_key.asset_id,
                    worker_key.source_time(),
                    &plugins,
                    &cache,
                )
                .map_err(|error| error.to_string());
                drop(sender.send(CompletedFrame {
                    key: worker_key,
                    result,
                }));
            });
        }
        context.request_repaint_after(std::time::Duration::from_millis(16));
        self.pending_frame(&key)
    }

    fn collect_completed(&mut self) {
        while let Ok(completed) = self.receiver.try_recv() {
            self.pending.remove(&completed.key);
            if self.completed.len() >= MAX_COMPLETED_FRAMES {
                if let Some(oldest) = self.completed.keys().next().cloned() {
                    self.completed.remove(&oldest);
                }
            }
            self.completed.insert(completed.key, completed.result);
        }
    }

    fn pending_frame(&mut self, key: &MediaPreviewKey) -> MediaPreviewFrame {
        let fallback_key = self.last_ready_source.get(&key.source_key()).cloned();
        if let Some(fallback_key) = fallback_key {
            if let Some(mut frame) = self.resident.get(&fallback_key).cloned() {
                self.touch_resident(&fallback_key);
                frame.requested_size = Some(key.requested_size);
                frame.pending = true;
                frame.fallback = true;
                return frame;
            }
            self.last_ready_source.remove(&key.source_key());
        }

        MediaPreviewFrame {
            requested_size: Some(key.requested_size),
            pending: true,
            ..MediaPreviewFrame::default()
        }
    }

    fn insert_resident(&mut self, key: MediaPreviewKey, frame: MediaPreviewFrame) {
        while self.resident.len() >= MAX_RESIDENT_TEXTURES {
            let Some(oldest) = self.resident_order.pop_front() else {
                break;
            };
            if self.resident.remove(&oldest).is_some() {
                self.last_ready_source
                    .retain(|_, resident_key| resident_key != &oldest);
            }
        }
        self.resident_order.retain(|candidate| candidate != &key);
        self.resident_order.push_back(key.clone());
        if frame.texture.is_some() {
            self.last_ready_source.insert(key.source_key(), key.clone());
        }
        self.resident.insert(key, frame);
    }

    fn touch_resident(&mut self, key: &MediaPreviewKey) {
        self.resident_order.retain(|candidate| candidate != key);
        self.resident_order.push_back(key.clone());
    }
}

fn image_content_hash(image: &library::Image) -> String {
    // Fixed FNV-1a parameters make QA identity stable across processes and do
    // not rely on Rust's randomized HashMap hasher.
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in image
        .width
        .to_le_bytes()
        .into_iter()
        .chain(image.height.to_le_bytes())
        .chain(image.data.iter().copied())
    {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use library::model::asset::{Asset, AssetKind};

    use super::{preview_request_size, representative_source_time, MediaPreviewKey};

    #[test]
    fn request_key_separates_video_strip_times_but_reuses_still_time() {
        let asset = Asset::new("video", "video.mp4", AssetKind::Video);
        assert_ne!(
            MediaPreviewKey::new(&asset, 0.0, 30.0, [160, 96]),
            MediaPreviewKey::new(&asset, 1.0, 30.0, [160, 96])
        );
        assert_eq!(
            MediaPreviewKey::new(&asset, 0.0, 30.0, [160, 96]),
            MediaPreviewKey::new(&asset, 0.0, 30.0, [160, 96])
        );
    }

    #[test]
    fn video_requests_within_one_frame_share_one_texture_key() {
        let mut asset = Asset::new("video", "video.mp4", AssetKind::Video);
        asset.fps = Some(25.0);
        assert_eq!(
            MediaPreviewKey::new(&asset, 1.001, 30.0, [160, 96]),
            MediaPreviewKey::new(&asset, 1.039, 30.0, [160, 96])
        );
    }

    #[test]
    fn request_key_separates_sizes_but_source_identity_allows_resize_fallback() {
        let asset = Asset::new("image", "image.png", AssetKind::Image);
        let small = MediaPreviewKey::new(&asset, 0.0, 30.0, [160, 96]);
        let large = MediaPreviewKey::new(&asset, 0.0, 30.0, [320, 192]);
        assert_ne!(small, large);
        assert_eq!(small.source_key(), large.source_key());
    }

    #[test]
    fn logical_request_size_is_quantized_against_subpixel_jitter() {
        let context = egui::Context::default();
        assert_eq!(
            preview_request_size(&context, egui::vec2(100.01, 50.01)),
            preview_request_size(&context, egui::vec2(100.02, 50.02))
        );
    }

    #[test]
    fn representative_video_time_is_bounded_and_stills_use_zero() {
        let image = Asset::new("image", "image.png", AssetKind::Image);
        assert_eq!(representative_source_time(&image), 0.0);
        let mut video = Asset::new("video", "video.mp4", AssetKind::Video);
        video.duration = Some(5.0);
        assert_eq!(representative_source_time(&video), 0.5);
        video.duration = Some(120.0);
        assert_eq!(representative_source_time(&video), 1.0);
    }
}
