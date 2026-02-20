//! Plugin manager for registering, loading, and accessing plugins.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, RwLock};

use libloading::{Library, Symbol};
use log::debug;

use crate::cache::CacheManager;
use crate::error::LibraryError;
use crate::model::frame::Image;
use crate::model::project::asset::AssetKind;
use crate::model::project::property::PropertyDefinition;
use crate::model::project::property::PropertyValue;
use crate::plugin::EntityConverterPlugin;
use crate::rendering::renderer::RenderOutput;
use crate::rendering::skia_utils::GpuContext;

use crate::plugin::PluginCategory;
use crate::plugin::effects::{EffectDefinition, EffectPlugin};
use crate::plugin::evaluator::PropertyEvaluatorRegistry;
use crate::plugin::exporters::{ExportPlugin, ExportSettings};
use crate::plugin::loaders::{
    AssetMetadata, LoadPlugin, LoadRepository, LoadRequest, LoadResponse,
};
use crate::plugin::node_types::{NodeCategory, NodeTypeDefinition};
use crate::plugin::repository::{PluginRegistry, PluginRepository};

use crate::plugin::traits::{Plugin, PropertyPlugin};
use crate::plugin::{DecoratorPlugin, EffectorPlugin, StylePlugin};

use crate::plugin::effects::{
    BlurEffectPlugin, DilateEffectPlugin, DropShadowEffectPlugin, ErodeEffectPlugin,
    MagnifierEffectPlugin, PixelSorterPlugin, TileEffectPlugin,
};
use crate::plugin::entity_converter::{
    ImageEntityConverterPlugin, ShapeEntityConverterPlugin, SkSLEntityConverterPlugin,
    TextEntityConverterPlugin, VideoEntityConverterPlugin,
};
use crate::plugin::exporters::{FfmpegExportPlugin, PngExportPlugin};
use crate::plugin::loaders::{FfmpegVideoLoader, NativeImageLoader};
use crate::plugin::properties::{
    ConstantPropertyPlugin, ExpressionPropertyPlugin, KeyframePropertyPlugin,
};

/// Main plugin manager.
pub struct PluginManager {
    inner: RwLock<PluginRegistry>,
}

impl Default for PluginManager {
    fn default() -> Self {
        let manager = Self::new();

        // Standard Effects
        manager.register_effect(Arc::new(BlurEffectPlugin::new()));
        manager.register_effect(Arc::new(PixelSorterPlugin::new()));
        manager.register_effect(Arc::new(DilateEffectPlugin::new()));
        manager.register_effect(Arc::new(ErodeEffectPlugin::new()));
        manager.register_effect(Arc::new(DropShadowEffectPlugin::new()));
        manager.register_effect(Arc::new(MagnifierEffectPlugin::new()));
        manager.register_effect(Arc::new(TileEffectPlugin::new()));

        // Standard Loaders
        manager.register_load_plugin(Arc::new(NativeImageLoader::new()));
        manager.register_load_plugin(Arc::new(FfmpegVideoLoader::new()));

        // Standard Exporters
        manager.register_export_plugin(Arc::new(PngExportPlugin::new()));
        manager.register_export_plugin(Arc::new(FfmpegExportPlugin::new()));

        // Standard Property Plugins
        manager.register_property_plugin(Arc::new(ConstantPropertyPlugin::new()));
        manager.register_property_plugin(Arc::new(KeyframePropertyPlugin::new()));
        manager.register_property_plugin(Arc::new(ExpressionPropertyPlugin::new()));

        // Standard Entity Converters
        manager.register_entity_converter_plugin(Arc::new(VideoEntityConverterPlugin::new()));
        manager.register_entity_converter_plugin(Arc::new(ImageEntityConverterPlugin::new()));
        manager.register_entity_converter_plugin(Arc::new(TextEntityConverterPlugin::new()));
        manager.register_entity_converter_plugin(Arc::new(ShapeEntityConverterPlugin::new()));
        manager.register_entity_converter_plugin(Arc::new(SkSLEntityConverterPlugin::new()));

        // Standard Effectors
        manager
            .register_effector_plugin(Arc::new(crate::plugin::effectors::TransformEffectorPlugin));
        manager
            .register_effector_plugin(Arc::new(crate::plugin::effectors::StepDelayEffectorPlugin));
        manager
            .register_effector_plugin(Arc::new(crate::plugin::effectors::RandomizeEffectorPlugin));
        manager.register_effector_plugin(Arc::new(crate::plugin::effectors::OpacityEffectorPlugin));

        // Standard Decorators
        manager.register_decorator_plugin(Arc::new(
            crate::plugin::decorators::BackplateDecoratorPlugin,
        ));

        // Standard Styles
        manager.register_style_plugin(Arc::new(crate::plugin::styles::FillStylePlugin));
        manager.register_style_plugin(Arc::new(crate::plugin::styles::StrokeStylePlugin));

        // Auto-register NodeTypeDefinitions from existing plugins
        manager.auto_register_node_types();

        // Register all built-in node types (from node_list.yml documentation)
        crate::plugin::node_definitions::register_all_node_types(&manager);

        manager
    }
}

/// Generate register, get, get_available, and get_properties methods for a plugin category.
macro_rules! impl_plugin_accessors {
    (
        $( {
            register: $register_fn:ident,
            get: $get_fn:ident,
            get_available: $get_available_fn:ident,
            get_properties: $get_properties_fn:ident,
            field: $field:ident,
            trait_type: $trait_type:ty
        } ),+ $(,)?
    ) => {
        impl PluginManager {
            $(
                pub fn $register_fn(&self, plugin: Arc<$trait_type>) {
                    let mut inner = self.inner.write().unwrap();
                    inner.$field.register(plugin);
                }

                pub fn $get_fn(&self, id: &str) -> Option<Arc<$trait_type>> {
                    let inner = self.inner.read().unwrap();
                    inner.$field.get(id).cloned()
                }

                pub fn $get_available_fn(&self) -> Vec<String> {
                    let inner = self.inner.read().unwrap();
                    inner.$field.values().map(|p| p.id().to_string()).collect()
                }

                pub fn $get_properties_fn(&self, id: &str) -> Vec<PropertyDefinition> {
                    self.$get_fn(id)
                        .map(|p| p.properties())
                        .unwrap_or_default()
                }
            )+
        }
    };
}

impl_plugin_accessors! {
    {
        register: register_effector_plugin,
        get: get_effector_plugin,
        get_available: get_available_effectors,
        get_properties: get_effector_properties,
        field: effector_plugins,
        trait_type: dyn EffectorPlugin
    },
    {
        register: register_decorator_plugin,
        get: get_decorator_plugin,
        get_available: get_available_decorators,
        get_properties: get_decorator_properties,
        field: decorator_plugins,
        trait_type: dyn DecoratorPlugin
    },
    {
        register: register_style_plugin,
        get: get_style_plugin,
        get_available: get_available_styles,
        get_properties: get_style_properties,
        field: style_plugins,
        trait_type: dyn StylePlugin
    }
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(PluginRegistry {
                effect_plugins: PluginRepository::new(),
                load_plugins: LoadRepository::new(),
                export_plugins: PluginRepository::new(),
                entity_converter_plugins: PluginRepository::new(),
                effector_plugins: PluginRepository::new(),
                decorator_plugins: PluginRepository::new(),
                style_plugins: PluginRepository::new(),
                property_evaluators: PropertyEvaluatorRegistry::new(),
                node_types: HashMap::new(),
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

    /// Set the priority order for loader plugins.
    pub fn set_loader_priority(&self, order: Vec<String>) {
        let mut inner = self.inner.write().unwrap();
        inner.load_plugins.set_priority_order(order);
    }

    /// Get the current loader plugin priority order.
    pub fn get_loader_priority(&self) -> Vec<String> {
        let inner = self.inner.read().unwrap();
        inner.load_plugins.get_priority_order().to_vec()
    }

    /// Get list of all registered loader plugins (id, name).
    pub fn get_loader_plugins(&self) -> Vec<(String, String)> {
        let inner = self.inner.read().unwrap();
        inner
            .load_plugins
            .get_priority_order()
            .iter()
            .filter_map(|id| inner.load_plugins.get(id).map(|p| (id.clone(), p.name())))
            .collect()
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
    ) -> Option<crate::model::project::effect::EffectConfig> {
        let def = self.get_effect_definition(effect_id)?;
        let mut props = crate::model::project::property::PropertyMap::new();
        for p in def.properties {
            props.set(
                p.name().to_string(),
                crate::model::project::property::Property::constant(p.default_value().clone()),
            );
        }
        Some(crate::model::project::effect::EffectConfig {
            id: uuid::Uuid::new_v4(),
            effect_type: effect_id.to_string(),
            properties: props,
        })
    }

    /// Load a resource (image or video frame).
    pub fn load_resource(
        &self,
        request: &LoadRequest,
        cache: &CacheManager,
    ) -> Result<LoadResponse, LibraryError> {
        let inner = self.inner.read().unwrap();
        for plugin in inner.load_plugins.values() {
            if let Ok(response) = plugin.load(request, cache) {
                return Ok(response);
            }
        }
        let path = request.path();
        log::error!("Failed to load resource: {}", path);
        Err(LibraryError::plugin(format!(
            "No load plugin registered for path {:?}",
            path
        )))
    }

    /// Get metadata for the first stream (for backward compatibility).
    pub fn get_metadata(&self, path: &str) -> Option<AssetMetadata> {
        self.get_available_streams(path)
            .and_then(|streams| streams.into_iter().next())
    }

    /// Get all available streams/resources from a file.
    pub fn get_available_streams(&self, path: &str) -> Option<Vec<AssetMetadata>> {
        let inner = self.inner.read().unwrap();
        for plugin in inner.load_plugins.values() {
            if let Ok(streams) = plugin.open(path) {
                return Some(streams);
            }
        }
        None
    }

    pub fn probe_asset_kind(&self, path: &str) -> AssetKind {
        self.get_metadata(path)
            .map(|m| m.kind)
            .unwrap_or(AssetKind::Other)
    }

    pub fn get_duration(&self, path: &str) -> Option<f64> {
        self.get_metadata(path).and_then(|m| m.duration)
    }

    pub fn get_fps(&self, path: &str) -> Option<f64> {
        self.get_metadata(path).and_then(|m| m.fps)
    }

    pub fn get_dimensions(&self, path: &str) -> Option<(u32, u32)> {
        self.get_metadata(path)
            .and_then(|m| match (m.width, m.height) {
                (Some(w), Some(h)) => Some((w, h)),
                _ => None,
            })
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
        Err(LibraryError::plugin(format!(
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
        Err(LibraryError::plugin(format!(
            "Exporter '{}' not found",
            exporter_id
        )))
    }

    unsafe fn load_plugin_generic<T: ?Sized + 'static>(
        &self,
        path: &Path,
        symbol: &[u8],
        register: impl FnOnce(&mut PluginRegistry, Arc<T>),
    ) -> Result<(), LibraryError> {
        let library = unsafe { Library::new(path)? };
        let constructor: Symbol<unsafe extern "C" fn() -> *mut T> = unsafe { library.get(symbol)? };
        let raw = unsafe { constructor() };
        if raw.is_null() {
            return Err(LibraryError::plugin(format!(
                "Plugin constructor {} returned null",
                String::from_utf8_lossy(symbol)
            )));
        }
        let plugin = unsafe { Arc::from(Box::from_raw(raw)) };

        let mut inner = self.inner.write().unwrap();
        register(&mut *inner, plugin);
        inner.dynamic_libraries.push(library);
        Ok(())
    }

    pub fn load_property_plugin_from_file<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<(), LibraryError> {
        unsafe {
            self.load_plugin_generic::<dyn PropertyPlugin>(
                path.as_ref(),
                b"create_property_plugin",
                |inner, plugin| {
                    let evaluator_id = plugin.id();
                    let evaluator_instance = plugin.get_evaluator_instance();
                    inner
                        .property_evaluators
                        .register(evaluator_id, evaluator_instance);
                },
            )
        }
    }

    pub fn load_effect_plugin_from_file<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<(), LibraryError> {
        unsafe {
            self.load_plugin_generic::<dyn EffectPlugin>(
                path.as_ref(),
                b"create_effect_plugin",
                |inner, plugin| {
                    inner.effect_plugins.register(plugin);
                },
            )
        }
    }

    pub fn load_load_plugin_from_file<P: AsRef<Path>>(&self, path: P) -> Result<(), LibraryError> {
        unsafe {
            self.load_plugin_generic::<dyn LoadPlugin>(
                path.as_ref(),
                b"create_load_plugin",
                |inner, plugin| {
                    inner.load_plugins.register(plugin);
                },
            )
        }
    }

    pub fn load_export_plugin_from_file<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<(), LibraryError> {
        unsafe {
            self.load_plugin_generic::<dyn ExportPlugin>(
                path.as_ref(),
                b"create_export_plugin",
                |inner, plugin| {
                    inner.export_plugins.register(plugin);
                },
            )
        }
    }

    pub fn load_entity_converter_plugin_from_file<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<(), LibraryError> {
        unsafe {
            self.load_plugin_generic::<dyn EntityConverterPlugin>(
                path.as_ref(),
                b"create_entity_converter_plugin",
                |inner, plugin| {
                    inner.entity_converter_plugins.register(plugin);
                },
            )
        }
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

    pub fn get_entity_converter(&self, kind: &str) -> Option<Arc<dyn EntityConverterPlugin>> {
        let inner = self.inner.read().unwrap();
        for plugin in inner.entity_converter_plugins.values() {
            if plugin.supports_kind(kind) {
                return Some(plugin.clone());
            }
        }
        None
    }

    pub fn get_inspector_definitions(
        &self,
        _kind: &crate::model::project::clip::TrackClipKind,
    ) -> Vec<PropertyDefinition> {
        // Inspector plugins removed. Return empty or implement static logic if needed.
        Vec::new()
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

    pub fn get_all_plugins(&self) -> Vec<PluginInfo> {
        let inner = self.inner.read().unwrap();
        let mut plugins = Vec::new();

        macro_rules! collect_plugins {
            ($repo:expr) => {
                for p in $repo.plugins.values() {
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
            };
        }

        collect_plugins!(inner.effect_plugins);
        collect_plugins!(inner.load_plugins);
        collect_plugins!(inner.export_plugins);
        collect_plugins!(inner.entity_converter_plugins);

        plugins.sort_by(|a, b| a.id.cmp(&b.id));
        plugins
    }

    // ==================== Node Type Definition Methods ====================

    /// Register a node type definition.
    pub fn register_node_type(&self, def: NodeTypeDefinition) {
        let mut inner = self.inner.write().unwrap();
        inner.node_types.insert(def.type_id.clone(), def);
    }

    /// Get a node type definition by type_id.
    pub fn get_node_type(&self, type_id: &str) -> Option<NodeTypeDefinition> {
        let inner = self.inner.read().unwrap();
        inner.node_types.get(type_id).cloned()
    }

    /// Get all available node type definitions.
    pub fn get_available_node_types(&self) -> Vec<NodeTypeDefinition> {
        let inner = self.inner.read().unwrap();
        inner.node_types.values().cloned().collect()
    }

    /// Get node type definitions filtered by category.
    pub fn get_node_types_by_category(&self, category: NodeCategory) -> Vec<NodeTypeDefinition> {
        let inner = self.inner.read().unwrap();
        inner
            .node_types
            .values()
            .filter(|def| def.category == category)
            .cloned()
            .collect()
    }

    /// Auto-register NodeTypeDefinitions from existing effect/style/effector/decorator plugins.
    fn auto_register_node_types(&self) {
        use crate::model::project::connection::PinDataType;

        let inner = self.inner.read().unwrap();
        let mut defs = Vec::new();

        let configs = [
            AutoRegisterConfig {
                type_prefix: "effect.",
                category: NodeCategory::Effect,
                chain_input: ("image_in", "Image", PinDataType::Image),
                chain_output: ("image_out", "Image", PinDataType::Image),
            },
            AutoRegisterConfig {
                type_prefix: "style.",
                category: NodeCategory::Style,
                chain_input: ("style_in", "Style In", PinDataType::Style),
                chain_output: ("style_out", "Style", PinDataType::Style),
            },
            AutoRegisterConfig {
                type_prefix: "effector.",
                category: NodeCategory::Effector,
                chain_input: ("effector_in", "Effector In", PinDataType::Effector),
                chain_output: ("effector_out", "Effector", PinDataType::Effector),
            },
            AutoRegisterConfig {
                type_prefix: "decorator.",
                category: NodeCategory::Decorator,
                chain_input: ("decorator_in", "Decorator In", PinDataType::Decorator),
                chain_output: ("decorator_out", "Decorator", PinDataType::Decorator),
            },
        ];

        // Collect (id, name, properties) from each plugin category
        let plugin_data: Vec<Vec<(String, String, Vec<PropertyDefinition>)>> = vec![
            inner
                .effect_plugins
                .values()
                .map(|p| (p.id().to_string(), p.name(), p.properties()))
                .collect(),
            inner
                .style_plugins
                .values()
                .map(|p| (p.id().to_string(), p.name(), p.properties()))
                .collect(),
            inner
                .effector_plugins
                .values()
                .map(|p| (p.id().to_string(), p.name(), p.properties()))
                .collect(),
            inner
                .decorator_plugins
                .values()
                .map(|p| (p.id().to_string(), p.name(), p.properties()))
                .collect(),
        ];

        for (config, plugins) in configs.iter().zip(plugin_data.iter()) {
            for (plugin_id, plugin_name, props) in plugins {
                defs.push(build_node_type_def(
                    config,
                    plugin_id,
                    plugin_name,
                    props.clone(),
                ));
            }
        }

        drop(inner);

        for def in defs {
            self.register_node_type(def);
        }
    }
}

/// Configuration for auto-registering a plugin category as node types.
struct AutoRegisterConfig {
    type_prefix: &'static str,
    category: NodeCategory,
    chain_input: (
        &'static str,
        &'static str,
        crate::model::project::connection::PinDataType,
    ),
    chain_output: (
        &'static str,
        &'static str,
        crate::model::project::connection::PinDataType,
    ),
}

/// Build a NodeTypeDefinition from plugin metadata and config.
fn build_node_type_def(
    config: &AutoRegisterConfig,
    plugin_id: &str,
    plugin_name: &str,
    props: Vec<PropertyDefinition>,
) -> NodeTypeDefinition {
    use crate::model::project::connection::PinDefinition;

    let (in_name, in_label, in_type) = &config.chain_input;
    let (out_name, out_label, out_type) = &config.chain_output;

    let mut inputs = vec![PinDefinition::input(in_name, in_label, in_type.clone())];
    for prop in &props {
        let pin_type = property_ui_to_pin_data_type(prop.ui_type());
        inputs.push(PinDefinition::input(prop.name(), prop.label(), pin_type));
    }
    let outputs = vec![PinDefinition::output(out_name, out_label, out_type.clone())];

    NodeTypeDefinition::new(
        &format!("{}{}", config.type_prefix, plugin_id),
        plugin_name,
        config.category,
    )
    .with_inputs(inputs)
    .with_outputs(outputs)
    .with_properties(props)
}

/// Convert a PropertyUiType to the closest PinDataType.
fn property_ui_to_pin_data_type(
    ui_type: &crate::model::project::property::PropertyUiType,
) -> crate::model::project::connection::PinDataType {
    use crate::model::project::connection::PinDataType;
    use crate::model::project::property::PropertyUiType;
    match ui_type {
        PropertyUiType::Float { .. } => PinDataType::Scalar,
        PropertyUiType::Integer { .. } => PinDataType::Integer,
        PropertyUiType::Color => PinDataType::Color,
        PropertyUiType::Text | PropertyUiType::MultilineText | PropertyUiType::Font => {
            PinDataType::String
        }
        PropertyUiType::Bool => PinDataType::Boolean,
        PropertyUiType::Vec2 { .. } => PinDataType::Vec2,
        PropertyUiType::Vec3 { .. } => PinDataType::Vec3,
        PropertyUiType::Vec4 { .. } => PinDataType::Scalar, // No Vec4 pin type yet
        PropertyUiType::Dropdown { .. } => PinDataType::Enum,
    }
}

/// Information about a registered plugin.
#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub plugin_type: PluginCategory,
    pub category: String,
    pub version: String,
    pub impl_type: String,
}
