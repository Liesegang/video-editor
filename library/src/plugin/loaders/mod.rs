pub mod ffmpeg_video;
pub mod native_image;

mod ffmpeg_color_metadata;

pub use self::ffmpeg_video::FfmpegVideoLoader;
pub use self::native_image::NativeImageLoader;

use crate::cache::CacheManager;
use crate::error::LibraryError;
use crate::model::asset::AssetKind;
use crate::model::asset::SourceColorDescription;
use crate::model::frame::Image;
use crate::plugin::{Plugin, PluginCategory};
use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Clone)]
pub enum LoadRequest {
    /// Load a static image.
    Image { path: String },
    /// Load a video frame.
    VideoFrame {
        path: String,
        /// Source-local seconds. This is the sole decode authority; the
        /// selected loader converts it into the stream's time-base/PTS.
        source_time: f64,
        stream_index: Option<usize>,
        input_color_space: Option<String>,
        output_color_space: Option<String>,
    },
}

impl LoadRequest {
    pub fn path(&self) -> &str {
        match self {
            LoadRequest::Image { path } => path,
            LoadRequest::VideoFrame { path, .. } => path,
        }
    }
}

#[derive(Debug)]
pub struct LoadResponse {
    pub image: Image,
}

/// A loader can either decline a request without error or report a real load
/// failure. This prevents decode errors from being misreported as a missing
/// plugin after the manager tries the next loader.
#[derive(Debug, Error)]
pub enum LoadPluginError {
    #[error("request is not supported by this loader")]
    Unsupported,
    #[error(transparent)]
    Failed(#[from] LibraryError),
}

pub type LoadPluginResult<T> = Result<T, LoadPluginError>;

#[derive(Debug, Clone)]
pub struct AssetMetadata {
    pub kind: AssetKind,
    pub duration: Option<f64>,
    pub fps: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub stream_index: Option<usize>,
    pub frame_count: Option<u64>,
    pub time_base: Option<(i32, i32)>,
    /// Color tags detected from this source stream. Empty fields remain
    /// unknown and must not be replaced with guessed defaults by the loader.
    pub source_color: SourceColorDescription,
}

pub trait LoadPlugin: Plugin {
    /// Probe a file and return metadata for all available streams.
    ///
    /// Probing must not require a video decoder: audio-only resources and
    /// metadata-only import paths must work independently from frame loading.
    /// Implementations may initialize their state lazily in [`Self::load`].
    /// Returns [`LoadPluginError::Unsupported`] when the plugin does not handle
    /// the file and [`LoadPluginError::Failed`] when it does but opening fails.
    fn open(&self, path: &str) -> LoadPluginResult<Vec<AssetMetadata>>;

    /// Load a frame from a file.
    /// The plugin uses internally cached reader if available.
    /// Unsupported request types must not be encoded as generic plugin errors.
    fn load(&self, request: &LoadRequest, cache: &CacheManager) -> LoadPluginResult<LoadResponse>;

    fn plugin_type(&self) -> PluginCategory {
        PluginCategory::Load
    }
}

#[derive(Default)]
pub struct LoadRepository {
    pub plugins: HashMap<String, Arc<dyn LoadPlugin>>,
    /// Plugin IDs in priority order (first = highest priority).
    priority_order: Vec<String>,
}

impl LoadRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, plugin: Arc<dyn LoadPlugin>) -> Option<Arc<dyn LoadPlugin>> {
        let id = plugin.id().to_string();
        if !self.priority_order.contains(&id) {
            self.priority_order.push(id.clone());
        }
        self.plugins.insert(id, plugin)
    }

    pub fn get(&self, id: &str) -> Option<&Arc<dyn LoadPlugin>> {
        self.plugins.get(id)
    }

    /// Set plugin priority order. IDs not in the list will be appended at the end.
    pub fn set_priority_order(&mut self, order: Vec<String>) {
        // Start with the given order, then append any missing plugins
        let mut new_order = order;
        for id in &self.priority_order {
            if !new_order.contains(id) {
                new_order.push(id.clone());
            }
        }
        self.priority_order = new_order;
    }

    /// Get priority order (for UI display).
    pub fn get_priority_order(&self) -> &[String] {
        &self.priority_order
    }

    /// Iterate plugins in priority order.
    pub fn values_by_priority(&self) -> impl Iterator<Item = &Arc<dyn LoadPlugin>> {
        self.priority_order
            .iter()
            .filter_map(|id| self.plugins.get(id))
    }

    pub fn values(&self) -> impl Iterator<Item = &Arc<dyn LoadPlugin>> {
        self.values_by_priority()
    }
}
