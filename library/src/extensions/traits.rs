use ordered_float::OrderedFloat;
use std::collections::HashMap;
use std::sync::Arc;
use crate::error::LibraryError;
use crate::cache::CacheManager;
use crate::io::image::Image;
use crate::core::model::property::PropertyValue;
use crate::graphics::renderer::RenderOutput;
use crate::graphics::skia_utils::GpuContext;
use crate::core::model::project::{Project, Composition}; // Added imports
use crate::core::model::asset::AssetKind;
use serde_json::Value;

// Re-export specific types if needed
pub use crate::extensions::properties::{
    ConstantPropertyPlugin, ExpressionPropertyPlugin, KeyframePropertyPlugin,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginCategory {
    Effect,
    Load,
    Export,
    Property,
    EntityConverter,
    Inspector,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PropertyUiType {
    Float {
        min: f64,
        max: f64,
        step: f64,
        suffix: String,
    },
    Integer {
        min: i64,
        max: i64,
        suffix: String,
    },
    Color,
    Text,
    MultilineText,
    Bool,
    Vec2 {
        suffix: String,
    },
    Vec3 {
        suffix: String,
    },
    Vec4 {
        suffix: String,
    },
    Dropdown {
        options: Vec<String>,
    },
    Font,
    Styles,
}

#[derive(Debug, Clone)]
pub struct PropertyDefinition {
    pub name: String,
    pub label: String,
    pub ui_type: PropertyUiType,
    pub default_value: PropertyValue,
    pub category: String,
}

#[derive(Debug, Clone)]
pub struct EffectDefinition {
    pub label: String,
    pub properties: Vec<PropertyDefinition>,
}

pub trait Plugin: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> String;
    fn category(&self) -> String;
    fn version(&self) -> (u32, u32, u32);
    fn impl_type(&self) -> String {
        "Native".to_string()
    }
}

pub trait EffectPlugin: Plugin {
    fn apply(
        &self,
        input: &RenderOutput,
        params: &HashMap<String, PropertyValue>,
        gpu_context: Option<&mut GpuContext>,
    ) -> Result<RenderOutput, LibraryError>;

    fn properties(&self) -> Vec<PropertyDefinition>;

    fn plugin_type(&self) -> PluginCategory {
        PluginCategory::Effect
    }
}

pub trait PropertyEvaluator: Send + Sync {
    fn evaluate(&self, property: &crate::core::model::property::Property, time: f64, ctx: &EvaluationContext) -> PropertyValue;
}

pub struct EvaluationContext<'a> {
    pub property_map: &'a crate::core::model::property::PropertyMap,
}

pub trait PropertyPlugin: Plugin {
    fn get_evaluator_instance(&self) -> Arc<dyn PropertyEvaluator>;

    fn plugin_type(&self) -> PluginCategory {
        PluginCategory::Property
    }
}

#[derive(Debug, Clone)]
pub enum LoadRequest {
    Image { path: String },
    VideoFrame { path: String, frame_number: u64 },
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

pub trait InspectorPlugin: Plugin {
    fn get_definitions(
        &self,
        kind: &crate::core::model::TrackClipKind,
    ) -> Vec<PropertyDefinition>;

    fn plugin_type(&self) -> PluginCategory {
        PluginCategory::Inspector
    }
}
