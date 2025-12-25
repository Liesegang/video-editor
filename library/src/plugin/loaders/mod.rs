pub mod ffmpeg_video;
pub mod native_image;

pub use self::ffmpeg_video::FfmpegVideoLoader;
pub use self::native_image::NativeImageLoader;

use crate::cache::CacheManager;
use crate::core::media::image::Image;
use crate::error::LibraryError;
use crate::model::project::asset::AssetKind;
use crate::plugin::{Plugin, PluginCategory};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum LoadRequest {
    Image {
        path: String,
    },
    VideoFrame {
        path: String,
        frame_number: u64,
        stream_index: Option<usize>,
        input_color_space: Option<String>,
        output_color_space: Option<String>,
    },
}

pub enum LoadResponse {
    Image(Image),
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
    fn supports(&self, request: &LoadRequest) -> bool;
    fn load(
        &self,
        request: &LoadRequest,
        cache: &CacheManager,
    ) -> Result<LoadResponse, LibraryError>;

    fn get_metadata(&self, _path: &str) -> Option<AssetMetadata> {
        None
    }

    fn get_available_streams(&self, _path: &str) -> Option<Vec<AssetMetadata>> {
        None
    }

    fn get_asset_kind(&self, _path: &str) -> Option<AssetKind> {
        None
    }

    fn get_duration(&self, _path: &str) -> Option<f64> {
        None
    }

    fn get_fps(&self, _path: &str) -> Option<f64> {
        None
    }

    fn plugin_type(&self) -> PluginCategory {
        PluginCategory::Load
    }

    fn get_dimensions(&self, _path: &str) -> Option<(u32, u32)> {
        None
    }

    fn priority(&self) -> u32 {
        0
    }
}

pub struct LoadRepository {
    pub plugins: HashMap<String, Arc<dyn LoadPlugin>>,
}

impl LoadRepository {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }

    pub fn register(&mut self, plugin: Arc<dyn LoadPlugin>) {
        self.plugins.insert(plugin.id().to_string(), plugin);
    }

    pub fn get(&self, id: &str) -> Option<&Arc<dyn LoadPlugin>> {
        self.plugins.get(id)
    }

    pub fn get_sorted_plugins(&self) -> Vec<Arc<dyn LoadPlugin>> {
        let mut plugins: Vec<_> = self.plugins.values().cloned().collect();
        plugins.sort_by(|a, b| b.priority().cmp(&a.priority()));
        plugins
    }
}
