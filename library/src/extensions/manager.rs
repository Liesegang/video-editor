use ordered_float::OrderedFloat;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

use crate::cache::CacheManager;
use crate::error::LibraryError;
use crate::io::image::Image;
use crate::core::model::property::{Property, PropertyValue};
use crate::core::model::EffectConfig;
use crate::graphics::renderer::RenderOutput;
use crate::graphics::skia_utils::GpuContext;
use libloading::{Library, Symbol};
use log::debug;
use log::warn;

use crate::timeline::converter::{EntityConverterPlugin, EntityConverterRegistry};
use crate::extensions::traits::{
    EffectDefinition, EffectPlugin, ExportPlugin, ExportSettings, InspectorPlugin, LoadPlugin,
    LoadRequest, LoadResponse, Plugin, PluginCategory, PropertyDefinition, PropertyEvaluator,
    PropertyPlugin, AssetMetadata, EvaluationContext
};
use crate::core::model::asset::AssetKind;

// Type definitions for C FFI
pub type PropertyPluginCreateFn = unsafe extern "C" fn() -> *mut dyn PropertyPlugin;
pub type EffectPluginCreateFn = unsafe extern "C" fn() -> *mut dyn EffectPlugin;
pub type LoadPluginCreateFn = unsafe extern "C" fn() -> *mut dyn LoadPlugin;
pub type ExportPluginCreateFn = unsafe extern "C" fn() -> *mut dyn ExportPlugin;
pub type EntityConverterPluginCreateFn = unsafe extern "C" fn() -> *mut dyn EntityConverterPlugin;
pub type InspectorPluginCreateFn = unsafe extern "C" fn() -> *mut dyn InspectorPlugin;

// Repositories
pub struct EffectRepository {
    plugins: HashMap<String, Arc<dyn EffectPlugin>>,
}
impl EffectRepository {
    pub fn new() -> Self { Self { plugins: HashMap::new() } }
    pub fn register(&mut self, plugin: Arc<dyn EffectPlugin>) { self.plugins.insert(plugin.id().to_string(), plugin); }
    pub fn get(&self, id: &str) -> Option<&Arc<dyn EffectPlugin>> { self.plugins.get(id) }
}

pub struct LoadRepository {
    plugins: HashMap<String, Arc<dyn LoadPlugin>>,
}
impl LoadRepository {
    pub fn new() -> Self { Self { plugins: HashMap::new() } }
    pub fn register(&mut self, plugin: Arc<dyn LoadPlugin>) { self.plugins.insert(plugin.id().to_string(), plugin); }
}

pub struct ExportRepository {
    plugins: HashMap<String, Arc<dyn ExportPlugin>>,
}
impl ExportRepository {
    pub fn new() -> Self { Self { plugins: HashMap::new() } }
    pub fn register(&mut self, plugin: Arc<dyn ExportPlugin>) { self.plugins.insert(plugin.id().to_string(), plugin); }
    pub fn get(&self, id: &str) -> Option<&Arc<dyn ExportPlugin>> { self.plugins.get(id) }
}

pub struct EntityConverterRepository {
    plugins: HashMap<String, Arc<dyn EntityConverterPlugin>>,
}
impl EntityConverterRepository {
    pub fn new() -> Self { Self { plugins: HashMap::new() } }
    pub fn register(&mut self, plugin: Arc<dyn EntityConverterPlugin>) { self.plugins.insert(plugin.id().to_string(), plugin); }
}

pub struct InspectorRepository {
    plugins: HashMap<String, Arc<dyn InspectorPlugin>>,
}
impl InspectorRepository {
    pub fn new() -> Self { Self { plugins: HashMap::new() } }
    pub fn register(&mut self, plugin: Arc<dyn InspectorPlugin>) { self.plugins.insert(plugin.id().to_string(), plugin); }
}

// Property Evaluator Registry
pub struct PropertyEvaluatorRegistry {
    evaluators: HashMap<&'static str, Arc<dyn PropertyEvaluator>>,
}
impl Clone for PropertyEvaluatorRegistry {
    fn clone(&self) -> Self { Self { evaluators: self.evaluators.clone() } }
}
impl PropertyEvaluatorRegistry {
    pub fn new() -> Self { Self { evaluators: HashMap::new() } }
    pub fn register(&mut self, key: &'static str, evaluator: Arc<dyn PropertyEvaluator>) {
        self.evaluators.insert(key, evaluator);
    }
    pub fn evaluate(&self, property: &Property, time: f64, ctx: &EvaluationContext) -> PropertyValue {
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

// Plugin Manager
struct PluginRepository {
    effect_plugins: EffectRepository,
    load_plugins: LoadRepository,
    export_plugins: ExportRepository,
    entity_converter_plugins: EntityConverterRepository,
    inspector_plugins: InspectorRepository,
    property_evaluators: PropertyEvaluatorRegistry,
    dynamic_libraries: Vec<Library>,
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
                dynamic_libraries: Vec::new(),
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
    ) -> Option<EffectConfig> {
        let def = self.get_effect_definition(effect_id)?;
        let mut props = crate::core::model::property::PropertyMap::new();
        for p in def.properties {
            props.set(
                p.name,
                crate::core::model::property::Property::constant(p.default_value),
            );
        }
        Some(EffectConfig {
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
        for plugin in inner.load_plugins.plugins.values() {
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
        for plugin in inner.load_plugins.plugins.values() {
            if let Some(metadata) = plugin.get_metadata(path) {
                return Some(metadata);
            }
        }
        None
    }

    pub fn probe_asset_kind(&self, path: &str) -> AssetKind {
        let inner = self.inner.read().unwrap();
        for plugin in inner.load_plugins.plugins.values() {
            if let Some(kind) = plugin.get_asset_kind(path) {
                return kind;
            }
        }
        AssetKind::Other
    }

    pub fn get_duration(&self, path: &str) -> Option<f64> {
        let inner = self.inner.read().unwrap();
        for plugin in inner.load_plugins.plugins.values() {
            if let Some(duration) = plugin.get_duration(path) {
                return Some(duration);
            }
        }
        None
    }

    pub fn get_fps(&self, path: &str) -> Option<f64> {
        let inner = self.inner.read().unwrap();
        for plugin in inner.load_plugins.plugins.values() {
            if let Some(fps) = plugin.get_fps(path) {
                return Some(fps);
            }
        }
        None
    }

    pub fn get_dimensions(&self, path: &str) -> Option<(u32, u32)> {
        let inner = self.inner.read().unwrap();
        for plugin in inner.load_plugins.plugins.values() {
            if let Some(dimensions) = plugin.get_dimensions(path) {
                return Some(dimensions);
            }
        }
        None
    }

    pub fn export_image(
        &self,
        exporter_id: &str,
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

    // Dynamic loading methods (simplified to just match signature for now)
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
            let plugin_arc: Arc<dyn PropertyPlugin> = Arc::from(plugin_box);

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
                let config_path = path.join("config.toml");
                let shader_path = path.join("shader.sksl");

                if config_path.exists() && shader_path.exists() {
                    log::info!("Loading SkSL plugin from: {}", path.display());
                    let toml_content =
                        std::fs::read_to_string(&config_path).map_err(|e| LibraryError::Io(e))?;
                    let sksl_content =
                        std::fs::read_to_string(&shader_path).map_err(|e| LibraryError::Io(e))?;

                    match crate::compositing::effects::SkslEffectPlugin::new(
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
        Arc::new(registry)
    }

    pub fn get_inspector_definitions(
        &self,
        kind: &crate::core::model::TrackClipKind,
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

    // Additional info getter
    pub fn get_all_plugins(&self) -> Vec<PluginInfo> {
        let inner = self.inner.read().unwrap();
        let mut plugins = Vec::new();

        for p in inner.effect_plugins.plugins.values() {
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

        plugins.sort_by(|a, b| a.id.cmp(&b.id));
        plugins
    }
}

#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub plugin_type: PluginCategory,
    pub category: String,
    pub version: String,
    pub impl_type: String,
}
