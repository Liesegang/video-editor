//! Plugin manager for registering, loading, and accessing plugins.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use libloading::{Library, Symbol};
use log::debug;

use crate::cache::CacheManager;
use crate::error::LibraryError;
use crate::model::asset::AssetKind;
use crate::model::frame::Image;
use crate::model::property::PropertyDefinition;
use crate::model::property::PropertyValue;
use crate::plugin::EntityConverterPlugin;
use crate::rendering::renderer::RenderOutput;
use crate::rendering::skia_utils::GpuContext;

use crate::plugin::PluginCategory;
use crate::plugin::effects::{EffectDefinition, EffectPlugin};
use crate::plugin::evaluator::PropertyEvaluatorRegistry;
use crate::plugin::exporters::{ExportPlugin, ExportSettings};
use crate::plugin::loaders::{
    AssetMetadata, LoadPlugin, LoadPluginError, LoadRepository, LoadRequest, LoadResponse,
};
use crate::plugin::repository::{PluginRegistry, PluginRepository};
use crate::plugin::runtime_native::{
    RuntimeBundleClaim, RuntimeBundleState, RuntimePluginDescriptor, RuntimePluginRegistry,
    RuntimePluginScanReport, discover_manifests, open_bundle, resolve_bundle,
    resolve_manifest_identity,
};

use crate::plugin::traits::{Plugin, PropertyPlugin};
use crate::plugin::{
    DECORATOR_CATEGORY, DECORATOR_PRODUCE_OPERATION, DecoratorPlugin, EFFECT_APPLY_OPERATION,
    EFFECT_CATEGORY, EFFECTOR_CATEGORY, EFFECTOR_PRODUCE_OPERATION, EffectorPlugin,
    OperationDescriptor, STYLE_CATEGORY, STYLE_PRODUCE_OPERATION, StylePlugin,
};

use crate::plugin::effects::{
    BlurEffectPlugin, DilateEffectPlugin, DropShadowEffectPlugin, ErodeEffectPlugin,
    MagnifierEffectPlugin, PixelSorterPlugin, TileEffectPlugin,
};
use crate::plugin::entity_converter::{
    ImageEntityConverterPlugin, ShapeEntityConverterPlugin, SkSLEntityConverterPlugin,
    SolidEntityConverterPlugin, TextEntityConverterPlugin, VideoEntityConverterPlugin,
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
        manager.register_entity_converter_plugin(Arc::new(SolidEntityConverterPlugin::new()));
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

        manager
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
                dynamic_libraries: Vec::new(),
                runtime_plugins: RuntimePluginRegistry::new(),
            }),
        }
    }

    fn read_registry(&self) -> RwLockReadGuard<'_, PluginRegistry> {
        self.inner.read().unwrap_or_else(|poisoned| {
            log::error!("plugin registry read lock was poisoned; recovering committed state");
            poisoned.into_inner()
        })
    }

    fn write_registry(&self) -> RwLockWriteGuard<'_, PluginRegistry> {
        self.inner.write().unwrap_or_else(|poisoned| {
            log::error!("plugin registry write lock was poisoned; recovering committed state");
            poisoned.into_inner()
        })
    }

    pub fn register_effect(&self, plugin: Arc<dyn EffectPlugin>) {
        let mut inner = self.write_registry();
        inner.effect_plugins.register(plugin);
    }

    pub fn register_load_plugin(&self, plugin: Arc<dyn LoadPlugin>) {
        let mut inner = self.write_registry();
        inner.load_plugins.register(plugin);
    }

    pub fn register_export_plugin(&self, plugin: Arc<dyn ExportPlugin>) {
        let mut inner = self.write_registry();
        inner.export_plugins.register(plugin);
    }

    pub fn register_entity_converter_plugin(&self, plugin: Arc<dyn EntityConverterPlugin>) {
        let mut inner = self.write_registry();
        inner.entity_converter_plugins.register(plugin);
    }

    pub fn register_property_plugin(&self, plugin: Arc<dyn PropertyPlugin>) {
        let mut inner = self.write_registry();
        let evaluator_id = plugin.id();
        let evaluator_instance = plugin.get_evaluator_instance();
        inner
            .property_evaluators
            .register(evaluator_id, evaluator_instance);
    }

    pub fn register_effector_plugin(&self, plugin: Arc<dyn EffectorPlugin>) {
        let mut inner = self.write_registry();
        inner.effector_plugins.register(plugin);
    }

    pub fn register_decorator_plugin(&self, plugin: Arc<dyn DecoratorPlugin>) {
        let mut inner = self.write_registry();
        inner.decorator_plugins.register(plugin);
    }

    pub fn register_style_plugin(&self, plugin: Arc<dyn StylePlugin>) {
        let mut inner = self.write_registry();
        inner.style_plugins.register(plugin);
    }

    pub fn get_effector_plugin(&self, id: &str) -> Option<Arc<dyn EffectorPlugin>> {
        let inner = self.read_registry();
        inner.effector_plugins.get(id).cloned()
    }

    pub fn get_decorator_plugin(&self, id: &str) -> Option<Arc<dyn DecoratorPlugin>> {
        let inner = self.read_registry();
        inner.decorator_plugins.get(id).cloned()
    }

    pub fn get_style_plugin(&self, id: &str) -> Option<Arc<dyn StylePlugin>> {
        let inner = self.read_registry();
        inner.style_plugins.get(id).cloned()
    }

    pub fn get_effect_plugin(&self, id: &str) -> Option<Arc<dyn EffectPlugin>> {
        let inner = self.read_registry();
        inner.effect_plugins.get(id).cloned()
    }

    /// Resolves an executable operation descriptor without making Project
    /// loading depend on plugin availability.
    pub fn operation_descriptor(
        &self,
        category: &str,
        component_id: &str,
        operation: &str,
    ) -> Result<OperationDescriptor, LibraryError> {
        let descriptor = match (category, operation) {
            (STYLE_CATEGORY, STYLE_PRODUCE_OPERATION) => self
                .get_style_plugin(component_id)
                .ok_or_else(|| {
                    LibraryError::Plugin(format!(
                        "Operation {category}/{component_id}/{operation} not found"
                    ))
                })?
                .descriptor()
                .map_err(|error| LibraryError::Plugin(error.to_string()))?,
            (EFFECT_CATEGORY, EFFECT_APPLY_OPERATION) => self
                .get_effect_plugin(component_id)
                .ok_or_else(|| {
                    LibraryError::Plugin(format!(
                        "Operation {category}/{component_id}/{operation} not found"
                    ))
                })?
                .descriptor()
                .map_err(|error| LibraryError::Plugin(error.to_string()))?,
            (EFFECTOR_CATEGORY, EFFECTOR_PRODUCE_OPERATION) => self
                .get_effector_plugin(component_id)
                .ok_or_else(|| {
                    LibraryError::Plugin(format!(
                        "Operation {category}/{component_id}/{operation} not found"
                    ))
                })?
                .descriptor()
                .map_err(|error| LibraryError::Plugin(error.to_string()))?,
            (DECORATOR_CATEGORY, DECORATOR_PRODUCE_OPERATION) => self
                .get_decorator_plugin(component_id)
                .ok_or_else(|| {
                    LibraryError::Plugin(format!(
                        "Operation {category}/{component_id}/{operation} not found"
                    ))
                })?
                .descriptor()
                .map_err(|error| LibraryError::Plugin(error.to_string()))?,
            _ => {
                return Err(LibraryError::Plugin(format!(
                    "Operation {category}/{component_id}/{operation} not found"
                )));
            }
        };
        if descriptor.category() != category
            || descriptor.component_id() != component_id
            || descriptor.operation() != operation
        {
            return Err(LibraryError::Plugin(format!(
                "Plugin {component_id} returned a mismatched operation descriptor"
            )));
        }
        Ok(descriptor)
    }

    /// Creates a fully initialized plugin operation Node through its
    /// descriptor. Callers cannot accidentally omit one of its defaults.
    pub fn create_operation_node(
        &self,
        category: &str,
        component_id: &str,
        operation: &str,
    ) -> Result<crate::model::Node, LibraryError> {
        self.operation_descriptor(category, component_id, operation)?
            .create_node()
            .map_err(|error| LibraryError::Plugin(error.to_string()))
    }

    pub fn create_style_operation_node(
        &self,
        component_id: &str,
    ) -> Result<crate::model::Node, LibraryError> {
        self.create_operation_node(STYLE_CATEGORY, component_id, STYLE_PRODUCE_OPERATION)
    }

    pub fn create_effect_operation_node(
        &self,
        component_id: &str,
    ) -> Result<crate::model::Node, LibraryError> {
        self.create_operation_node(EFFECT_CATEGORY, component_id, EFFECT_APPLY_OPERATION)
    }

    pub fn create_effector_operation_node(
        &self,
        component_id: &str,
    ) -> Result<crate::model::Node, LibraryError> {
        self.create_operation_node(EFFECTOR_CATEGORY, component_id, EFFECTOR_PRODUCE_OPERATION)
    }

    pub fn create_decorator_operation_node(
        &self,
        component_id: &str,
    ) -> Result<crate::model::Node, LibraryError> {
        self.create_operation_node(
            DECORATOR_CATEGORY,
            component_id,
            DECORATOR_PRODUCE_OPERATION,
        )
    }

    /// Temporary legacy factory backed by the same descriptor defaults as a
    /// standalone Style operation Node.
    pub fn create_style_instance(
        &self,
        component_id: &str,
    ) -> Result<crate::model::style::StyleInstance, LibraryError> {
        let descriptor =
            self.operation_descriptor(STYLE_CATEGORY, component_id, STYLE_PRODUCE_OPERATION)?;
        Ok(crate::model::style::StyleInstance::new(
            component_id,
            crate::model::property::PropertyMap::from_definitions(descriptor.properties()),
        ))
    }

    /// Evaluates one Style producer through its descriptor-backed render-only
    /// input contract. Every declared value is resolved and validated before
    /// plugin code runs, so missing or invalid authored/keyframed/scalar input
    /// cannot be mistaken for a plugin fallback.
    pub fn evaluate_style_operation(
        &self,
        context: &crate::plugin::FrameEvaluationContext,
        component_id: &str,
        source_id: uuid::Uuid,
        properties: &crate::model::property::PropertyMap,
        eval_time: f64,
    ) -> crate::model::project::EvalOutput<crate::model::frame::entity::StyleConfig> {
        let Some(plugin) = self.get_style_plugin(component_id) else {
            log::warn!("Style plugin {component_id} is unavailable; producing NoOutput");
            return crate::model::project::EvalOutput::NoOutput;
        };
        let descriptor = match self.operation_descriptor(
            STYLE_CATEGORY,
            component_id,
            STYLE_PRODUCE_OPERATION,
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                log::warn!(
                    "Style plugin {component_id} has no valid operation descriptor: {error}; producing NoOutput"
                );
                return crate::model::project::EvalOutput::NoOutput;
            }
        };
        if context
            .evaluate_operation_properties(
                descriptor.properties(),
                properties,
                eval_time,
                &format!("Style {component_id}"),
            )
            .is_none()
        {
            return crate::model::project::EvalOutput::NoOutput;
        }
        plugin
            .evaluate_source(context, source_id, properties, eval_time)
            .map(crate::model::project::EvalOutput::Produced)
            .unwrap_or(crate::model::project::EvalOutput::NoOutput)
    }

    /// Evaluates one standalone Effector producer only after all descriptor
    /// properties have resolved to valid authored/keyframed/scalar values.
    pub fn evaluate_effector_operation(
        &self,
        context: &crate::plugin::FrameEvaluationContext,
        component_id: &str,
        source_id: uuid::Uuid,
        properties: &crate::model::property::PropertyMap,
        eval_time: f64,
    ) -> crate::model::project::EvalOutput<crate::core::ensemble::types::EffectorConfig> {
        let Some(plugin) = self.get_effector_plugin(component_id) else {
            log::warn!("Effector plugin {component_id} is unavailable; producing NoOutput");
            return crate::model::project::EvalOutput::NoOutput;
        };
        let descriptor = match self.operation_descriptor(
            EFFECTOR_CATEGORY,
            component_id,
            EFFECTOR_PRODUCE_OPERATION,
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                log::warn!(
                    "Effector plugin {component_id} has no valid operation descriptor: {error}; producing NoOutput"
                );
                return crate::model::project::EvalOutput::NoOutput;
            }
        };
        if context
            .evaluate_operation_properties(
                descriptor.properties(),
                properties,
                eval_time,
                &format!("Effector {component_id}"),
            )
            .is_none()
        {
            return crate::model::project::EvalOutput::NoOutput;
        }
        plugin
            .evaluate_source(context, source_id, properties, eval_time)
            .map(crate::model::project::EvalOutput::Produced)
            .unwrap_or(crate::model::project::EvalOutput::NoOutput)
    }

    /// Evaluates one standalone Decorator producer only after every
    /// descriptor property has resolved to a valid value.
    pub fn evaluate_decorator_operation(
        &self,
        context: &crate::plugin::FrameEvaluationContext,
        component_id: &str,
        source_id: uuid::Uuid,
        properties: &crate::model::property::PropertyMap,
        eval_time: f64,
    ) -> crate::model::project::EvalOutput<crate::core::ensemble::types::DecoratorConfig> {
        let Some(plugin) = self.get_decorator_plugin(component_id) else {
            log::warn!("Decorator plugin {component_id} is unavailable; producing NoOutput");
            return crate::model::project::EvalOutput::NoOutput;
        };
        let descriptor = match self.operation_descriptor(
            DECORATOR_CATEGORY,
            component_id,
            DECORATOR_PRODUCE_OPERATION,
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                log::warn!(
                    "Decorator plugin {component_id} has no valid operation descriptor: {error}; producing NoOutput"
                );
                return crate::model::project::EvalOutput::NoOutput;
            }
        };
        if context
            .evaluate_operation_properties(
                descriptor.properties(),
                properties,
                eval_time,
                &format!("Decorator {component_id}"),
            )
            .is_none()
        {
            return crate::model::project::EvalOutput::NoOutput;
        }
        plugin
            .evaluate_source(context, source_id, properties, eval_time)
            .map(crate::model::project::EvalOutput::Produced)
            .unwrap_or(crate::model::project::EvalOutput::NoOutput)
    }

    pub fn get_available_effectors(&self) -> Vec<String> {
        let inner = self.read_registry();
        inner
            .effector_plugins
            .values()
            .map(|p| p.id().to_string())
            .collect()
    }

    pub fn get_available_decorators(&self) -> Vec<String> {
        let inner = self.read_registry();
        inner
            .decorator_plugins
            .values()
            .map(|p| p.id().to_string())
            .collect()
    }

    pub fn get_available_styles(&self) -> Vec<String> {
        let inner = self.read_registry();
        inner
            .style_plugins
            .values()
            .map(|p| p.id().to_string())
            .collect()
    }

    pub fn get_effector_properties(&self, id: &str) -> Vec<PropertyDefinition> {
        self.operation_descriptor(EFFECTOR_CATEGORY, id, EFFECTOR_PRODUCE_OPERATION)
            .map(|descriptor| descriptor.properties().to_vec())
            .unwrap_or_default()
    }

    pub fn get_decorator_properties(&self, id: &str) -> Vec<PropertyDefinition> {
        self.operation_descriptor(DECORATOR_CATEGORY, id, DECORATOR_PRODUCE_OPERATION)
            .map(|descriptor| descriptor.properties().to_vec())
            .unwrap_or_default()
    }

    pub fn get_style_properties(&self, id: &str) -> Vec<PropertyDefinition> {
        self.get_style_plugin(id)
            .map(|p| p.properties())
            .unwrap_or_default()
    }

    /// Temporary legacy embedded-instance factory. New authored Effectors use
    /// [`PluginManager::create_effector_operation_node`]. Both factories are
    /// backed by the exact same descriptor defaults.
    pub fn create_effector_instance(
        &self,
        effector_id: &str,
    ) -> Result<crate::model::ensemble::EffectorInstance, LibraryError> {
        let descriptor =
            self.operation_descriptor(EFFECTOR_CATEGORY, effector_id, EFFECTOR_PRODUCE_OPERATION)?;
        Ok(crate::model::ensemble::EffectorInstance::new(
            effector_id,
            crate::model::property::PropertyMap::from_definitions(descriptor.properties()),
        ))
    }

    /// Fills definition-backed defaults that are absent from a known
    /// Effector instance without deleting unknown properties. Unknown plugin
    /// IDs are intentionally left untouched so their Project data is lossless.
    pub fn complete_effector_instance(
        &self,
        instance: &mut crate::model::ensemble::EffectorInstance,
    ) -> bool {
        let Ok(descriptor) = self.operation_descriptor(
            EFFECTOR_CATEGORY,
            &instance.effector_type,
            EFFECTOR_PRODUCE_OPERATION,
        ) else {
            return false;
        };
        for definition in descriptor.properties() {
            if instance.properties.get(definition.name()).is_none() {
                instance.properties.set(
                    definition.name().to_string(),
                    crate::model::property::Property::constant(definition.default_value().clone()),
                );
            }
        }
        true
    }

    /// Evaluates an Effector through the common definition-backed path.
    /// Sparse known instances receive descriptor defaults in a temporary copy;
    /// the authoritative Project and unknown plugin data remain untouched.
    pub fn convert_effector_instance(
        &self,
        context: &crate::plugin::FrameEvaluationContext,
        instance: &crate::model::ensemble::EffectorInstance,
        eval_time: f64,
    ) -> Option<crate::core::ensemble::types::EffectorConfig> {
        let plugin = self.get_effector_plugin(&instance.effector_type)?;
        let descriptor = self
            .operation_descriptor(
                EFFECTOR_CATEGORY,
                &instance.effector_type,
                EFFECTOR_PRODUCE_OPERATION,
            )
            .ok()?;
        let mut resolved = instance.clone();
        for definition in descriptor.properties() {
            if resolved.properties.get(definition.name()).is_none() {
                resolved.properties.set(
                    definition.name().to_string(),
                    crate::model::property::Property::constant(definition.default_value().clone()),
                );
            }
        }
        context.evaluate_operation_properties(
            descriptor.properties(),
            &resolved.properties,
            eval_time,
            &format!("Legacy Effector {}", instance.effector_type),
        )?;
        plugin.evaluate_source(context, resolved.id, &resolved.properties, eval_time)
    }

    /// Temporary legacy embedded-instance factory backed by the same
    /// descriptor defaults as a standalone Decorator operation Node.
    pub fn create_decorator_instance(
        &self,
        decorator_id: &str,
    ) -> Result<crate::model::ensemble::DecoratorInstance, LibraryError> {
        let descriptor = self.operation_descriptor(
            DECORATOR_CATEGORY,
            decorator_id,
            DECORATOR_PRODUCE_OPERATION,
        )?;
        Ok(crate::model::ensemble::DecoratorInstance::new(
            decorator_id,
            crate::model::property::PropertyMap::from_definitions(descriptor.properties()),
        ))
    }

    pub fn complete_decorator_instance(
        &self,
        instance: &mut crate::model::ensemble::DecoratorInstance,
    ) -> bool {
        let Ok(descriptor) = self.operation_descriptor(
            DECORATOR_CATEGORY,
            &instance.decorator_type,
            DECORATOR_PRODUCE_OPERATION,
        ) else {
            return false;
        };
        for definition in descriptor.properties() {
            if instance.properties.get(definition.name()).is_none() {
                instance.properties.set(
                    definition.name().to_string(),
                    crate::model::property::Property::constant(definition.default_value().clone()),
                );
            }
        }
        true
    }

    pub fn convert_decorator_instance(
        &self,
        context: &crate::plugin::FrameEvaluationContext,
        instance: &crate::model::ensemble::DecoratorInstance,
        eval_time: f64,
    ) -> Option<crate::core::ensemble::types::DecoratorConfig> {
        let plugin = self.get_decorator_plugin(&instance.decorator_type)?;
        let descriptor = self
            .operation_descriptor(
                DECORATOR_CATEGORY,
                &instance.decorator_type,
                DECORATOR_PRODUCE_OPERATION,
            )
            .ok()?;
        let mut resolved = instance.clone();
        for definition in descriptor.properties() {
            if resolved.properties.get(definition.name()).is_none() {
                resolved.properties.set(
                    definition.name().to_string(),
                    crate::model::property::Property::constant(definition.default_value().clone()),
                );
            }
        }
        context.evaluate_operation_properties(
            descriptor.properties(),
            &resolved.properties,
            eval_time,
            &format!("Legacy Decorator {}", instance.decorator_type),
        )?;
        plugin.evaluate_source(context, resolved.id, &resolved.properties, eval_time)
    }

    /// Returns descriptors reported by successfully loaded ABI-v1 bundles.
    pub fn get_runtime_plugin_descriptors(&self) -> Vec<RuntimePluginDescriptor> {
        self.read_registry().runtime_plugins.descriptors()
    }

    /// Invokes a descriptor-declared low-bandwidth operation through the
    /// generic JSON control plane. Effector rendering is the first category
    /// integrated into the editor. Frame/resource-heavy categories require a
    /// separately versioned typed extension table and host-owned handles.
    pub fn invoke_runtime_plugin(
        &self,
        category: &str,
        component_id: &str,
        operation: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, LibraryError> {
        self.read_registry()
            .runtime_plugins
            .invoke(category, component_id, operation, payload)
    }

    /// Discovers `ruvie-plugin.toml` bundles in configured runtime directories
    /// and registers bundles that were added since startup.
    pub fn rescan_runtime_plugins<I, P>(&self, paths: I) -> RuntimePluginScanReport
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let mut report = RuntimePluginScanReport::default();
        let mut manifests = Vec::new();
        for path in paths {
            let configured_path = path.as_ref();
            match discover_manifests(configured_path) {
                Ok(mut discovered) => manifests.append(&mut discovered),
                Err(error) => report
                    .failures
                    .push((configured_path.to_path_buf(), error.to_string())),
            }
        }
        manifests.sort();
        manifests.dedup();
        report.discovered_manifests = manifests.len();

        for discovered_manifest in manifests {
            let manifest_path = match resolve_manifest_identity(&discovered_manifest) {
                Ok(manifest_path) => manifest_path,
                Err(error) => {
                    report
                        .failures
                        .push((discovered_manifest, error.to_string()));
                    continue;
                }
            };

            match self
                .read_registry()
                .runtime_plugins
                .manifest_state(&manifest_path)
            {
                RuntimeBundleState::Loaded => {
                    report.already_loaded_bundles.push(manifest_path);
                    continue;
                }
                RuntimeBundleState::InFlight => {
                    report.in_flight_bundles.push(manifest_path);
                    continue;
                }
                RuntimeBundleState::Unseen => {}
            }

            let resolved = match resolve_bundle(&manifest_path) {
                Ok(resolved) => resolved,
                Err(error) => {
                    // A concurrent scan can finish while this scan is resolving
                    // disk state. Prefer the committed/in-flight identity over
                    // a transient file error and never call plugin callbacks.
                    match self
                        .read_registry()
                        .runtime_plugins
                        .manifest_state(&manifest_path)
                    {
                        RuntimeBundleState::Loaded => {
                            report.already_loaded_bundles.push(manifest_path)
                        }
                        RuntimeBundleState::InFlight => {
                            report.in_flight_bundles.push(manifest_path)
                        }
                        RuntimeBundleState::Unseen => {
                            report.failures.push((manifest_path, error.to_string()))
                        }
                    }
                    continue;
                }
            };

            let claim = self
                .write_registry()
                .runtime_plugins
                .claim_bundle(&resolved);
            match claim {
                RuntimeBundleClaim::AlreadyLoaded => {
                    report.already_loaded_bundles.push(manifest_path);
                    continue;
                }
                RuntimeBundleClaim::InFlight => {
                    report.in_flight_bundles.push(manifest_path);
                    continue;
                }
                RuntimeBundleClaim::Claimed => {}
            }

            let pending = match open_bundle(&resolved) {
                Ok(pending) => pending,
                Err(error) => {
                    self.write_registry()
                        .runtime_plugins
                        .cancel_bundle_load(&resolved);
                    report.failures.push((manifest_path, error.to_string()));
                    continue;
                }
            };

            let mut inner = self.write_registry();
            let PluginRegistry {
                runtime_plugins,
                effector_plugins,
                ..
            } = &mut *inner;
            match runtime_plugins.register_bundle(pending, effector_plugins) {
                Ok(registered) => {
                    report.loaded_bundles.push(manifest_path);
                    report.registered_components.extend(registered);
                }
                Err(error) => {
                    runtime_plugins.cancel_bundle_load(&resolved);
                    report.failures.push((manifest_path, error.to_string()));
                }
            }
        }
        report
    }

    /// Convenience form for loading or rescanning one bundle/directory.
    pub fn rescan_runtime_plugin_path<P: AsRef<Path>>(&self, path: P) -> RuntimePluginScanReport {
        self.rescan_runtime_plugins([PathBuf::from(path.as_ref())])
    }

    /// Set the priority order for loader plugins.
    pub fn set_loader_priority(&self, order: Vec<String>) {
        let mut inner = self.write_registry();
        inner.load_plugins.set_priority_order(order);
    }

    /// Get the current loader plugin priority order.
    pub fn get_loader_priority(&self) -> Vec<String> {
        let inner = self.read_registry();
        inner.load_plugins.get_priority_order().to_vec()
    }

    /// Get list of all registered loader plugins (id, name).
    pub fn get_loader_plugins(&self) -> Vec<(String, String)> {
        let inner = self.read_registry();
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
        let inner = self.read_registry();
        if let Some(plugin) = inner.effect_plugins.get(key) {
            debug!("PluginManager: Applying effect '{}'", key);
            plugin.apply(input, params, gpu_context)
        } else {
            log::warn!("Effect '{}' not found", key);
            Ok(input.clone())
        }
    }

    pub fn get_effect_definition(&self, effect_id: &str) -> Option<EffectDefinition> {
        let descriptor =
            match self.operation_descriptor(EFFECT_CATEGORY, effect_id, EFFECT_APPLY_OPERATION) {
                Ok(descriptor) => descriptor,
                Err(error) => {
                    log::warn!("Invalid or unavailable Effect descriptor {effect_id}: {error}");
                    return None;
                }
            };
        Some(EffectDefinition {
            label: descriptor.label().to_string(),
            properties: descriptor.properties().to_vec(),
        })
    }

    pub fn get_default_effect_config(&self, effect_id: &str) -> Option<crate::model::EffectConfig> {
        let descriptor = self
            .operation_descriptor(EFFECT_CATEGORY, effect_id, EFFECT_APPLY_OPERATION)
            .ok()?;
        let props = crate::model::property::PropertyMap::from_definitions(descriptor.properties());
        Some(crate::model::EffectConfig {
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
        let inner = self.read_registry();
        for plugin in inner.load_plugins.values() {
            match plugin.load(request, cache) {
                Ok(response) => return Ok(response),
                Err(LoadPluginError::Unsupported) => {}
                Err(LoadPluginError::Failed(error)) => {
                    log::error!(
                        "Load plugin '{}' failed for request {:?}: {}",
                        plugin.id(),
                        request,
                        error
                    );
                    return Err(error);
                }
            }
        }
        let path = request.path();
        log::error!("No compatible load plugin for request {:?}", request);
        Err(LibraryError::Plugin(format!(
            "No compatible load plugin for path {:?}",
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
        let inner = self.read_registry();
        for plugin in inner.load_plugins.values() {
            match plugin.open(path) {
                Ok(streams) => return Some(streams),
                Err(LoadPluginError::Unsupported) => {}
                Err(LoadPluginError::Failed(error)) => {
                    log::warn!(
                        "Load plugin '{}' failed to inspect path {:?}: {}",
                        plugin.id(),
                        path,
                        error
                    );
                }
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
        let inner = self.read_registry();
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
        let inner = self.read_registry();
        inner
            .export_plugins
            .get(exporter_id)
            .map(|p| p.properties())
    }

    pub fn finish_export(&self, exporter_id: &str, path: &str) -> Result<(), LibraryError> {
        let inner = self.read_registry();
        if let Some(plugin) = inner.export_plugins.get(exporter_id) {
            return plugin.finish_export(path);
        }
        Err(LibraryError::Plugin(format!(
            "Exporter '{}' not found",
            exporter_id
        )))
    }

    /// Loads a Rust-ABI plugin constructor and keeps its library loaded.
    ///
    /// # Safety
    ///
    /// `path` must identify a trusted plugin built with the same Rust toolchain
    /// and the exact trait definition represented by `T`. `symbol` must return
    /// a non-null pointer produced by `Box::into_raw(Box<T>)` and transfer its
    /// sole ownership to this function.
    unsafe fn load_plugin_generic<T: ?Sized + 'static>(
        &self,
        path: &Path,
        symbol: &[u8],
        register: impl FnOnce(&mut PluginRegistry, Arc<T>),
    ) -> Result<(), LibraryError> {
        // SAFETY: The caller guarantees that this is a trusted native plugin;
        // loading it may execute platform-specific initializers.
        let library = unsafe { Library::new(path)? };
        // SAFETY: The caller guarantees the symbol has this exact Rust trait
        // object ABI and was compiled against the same plugin API.
        let constructor: Symbol<unsafe extern "C" fn() -> *mut T> = unsafe { library.get(symbol)? };
        // SAFETY: The constructor contract described above permits one call and
        // transfers ownership of its returned allocation.
        let raw = unsafe { constructor() };
        if raw.is_null() {
            return Err(LibraryError::Plugin(format!(
                "Plugin constructor {} returned null",
                String::from_utf8_lossy(symbol)
            )));
        }
        // SAFETY: The null check and caller contract guarantee `raw` came from
        // Box::into_raw exactly once. Arc takes ownership of the reconstructed Box.
        let plugin = unsafe { Arc::from(Box::from_raw(raw)) };

        let mut inner = self.write_registry();
        register(&mut inner, plugin);
        inner.dynamic_libraries.push(library);
        Ok(())
    }

    pub fn load_property_plugin_from_file<P: AsRef<Path>>(
        &self,
        path: P,
    ) -> Result<(), LibraryError> {
        // SAFETY: Dynamic plugins are a trusted same-toolchain extension point;
        // load_plugin_generic validates the pointer and retains the library.
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
        // SAFETY: Dynamic plugins are a trusted same-toolchain extension point;
        // load_plugin_generic validates the pointer and retains the library.
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
        // SAFETY: Dynamic plugins are a trusted same-toolchain extension point;
        // load_plugin_generic validates the pointer and retains the library.
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
        // SAFETY: Dynamic plugins are a trusted same-toolchain extension point;
        // load_plugin_generic validates the pointer and retains the library.
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
        // SAFETY: Dynamic plugins are a trusted same-toolchain extension point;
        // load_plugin_generic validates the pointer and retains the library.
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
                        std::fs::read_to_string(&config_path).map_err(LibraryError::Io)?;
                    let sksl_content =
                        std::fs::read_to_string(&shader_path).map_err(LibraryError::Io)?;

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
        let inner = self.read_registry();
        Arc::new(inner.property_evaluators.clone())
    }

    pub fn get_entity_converter(&self, kind: &str) -> Option<Arc<dyn EntityConverterPlugin>> {
        let inner = self.read_registry();
        for plugin in inner.entity_converter_plugins.values() {
            if plugin.supports_kind(kind) {
                return Some(plugin.clone());
            }
        }
        None
    }

    pub fn get_inspector_definitions(&self, _kind: &str) -> Vec<PropertyDefinition> {
        // Inspector plugins removed. Return empty or implement static logic if needed.
        Vec::new()
    }

    pub fn get_available_effects(&self) -> Vec<(String, String, String)> {
        let inner = self.read_registry();
        inner
            .effect_plugins
            .plugins
            .values()
            .map(|p| (p.id().to_string(), p.name(), p.category()))
            .collect()
    }

    pub fn get_effect_properties(&self, effect_id: &str) -> Vec<PropertyDefinition> {
        self.operation_descriptor(EFFECT_CATEGORY, effect_id, EFFECT_APPLY_OPERATION)
            .map(|descriptor| descriptor.properties().to_vec())
            .unwrap_or_default()
    }

    pub fn get_available_exporters(&self) -> Vec<(String, String)> {
        let inner = self.read_registry();
        inner
            .export_plugins
            .plugins
            .values()
            .map(|p| (p.id().to_string(), p.name()))
            .collect()
    }

    pub fn get_all_plugins(&self) -> Vec<PluginInfo> {
        let inner = self.read_registry();
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
        for p in inner.effector_plugins.plugins.values() {
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
        for p in inner.decorator_plugins.plugins.values() {
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
        for p in inner.style_plugins.plugins.values() {
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
