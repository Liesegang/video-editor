use std::collections::HashMap;
use std::error::Error;
use std::path::Path;
use std::sync::{Arc, RwLock};

use crate::cache::{CacheManager, SharedCacheManager};
use crate::framing::property::PropertyEvaluatorRegistry;
use crate::loader::image::Image;
use crate::model::project::entity::Entity;
use crate::model::project::property::PropertyValue;
use libloading::{Library, Symbol};
use crate::model::project::project::{Composition, Project};
use serde_json::Value;

mod exporters;
mod loaders;
mod property;


use exporters::{FfmpegExportPlugin, PngExportPlugin};
use loaders::{FfmpegVideoLoader, NativeImageLoader};
use property::BuiltinPropertyPlugin;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginCategory {
    Effect,
    Load,
    Export,
    Property,
}

pub trait Plugin: Send + Sync {
    fn id(&self) -> &'static str;
    fn category(&self) -> PluginCategory;
}

pub trait EffectPlugin: Plugin {
    fn create(&self, params: HashMap<String, PropertyValue>) -> Entity;
}

pub trait PropertyPlugin: Plugin {
    fn register(&self, registry: &mut PropertyEvaluatorRegistry);
}

#[derive(Debug, Clone)]
pub enum LoadRequest {
    Image { path: String },
    VideoFrame { path: String, frame_number: u64 },
}

pub enum LoadResponse {
    Image(Image),
}

pub trait LoadPlugin: Plugin {
    fn supports(&self, request: &LoadRequest) -> bool;
    fn load(
        &self,
        request: &LoadRequest,
        cache: &CacheManager,
    ) -> Result<LoadResponse, Box<dyn Error>>;
}

pub trait ExportPlugin: Plugin {
    fn supports(&self, format: ExportFormat) -> bool;
    fn export_image(
        &self,
        format: ExportFormat,
        path: &str,
        image: &Image,
        settings: &ExportSettings,
    ) -> Result<(), Box<dyn Error>>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub fn from_project(project: &Project, composition: &Composition) -> Self {
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
            return settings;
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

        settings
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

struct PluginRepository {
    effect_plugins: HashMap<String, Box<dyn EffectPlugin>>,
    load_plugins: Vec<Box<dyn LoadPlugin>>,
    export_plugins: Vec<Box<dyn ExportPlugin>>,
    property_plugins: Vec<Box<dyn PropertyPlugin>>,
    dynamic_libraries: Vec<Library>,
}

pub struct PluginManager {
    inner: RwLock<PluginRepository>,
    cache_manager: SharedCacheManager,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(PluginRepository {
                effect_plugins: HashMap::new(),
                load_plugins: Vec::new(),
                export_plugins: Vec::new(),
                property_plugins: Vec::new(),
                dynamic_libraries: Vec::new(),
            }),
            cache_manager: Arc::new(CacheManager::new()),
        }
    }

    pub fn cache_manager(&self) -> SharedCacheManager {
        Arc::clone(&self.cache_manager)
    }

    pub fn register_effect(&self, key: &str, plugin: Box<dyn EffectPlugin>) {
        let mut inner = self.inner.write().unwrap();
        inner.effect_plugins.insert(key.to_string(), plugin);
    }

    pub fn register_load_plugin(&self, plugin: Box<dyn LoadPlugin>) {
        let mut inner = self.inner.write().unwrap();
        inner.load_plugins.push(plugin);
    }

    pub fn register_export_plugin(&self, plugin: Box<dyn ExportPlugin>) {
        let mut inner = self.inner.write().unwrap();
        inner.export_plugins.push(plugin);
    }

    pub fn register_property_plugin(&self, plugin: Box<dyn PropertyPlugin>) {
        let mut inner = self.inner.write().unwrap();
        inner.property_plugins.push(plugin);
    }

    pub fn create_entity(
        &self,
        key: &str,
        params: HashMap<String, PropertyValue>,
    ) -> Option<Entity> {
        let inner = self.inner.read().unwrap();
        inner
            .effect_plugins
            .get(key)
            .map(|plugin| plugin.create(params))
    }

    pub fn load_resource(&self, request: &LoadRequest) -> Result<LoadResponse, Box<dyn Error>> {
        let inner = self.inner.read().unwrap();
        for plugin in &inner.load_plugins {
            if plugin.supports(request) {
                return plugin.load(request, &self.cache_manager);
            }
        }
        Err(format!("No load plugin registered for request {:?}", request).into())
    }

    pub fn export_image(
        &self,
        format: ExportFormat,
        path: &str,
        image: &Image,
        settings: &ExportSettings,
    ) -> Result<(), Box<dyn Error>> {
        let inner = self.inner.read().unwrap();
        for plugin in &inner.export_plugins {
            if plugin.supports(format) {
                return plugin.export_image(format, path, image, settings);
            }
        }
        Err("No export plugin registered for requested format".into())
    }

    pub fn build_property_registry(&self) -> PropertyEvaluatorRegistry {
        let mut registry = PropertyEvaluatorRegistry::new();
        let inner = self.inner.read().unwrap();
        for plugin in &inner.property_plugins {
            plugin.register(&mut registry);
        }
        registry
    }

    pub fn load_property_plugin_from_file<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<(), Box<dyn Error>> {
        unsafe {
            let library = Library::new(path.as_ref())?;
            let constructor: Symbol<PropertyPluginCreateFn> =
                library.get(b"create_property_plugin")?;
            let raw = constructor();
            if raw.is_null() {
                return Err("create_property_plugin returned null".into());
            }
            let plugin = Box::from_raw(raw);
            let mut inner = self.inner.write().unwrap();
            inner.property_plugins.push(plugin);
            inner.dynamic_libraries.push(library);
        }
        Ok(())
    }
}

#[allow(improper_ctypes_definitions)]
type PropertyPluginCreateFn = unsafe extern "C" fn() -> *mut dyn PropertyPlugin;

pub struct BasicTextEffectFactory;

impl Plugin for BasicTextEffectFactory {
    fn id(&self) -> &'static str {
        "basic_text_effect"
    }

    fn category(&self) -> PluginCategory {
        PluginCategory::Effect
    }
}

impl EffectPlugin for BasicTextEffectFactory {
    fn create(&self, params: HashMap<String, PropertyValue>) -> Entity {
        let mut text_entity = Entity::new("text");

        if let Some(PropertyValue::String(text)) = params.get("text") {
            text_entity.set_constant_property("text", PropertyValue::String(text.clone()));
        }

        if let Some(PropertyValue::Number(start)) = params.get("start_time") {
            text_entity.start_time = *start;
        }

        if let Some(PropertyValue::Number(end)) = params.get("end_time") {
            text_entity.end_time = *end;
        }

        text_entity.set_constant_property("size", PropertyValue::Number(24.0));
        text_entity.set_constant_property("font", PropertyValue::String("Arial".to_string()));

        text_entity
    }
}

pub fn load_plugins() -> Arc<PluginManager> {
    let manager = Arc::new(PluginManager::new());
    manager.register_effect("basic_text", Box::new(BasicTextEffectFactory));
    manager.register_load_plugin(Box::new(NativeImageLoader::new()));
    manager.register_load_plugin(Box::new(FfmpegVideoLoader::new()));
    manager.register_export_plugin(Box::new(PngExportPlugin::new()));
    manager.register_export_plugin(Box::new(FfmpegExportPlugin::new()));
    manager.register_property_plugin(Box::new(BuiltinPropertyPlugin));
    manager
}
