pub mod ffmpeg_video;
pub mod native_image;

pub use self::ffmpeg_video::FfmpegVideoLoader;
pub use self::native_image::NativeImageLoader;

use crate::cache::CacheManager;
use crate::error::LibraryError;
use crate::model::frame::Image;
use crate::model::project::asset::AssetKind;
use crate::plugin::{Plugin, PluginCategory};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum LoadRequest {
    /// Load a static image.
    Image { path: String },
    /// Load a video frame.
    VideoFrame {
        path: String,
        frame_number: u64,
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

pub struct LoadResponse {
    pub image: Image,
}

#[derive(Debug, Clone)]
pub struct AssetMetadata {
    pub kind: AssetKind,
    pub duration: Option<f64>,
    pub fps: Option<f64>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub stream_index: Option<usize>,
}

pub trait LoadPlugin: Plugin {
    /// Open a file and return metadata for all available streams.
    /// The plugin internally caches the reader/decoder for subsequent load() calls.
    /// Returns Err if this plugin cannot handle the file.
    fn open(&self, path: &str) -> Result<Vec<AssetMetadata>, LibraryError>;

    /// Load a frame from a file.
    /// The plugin uses internally cached reader if available.
    /// Returns Err if the request type is not supported.
    fn load(
        &self,
        request: &LoadRequest,
        cache: &CacheManager,
    ) -> Result<LoadResponse, LibraryError>;

    fn plugin_type(&self) -> PluginCategory {
        PluginCategory::Load
    }
}

pub struct LoadRepository {
    pub plugins: HashMap<String, Arc<dyn LoadPlugin>>,
    /// Plugin IDs in priority order (first = highest priority).
    priority_order: Vec<String>,
}

impl LoadRepository {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
            priority_order: Vec::new(),
        }
    }

    pub fn register(&mut self, plugin: Arc<dyn LoadPlugin>) {
        let id = plugin.id().to_string();
        if !self.priority_order.contains(&id) {
            self.priority_order.push(id.clone());
        }
        self.plugins.insert(id, plugin);
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
