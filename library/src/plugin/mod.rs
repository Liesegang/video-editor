#![allow(improper_ctypes_definitions)]
use ordered_float::OrderedFloat;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

use crate::cache::CacheManager;

use crate::loader::image::Image;
// use crate::model::project::entity::Entity; // Removed - This line was there from previous step, but should be removed
use crate::error::LibraryError;
use crate::model::project::project::{Composition, Project};
use libloading::{Library, Symbol};
use log::debug;
use serde_json::Value;

use crate::framing::entity_converters::{EntityConverterPlugin, EntityConverterRegistry}; // Added this line
use crate::rendering::renderer::RenderOutput;
use crate::rendering::skia_utils::GpuContext;

pub type PropertyPluginCreateFn = unsafe extern "C" fn() -> *mut dyn PropertyPlugin;
pub type EffectPluginCreateFn = unsafe extern "C" fn() -> *mut dyn EffectPlugin;
pub type LoadPluginCreateFn = unsafe extern "C" fn() -> *mut dyn LoadPlugin;
pub type ExportPluginCreateFn = unsafe extern "C" fn() -> *mut dyn ExportPlugin;
pub type EntityConverterPluginCreateFn = unsafe extern "C" fn() -> *mut dyn EntityConverterPlugin;

pub mod effects;
pub mod exporters;
pub mod loaders;
pub mod properties;

// Publicly re-export plugin types from their sub-modules
pub use crate::plugin::effects::blur::BlurEffectPlugin;
pub use crate::plugin::effects::dilate::DilateEffectPlugin;
pub use crate::plugin::effects::drop_shadow::DropShadowEffectPlugin;
pub use crate::plugin::effects::erode::ErodeEffectPlugin;
pub use crate::plugin::effects::magnifier::MagnifierEffectPlugin;
pub use crate::plugin::effects::tile::TileEffectPlugin;
pub use crate::plugin::exporters::ffmpeg_export::FfmpegExportPlugin;
pub use crate::plugin::exporters::png_export::PngExportPlugin;
pub use crate::plugin::loaders::ffmpeg_video::FfmpegVideoLoader;
pub use crate::plugin::loaders::native_image::NativeImageLoader;
pub use crate::plugin::properties::{
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

pub trait InspectorPlugin: Plugin {
    // We pass the "kind" of the clip (e.g. Video, Image, Effect?)
    // Actually, for now, let's just say it receives the TrackClipKind.
    fn get_definitions(
        &self,
        kind: &crate::model::project::TrackClipKind,
    ) -> Vec<PropertyDefinition>;

    fn plugin_type(&self) -> PluginCategory {
        PluginCategory::Inspector
    }
}

// Re-export this if needed?
pub type InspectorPluginCreateFn = unsafe extern "C" fn() -> *mut dyn InspectorPlugin;

pub trait Plugin: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> String;
    fn category(&self) -> String; // New category grouping
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

pub trait PropertyPlugin: Plugin {
    fn get_evaluator_instance(&self) -> Arc<dyn PropertyEvaluator>;

    fn plugin_type(&self) -> PluginCategory {
        PluginCategory::Property
    }
}

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

use crate::model::project::asset::AssetKind; // Added import

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
        // Default implementation tries individual methods (backward compatibility logic if needed,
        // but for efficiency plugins should override this).
        // For now, simpler to leave default as None or implement naive fallback.
        // Let's rely on implementation to do it right.
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

pub struct EffectRepository {
    plugins: HashMap<String, Arc<dyn EffectPlugin>>,
}

impl EffectRepository {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }

    pub fn register(&mut self, plugin: Arc<dyn EffectPlugin>) {
        self.plugins.insert(plugin.id().to_string(), plugin);
    }

    pub fn get(&self, id: &str) -> Option<&Arc<dyn EffectPlugin>> {
        self.plugins.get(id)
    }
}

pub struct LoadRepository {
    plugins: HashMap<String, Arc<dyn LoadPlugin>>,
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

pub struct ExportRepository {
    plugins: HashMap<String, Arc<dyn ExportPlugin>>,
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

pub struct EntityConverterRepository {
    plugins: HashMap<String, Arc<dyn EntityConverterPlugin>>,
}

// ... existing impl ...

pub struct InspectorRepository {
    plugins: HashMap<String, Arc<dyn InspectorPlugin>>,
}

impl InspectorRepository {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }

    pub fn register(&mut self, plugin: Arc<dyn InspectorPlugin>) {
        self.plugins.insert(plugin.id().to_string(), plugin);
    }
}

impl EntityConverterRepository {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }

    pub fn register(&mut self, plugin: Arc<dyn EntityConverterPlugin>) {
        self.plugins.insert(plugin.id().to_string(), plugin);
    }

    pub fn get(&self, id: &str) -> Option<&Arc<dyn EntityConverterPlugin>> {
        self.plugins.get(id)
    }
}

struct PluginRepository {
    effect_plugins: EffectRepository,
    load_plugins: LoadRepository,
    export_plugins: ExportRepository,
    entity_converter_plugins: EntityConverterRepository,
    inspector_plugins: InspectorRepository,
    property_evaluators: PropertyEvaluatorRegistry, // Direct ownership
    dynamic_libraries: Vec<Library>,                // Moved here
}

pub struct PluginManager {
    inner: RwLock<PluginRepository>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(PluginRepository {
                effect_plugins: EffectRepository::new(),
                load_plugins: LoadRepository::new(),
                export_plugins: ExportRepository::new(),
                entity_converter_plugins: EntityConverterRepository::new(),
                inspector_plugins: InspectorRepository::new(),
                property_evaluators: PropertyEvaluatorRegistry::new(),
                dynamic_libraries: Vec::new(), // Initialized here
            }),
        }
    }

    pub fn register_effect(&self, plugin: Arc<dyn EffectPlugin>) {
        let mut inner = self.inner.write().unwrap();
        inner.effect_plugins.register(plugin);
    }

    pub fn register_load_plugin(&self, plugin: Arc<dyn LoadPlugin>) {
        let mut inner = self.inner.write().unwrap();
        inner.load_plugins.register(plugin);
    }

    pub fn register_export_plugin(&self, plugin: Arc<dyn ExportPlugin>) {
        let mut inner = self.inner.write().unwrap();
        inner.export_plugins.register(plugin);
    }

    pub fn register_entity_converter_plugin(&self, plugin: Arc<dyn EntityConverterPlugin>) {
        let mut inner = self.inner.write().unwrap();
        inner.entity_converter_plugins.register(plugin);
    }

    pub fn register_property_plugin(&self, plugin: Arc<dyn PropertyPlugin>) {
        let mut inner = self.inner.write().unwrap();
        let evaluator_id = plugin.id();
        let evaluator_instance = plugin.get_evaluator_instance();
        inner
            .property_evaluators
            .register(evaluator_id, evaluator_instance);
    }

    pub fn register_inspector_plugin(&self, plugin: Arc<dyn InspectorPlugin>) {
        let mut inner = self.inner.write().unwrap();
        inner.inspector_plugins.register(plugin);
    }

    pub fn apply_effect(
        &self,
        key: &str,
        input: &RenderOutput,
        params: &HashMap<String, PropertyValue>,
        gpu_context: Option<&mut GpuContext>,
    ) -> Result<RenderOutput, LibraryError> {
        let inner = self.inner.read().unwrap();
        if let Some(plugin) = inner.effect_plugins.get(key) {
            debug!("PluginManager: Applying effect '{}'", key);
            plugin.apply(input, params, gpu_context)
        } else {
            log::warn!("Effect '{}' not found", key);
            Ok(input.clone())
        }
    }

    pub fn get_effect_definition(&self, effect_id: &str) -> Option<EffectDefinition> {
        let inner = self.inner.read().unwrap();
        inner
            .effect_plugins
            .get(effect_id)
            .map(|plugin| EffectDefinition {
                label: plugin.name(),
                properties: plugin.properties(),
            })
    }

    pub fn get_default_effect_config(
        &self,
        effect_id: &str,
    ) -> Option<crate::model::project::EffectConfig> {
        let def = self.get_effect_definition(effect_id)?;
        let mut props = crate::model::project::property::PropertyMap::new();
        for p in def.properties {
            props.set(
                p.name,
                crate::model::project::property::Property::constant(p.default_value),
            );
        }
        Some(crate::model::project::EffectConfig {
            id: uuid::Uuid::new_v4(),
            effect_type: effect_id.to_string(),
            properties: props,
        })
    }

    pub fn load_resource(
        &self,
        request: &LoadRequest,
        cache: &CacheManager,
    ) -> Result<LoadResponse, LibraryError> {
        let inner = self.inner.read().unwrap();
        for plugin in inner.load_plugins.get_sorted_plugins() {
            if plugin.supports(request) {
                return plugin.load(request, cache);
            }
        }
        Err(LibraryError::Plugin(format!(
            "No load plugin registered for request {:?}",
            request
        )))
    }

    pub fn get_metadata(&self, path: &str) -> Option<AssetMetadata> {
        let inner = self.inner.read().unwrap();

        for plugin in inner.load_plugins.get_sorted_plugins() {
            if let Some(metadata) = plugin.get_metadata(path) {
                return Some(metadata);
            }
        }
        None
    }

    pub fn get_available_streams(&self, path: &str) -> Option<Vec<AssetMetadata>> {
        let inner = self.inner.read().unwrap();

        for plugin in inner.load_plugins.get_sorted_plugins() {
            if let Some(streams) = plugin.get_available_streams(path) {
                return Some(streams);
            }
        }
        None
    }

    pub fn probe_asset_kind(&self, path: &str) -> AssetKind {
        let inner = self.inner.read().unwrap();

        for plugin in inner.load_plugins.get_sorted_plugins() {
            if let Some(kind) = plugin.get_asset_kind(path) {
                return kind;
            }
        }
        AssetKind::Other
    }

    pub fn get_duration(&self, path: &str) -> Option<f64> {
        let inner = self.inner.read().unwrap();

        for plugin in inner.load_plugins.get_sorted_plugins() {
            if let Some(duration) = plugin.get_duration(path) {
                return Some(duration);
            }
        }
        None
    }

    pub fn get_fps(&self, path: &str) -> Option<f64> {
        let inner = self.inner.read().unwrap();

        for plugin in inner.load_plugins.get_sorted_plugins() {
            if let Some(fps) = plugin.get_fps(path) {
                return Some(fps);
            }
        }
        None
    }

    pub fn get_dimensions(&self, path: &str) -> Option<(u32, u32)> {
        let inner = self.inner.read().unwrap();
        for plugin in inner.load_plugins.get_sorted_plugins() {
            if let Some(dimensions) = plugin.get_dimensions(path) {
                return Some(dimensions);
            }
        }
        None
    }

    pub fn export_image(
        &self,
        exporter_id: &str, // Changed from format: ExportFormat
        path: &str,
        image: &Image,
        settings: &ExportSettings,
    ) -> Result<(), LibraryError> {
        let inner = self.inner.read().unwrap();
        if let Some(plugin) = inner.export_plugins.get(exporter_id) {
            return plugin.export_image(path, image, settings);
        }
        Err(LibraryError::Plugin(format!(
            "Exporter '{}' not found",
            exporter_id
        )))
    }

    pub fn get_export_plugin_properties(
        &self,
        exporter_id: &str,
    ) -> Option<Vec<PropertyDefinition>> {
        let inner = self.inner.read().unwrap();
        inner
            .export_plugins
            .get(exporter_id)
            .map(|p| p.properties())
    }

    pub fn finish_export(&self, exporter_id: &str, path: &str) -> Result<(), LibraryError> {
        let inner = self.inner.read().unwrap();
        if let Some(plugin) = inner.export_plugins.get(exporter_id) {
            return plugin.finish_export(path);
        }
        Err(LibraryError::Plugin(format!(
            "Exporter '{}' not found",
            exporter_id
        )))
    }

    pub fn load_property_plugin_from_file<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<(), LibraryError> {
        unsafe {
            let library = Library::new(path.as_ref())?;
            let constructor: Symbol<unsafe extern "C" fn() -> *mut dyn PropertyPlugin> =
                library.get(b"create_property_plugin")?;
            let raw = constructor();
            if raw.is_null() {
                return Err(LibraryError::Plugin(
                    "create_property_plugin returned null".to_string(),
                ));
            }
            let plugin_box = Box::from_raw(raw);
            let plugin_arc: Arc<dyn PropertyPlugin> = Arc::from(plugin_box); // Convert Box to Arc

            let mut inner = self.inner.write().unwrap();
            let evaluator_id = plugin_arc.id();
            let evaluator_instance = plugin_arc.get_evaluator_instance();
            inner
                .property_evaluators
                .register(evaluator_id, evaluator_instance);
            inner.dynamic_libraries.push(library);
        }
        Ok(())
    }

    pub fn load_effect_plugin_from_file<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<(), LibraryError> {
        unsafe {
            let library = Library::new(path.as_ref())?;
            let constructor: Symbol<unsafe extern "C" fn() -> *mut dyn EffectPlugin> =
                library.get(b"create_effect_plugin")?;
            let raw = constructor();
            if raw.is_null() {
                return Err(LibraryError::Plugin(
                    "create_effect_plugin returned null".to_string(),
                ));
            }
            let plugin_box = Box::from_raw(raw);
            let plugin_arc: Arc<dyn EffectPlugin> = Arc::from(plugin_box); // Convert Box to Arc

            let mut inner = self.inner.write().unwrap();
            inner.effect_plugins.register(plugin_arc);
            inner.dynamic_libraries.push(library);
        }
        Ok(())
    }

    pub fn load_load_plugin_from_file<P: AsRef<Path>>(&self, path: P) -> Result<(), LibraryError> {
        unsafe {
            let library = Library::new(path.as_ref())?;
            let constructor: Symbol<unsafe extern "C" fn() -> *mut dyn LoadPlugin> =
                library.get(b"create_load_plugin")?;
            let raw = constructor();
            if raw.is_null() {
                return Err(LibraryError::Plugin(
                    "create_load_plugin returned null".to_string(),
                ));
            }
            let plugin_box = Box::from_raw(raw);
            let plugin_arc: Arc<dyn LoadPlugin> = Arc::from(plugin_box); // Convert Box to Arc

            let mut inner = self.inner.write().unwrap();
            inner.load_plugins.register(plugin_arc);
            inner.dynamic_libraries.push(library);
        }
        Ok(())
    }

    pub fn load_export_plugin_from_file<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<(), LibraryError> {
        unsafe {
            let library = Library::new(path.as_ref())?;
            let constructor: Symbol<unsafe extern "C" fn() -> *mut dyn ExportPlugin> =
                library.get(b"create_export_plugin")?;
            let raw = constructor();
            if raw.is_null() {
                return Err(LibraryError::Plugin(
                    "create_export_plugin returned null".to_string(),
                ));
            }
            let plugin_box = Box::from_raw(raw);
            let plugin_arc: Arc<dyn ExportPlugin> = Arc::from(plugin_box); // Convert Box to Arc

            let mut inner = self.inner.write().unwrap();
            inner.export_plugins.register(plugin_arc);
            inner.dynamic_libraries.push(library);
        }
        Ok(())
    }

    pub fn load_entity_converter_plugin_from_file<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<(), LibraryError> {
        unsafe {
            let library = Library::new(path.as_ref())?;
            let constructor: Symbol<unsafe extern "C" fn() -> *mut dyn EntityConverterPlugin> =
                library.get(b"create_entity_converter_plugin")?;
            let raw = constructor();
            if raw.is_null() {
                return Err(LibraryError::Plugin(
                    "create_entity_converter_plugin returned null".to_string(),
                ));
            }
            let plugin_box = Box::from_raw(raw);
            let plugin_arc: Arc<dyn EntityConverterPlugin> = Arc::from(plugin_box); // Convert Box to Arc

            let mut inner = self.inner.write().unwrap();
            inner.entity_converter_plugins.register(plugin_arc);
            inner.dynamic_libraries.push(library);
        }
        Ok(())
    }

    pub fn load_inspector_plugin_from_file<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<(), LibraryError> {
        unsafe {
            let library = Library::new(path.as_ref())?;
            let constructor: Symbol<unsafe extern "C" fn() -> *mut dyn InspectorPlugin> =
                library.get(b"create_inspector_plugin")?;
            let raw = constructor();
            if raw.is_null() {
                return Err(LibraryError::Plugin(
                    "create_inspector_plugin returned null".to_string(),
                ));
            }
            let plugin_box = Box::from_raw(raw);
            let plugin_arc: Arc<dyn InspectorPlugin> = Arc::from(plugin_box);

            let mut inner = self.inner.write().unwrap();
            inner.inspector_plugins.register(plugin_arc);
            inner.dynamic_libraries.push(library);
        }
        Ok(())
    }

    pub fn load_plugins_from_directory<P: AsRef<Path>>(
        &self,
        dir_path: P,
    ) -> Result<(), LibraryError> {
        let dir = dir_path.as_ref();
        if !dir.is_dir() {
            log::warn!("Plugin directory not found: {}", dir.display());
            return Ok(());
        }

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                let extension = path.extension().and_then(|s| s.to_str());
                if matches!(extension, Some("dll") | Some("so")) {
                    log::info!("Attempting to load plugin from: {}", path.display());
                    // Try to load as each type of plugin
                    if let Err(e) = self.load_property_plugin_from_file(&path) {
                        log::debug!("Not a property plugin: {}", e);
                    } else {
                        continue;
                    }
                    if let Err(e) = self.load_effect_plugin_from_file(&path) {
                        log::debug!("Not an effect plugin: {}", e);
                    } else {
                        continue;
                    }
                    if let Err(e) = self.load_load_plugin_from_file(&path) {
                        log::debug!("Not a load plugin: {}", e);
                    } else {
                        continue;
                    }
                    if let Err(e) = self.load_export_plugin_from_file(&path) {
                        log::debug!("Not an export plugin: {}", e);
                    } else {
                        continue;
                    }
                    if let Err(e) = self.load_entity_converter_plugin_from_file(&path) {
                        log::debug!("Not an entity converter plugin: {}", e);
                    } else {
                        continue;
                    }
                    if let Err(e) = self.load_inspector_plugin_from_file(&path) {
                        log::debug!("Not an inspector plugin: {}", e);
                    } else {
                        continue;
                    }
                    log::warn!("File is not a recognized plugin type: {}", path.display());
                }
            }
        }
        Ok(())
    }

    pub fn load_sksl_plugins_from_directory<P: AsRef<Path>>(
        &self,
        dir_path: P,
    ) -> Result<(), LibraryError> {
        let dir = dir_path.as_ref();
        if !dir.exists() {
            log::warn!("SkSL plugin directory not found: {}", dir.display());
            return Ok(());
        }

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                // Check for config.toml and shader.sksl
                let config_path = path.join("config.toml");
                let shader_path = path.join("shader.sksl");

                if config_path.exists() && shader_path.exists() {
                    log::info!("Loading SkSL plugin from: {}", path.display());
                    let toml_content =
                        std::fs::read_to_string(&config_path).map_err(|e| LibraryError::Io(e))?;
                    let sksl_content =
                        std::fs::read_to_string(&shader_path).map_err(|e| LibraryError::Io(e))?;

                    match crate::plugin::effects::SkslEffectPlugin::new(
                        &toml_content,
                        &sksl_content,
                    ) {
                        Ok(plugin) => {
                            log::info!("Successfully registered SkSL plugin: {}", plugin.id());
                            self.register_effect(Arc::new(plugin));
                        }
                        Err(e) => {
                            log::error!("Failed to load SkSL plugin at {}: {}", path.display(), e);
                        }
                    }
                } else {
                    log::warn!(
                        "Skipping directory {}, missing config.toml or shader.sksl",
                        path.display()
                    );
                }
            }
        }
        Ok(())
    }

    pub fn get_property_evaluators(&self) -> Arc<PropertyEvaluatorRegistry> {
        let inner = self.inner.read().unwrap();
        Arc::new(inner.property_evaluators.clone())
    }

    pub fn get_entity_converter_registry(&self) -> Arc<EntityConverterRegistry> {
        let inner = self.inner.read().unwrap();
        let mut registry = EntityConverterRegistry::new();
        for plugin in inner.entity_converter_plugins.plugins.values() {
            plugin.register_converters(&mut registry);
        }
        Arc::new(registry) // EntityConverterRegistry will need to be Clone
    }
    pub fn get_inspector_definitions(
        &self,
        kind: &crate::model::project::TrackClipKind,
    ) -> Vec<PropertyDefinition> {
        let inner = self.inner.read().unwrap();
        let mut definitions = Vec::new();
        for plugin in inner.inspector_plugins.plugins.values() {
            definitions.extend(plugin.get_definitions(kind));
        }
        definitions
    }

    pub fn get_available_effects(&self) -> Vec<(String, String, String)> {
        let inner = self.inner.read().unwrap();
        inner
            .effect_plugins
            .plugins
            .values()
            .map(|p| (p.id().to_string(), p.name(), p.category()))
            .collect()
    }

    pub fn get_effect_properties(&self, effect_id: &str) -> Vec<PropertyDefinition> {
        let inner = self.inner.read().unwrap();
        if let Some(plugin) = inner.effect_plugins.get(effect_id) {
            plugin.properties()
        } else {
            Vec::new()
        }
    }

    pub fn get_available_exporters(&self) -> Vec<(String, String)> {
        let inner = self.inner.read().unwrap();
        inner
            .export_plugins
            .plugins
            .values()
            .map(|p| (p.id().to_string(), p.name()))
            .collect()
    }
}
// ... existing imports ...

#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub plugin_type: PluginCategory, // Was category
    pub category: String,            // New field
    pub version: String,
    pub impl_type: String, // Was plugin_type
}

// ... existing code ...

impl PluginManager {
    // ... existing new() ...

    pub fn get_all_plugins(&self) -> Vec<PluginInfo> {
        let inner = self.inner.read().unwrap();
        let mut plugins = Vec::new();

        for p in inner.effect_plugins.plugins.values() {
            let v = p.version();
            plugins.push(PluginInfo {
                id: p.id().to_string(),
                name: p.name(),
                plugin_type: p.plugin_type(),
                category: p.category(), // New field
                version: format!("{}.{}.{}", v.0, v.1, v.2),
                impl_type: p.impl_type(),
            });
        }
        for p in inner.load_plugins.plugins.values() {
            let v = p.version();
            plugins.push(PluginInfo {
                id: p.id().to_string(),
                name: p.name(),
                plugin_type: p.plugin_type(),
                category: p.category(),
                version: format!("{}.{}.{}", v.0, v.1, v.2),
                impl_type: p.impl_type(),
            });
        }
        for p in inner.export_plugins.plugins.values() {
            let v = p.version();
            plugins.push(PluginInfo {
                id: p.id().to_string(),
                name: p.name(),
                plugin_type: p.plugin_type(),
                category: p.category(),
                version: format!("{}.{}.{}", v.0, v.1, v.2),
                impl_type: p.impl_type(),
            });
        }
        for p in inner.entity_converter_plugins.plugins.values() {
            let v = p.version();
            plugins.push(PluginInfo {
                id: p.id().to_string(),
                name: p.name(),
                plugin_type: p.plugin_type(),
                category: p.category(),
                version: format!("{}.{}.{}", v.0, v.1, v.2),
                impl_type: p.impl_type(),
            });
        }
        for p in inner.inspector_plugins.plugins.values() {
            let v = p.version();
            plugins.push(PluginInfo {
                id: p.id().to_string(),
                name: p.name(),
                plugin_type: p.plugin_type(),
                category: p.category(),
                version: format!("{}.{}.{}", v.0, v.1, v.2),
                impl_type: p.impl_type(),
            });
        }

        // Sorting?
        plugins.sort_by(|a, b| a.id.cmp(&b.id));
        plugins
    }

    // ... existing methods ...
}

// Trait and structs moved from framing/property.rs
use crate::model::project::property::{Property, PropertyMap, PropertyValue};
use log::warn;

pub struct PropertyEvaluatorRegistry {
    evaluators: HashMap<&'static str, Arc<dyn PropertyEvaluator>>,
}

impl Clone for PropertyEvaluatorRegistry {
    fn clone(&self) -> Self {
        Self {
            evaluators: self.evaluators.clone(),
        }
    }
}

impl PropertyEvaluatorRegistry {
    pub fn new() -> Self {
        Self {
            evaluators: HashMap::new(),
        }
    }

    pub fn register(&mut self, key: &'static str, evaluator: Arc<dyn PropertyEvaluator>) {
        self.evaluators.insert(key, evaluator);
    }

    pub fn evaluate(
        &self,
        property: &Property,
        time: f64,
        ctx: &EvaluationContext,
    ) -> PropertyValue {
        let key = property.evaluator.as_str();
        match self.evaluators.get(key) {
            Some(evaluator) => evaluator.evaluate(property, time, ctx),
            None => {
                warn!("Unknown property evaluator '{}'", key);
                PropertyValue::Number(OrderedFloat(0.0))
            }
        }
    }
}

pub trait PropertyEvaluator: Send + Sync {
    fn evaluate(&self, property: &Property, time: f64, ctx: &EvaluationContext) -> PropertyValue;
}

pub struct EvaluationContext<'a> {
    pub property_map: &'a PropertyMap,
    pub fps: f64,
}
