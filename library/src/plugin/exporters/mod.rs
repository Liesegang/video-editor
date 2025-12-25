pub mod ffmpeg_export;
pub mod png_export;

pub use self::ffmpeg_export::FfmpegExportPlugin;
pub use self::png_export::PngExportPlugin;

use crate::core::media::image::Image;
use crate::error::LibraryError;
use crate::model::project::project::{Composition, Project};
use crate::model::project::property::PropertyDefinition;
use crate::plugin::{Plugin, PluginCategory};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

pub trait ExportPlugin: Plugin {
    fn export_image(
        &self,
        path: &str,
        image: &Image,
        settings: &ExportSettings,
    ) -> Result<(), LibraryError>;

    fn finish_export(&self, _path: &str) -> Result<(), LibraryError> {
        Ok(())
    }

    fn properties(&self) -> Vec<PropertyDefinition> {
        Vec::new()
    }

    fn plugin_type(&self) -> PluginCategory {
        PluginCategory::Export
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExportFormat {
    Png,
    Video,
}

#[derive(Debug, Clone)]
pub struct ExportSettings {
    pub container: String,
    pub codec: String,
    pub pixel_format: String,
    pub ffmpeg_path: Option<String>,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub parameters: HashMap<String, Value>,
}

impl ExportSettings {
    pub fn from_project(
        project: &Project,
        composition: &Composition,
    ) -> Result<Self, LibraryError> {
        let mut settings = ExportSettings::for_dimensions(
            composition.width as u32,
            composition.height as u32,
            composition.fps,
        );

        let config = &project.export;
        if config.container.is_none()
            && config.codec.is_none()
            && config.pixel_format.is_none()
            && config.ffmpeg_path.is_none()
            && config.parameters.is_empty()
        {
            return Ok(settings);
        }

        if let Some(value) = &config.container {
            settings.container = value.clone();
        }
        if let Some(value) = &config.codec {
            settings.codec = value.clone();
        }
        if let Some(value) = &config.pixel_format {
            settings.pixel_format = value.clone();
        }
        if let Some(value) = &config.ffmpeg_path {
            settings.ffmpeg_path = Some(value.clone());
        }
        settings.parameters = config.parameters.clone();

        if matches!(settings.export_format(), ExportFormat::Video) {
            if settings.codec == "png" {
                settings.codec = "libx264".into();
            }
            if settings.pixel_format == "rgba" {
                settings.pixel_format = "yuv420p".into();
            }
        }

        Ok(settings)
    }

    pub fn for_dimensions(width: u32, height: u32, fps: f64) -> Self {
        Self {
            container: "png".into(),
            codec: "png".into(),
            pixel_format: "rgba".into(),
            ffmpeg_path: None,
            width,
            height,
            fps,
            parameters: HashMap::new(),
        }
    }

    pub fn export_format(&self) -> ExportFormat {
        match self.container.as_str() {
            "png" | "apng" => ExportFormat::Png,
            _ => ExportFormat::Video,
        }
    }

    pub fn parameter_string(&self, key: &str) -> Option<String> {
        match self.parameters.get(key)? {
            Value::String(value) => Some(value.clone()),
            Value::Number(value) => Some(value.to_string()),
            Value::Bool(value) => Some(value.to_string()),
            _ => None,
        }
    }

    pub fn parameter_u64(&self, key: &str) -> Option<u64> {
        match self.parameters.get(key)? {
            Value::Number(value) => {
                if value.is_u64() {
                    value.as_u64()
                } else if value.is_i64() {
                    value
                        .as_i64()
                        .and_then(|v| if v >= 0 { Some(v as u64) } else { None })
                } else {
                    value.as_f64().map(|v| v.max(0.0).round() as u64)
                }
            }
            Value::String(value) => value.parse::<u64>().ok(),
            _ => None,
        }
    }

    pub fn parameter_f64(&self, key: &str) -> Option<f64> {
        match self.parameters.get(key)? {
            Value::Number(value) => value.as_f64(),
            Value::String(value) => value.parse::<f64>().ok(),
            _ => None,
        }
    }
}

pub struct ExportRepository {
    pub plugins: HashMap<String, Arc<dyn ExportPlugin>>,
}

impl ExportRepository {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }

    pub fn register(&mut self, plugin: Arc<dyn ExportPlugin>) {
        self.plugins.insert(plugin.id().to_string(), plugin);
    }

    pub fn get(&self, id: &str) -> Option<&Arc<dyn ExportPlugin>> {
        self.plugins.get(id)
    }
}
