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
    RuntimePluginScanReport, RuntimeRegistrationTargets, discover_manifests, open_bundle,
    resolve_bundle, resolve_manifest_identity,
};

use crate::plugin::traits::{Plugin, PropertyPlugin};
use crate::plugin::{
    DECORATOR_APPLY_OPERATION, DECORATOR_CATEGORY, DecoratorPlugin, EFFECT_APPLY_OPERATION,
    EFFECT_CATEGORY, EFFECTOR_APPLY_OPERATION, EFFECTOR_CATEGORY, EffectorPlugin,
    OperationDescriptor, STYLE_APPLY_OPERATION, STYLE_CATEGORY, StylePlugin,
    TRANSFORM_APPLY_OPERATION, TRANSFORM_CATEGORY, TRANSFORM_COMPONENT_ID,
};

fn materialize_validated_operation_properties(
    context: &crate::plugin::FrameEvaluationContext,
    definitions: &[PropertyDefinition],
    properties: &crate::model::property::PropertyMap,
    eval_time: f64,
    operation_label: &str,
) -> Option<crate::model::property::PropertyMap> {
    let evaluated = context.evaluate_operation_properties(
        definitions,
        properties,
        eval_time,
        operation_label,
    )?;
    let mut materialized = crate::model::property::PropertyMap::new();
    for definition in definitions {
        let Some(value) = evaluated.get(definition.name()) else {
            log::error!(
                "{operation_label} validated without materializing declared property {}",
                definition.name()
            );
            return None;
        };
        materialized.set(
            definition.name().to_string(),
            crate::model::property::Property::constant(value.clone()),
        );
    }
    Some(materialized)
}

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
        let replaced = {
            let mut inner = self.write_registry();
            inner.effect_plugins.register(plugin)
        };
        drop(replaced);
    }

    pub fn register_load_plugin(&self, plugin: Arc<dyn LoadPlugin>) {
        let replaced = {
            let mut inner = self.write_registry();
            inner.load_plugins.register(plugin)
        };
        drop(replaced);
    }

    pub fn register_export_plugin(&self, plugin: Arc<dyn ExportPlugin>) {
        let replaced = {
            let mut inner = self.write_registry();
            inner.export_plugins.register(plugin)
        };
        drop(replaced);
    }

    pub fn register_entity_converter_plugin(&self, plugin: Arc<dyn EntityConverterPlugin>) {
        let replaced = {
            let mut inner = self.write_registry();
            inner.entity_converter_plugins.register(plugin)
        };
        drop(replaced);
    }

    pub fn register_property_plugin(&self, plugin: Arc<dyn PropertyPlugin>) {
        let evaluator_id = plugin.id();
        let evaluator_instance = plugin.get_evaluator_instance();
        let replaced = {
            let mut inner = self.write_registry();
            inner
                .property_evaluators
                .register(evaluator_id, evaluator_instance)
        };
        drop(replaced);
    }

    pub fn register_effector_plugin(&self, plugin: Arc<dyn EffectorPlugin>) {
        let replaced = {
            let mut inner = self.write_registry();
            inner.effector_plugins.register(plugin)
        };
        drop(replaced);
    }

    pub fn register_decorator_plugin(&self, plugin: Arc<dyn DecoratorPlugin>) {
        let replaced = {
            let mut inner = self.write_registry();
            inner.decorator_plugins.register(plugin)
        };
        drop(replaced);
    }

    pub fn register_style_plugin(&self, plugin: Arc<dyn StylePlugin>) {
        let replaced = {
            let mut inner = self.write_registry();
            inner.style_plugins.register(plugin)
        };
        drop(replaced);
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
            (STYLE_CATEGORY, STYLE_APPLY_OPERATION) => self
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
            (EFFECTOR_CATEGORY, EFFECTOR_APPLY_OPERATION) => self
                .get_effector_plugin(component_id)
                .ok_or_else(|| {
                    LibraryError::Plugin(format!(
                        "Operation {category}/{component_id}/{operation} not found"
                    ))
                })?
                .descriptor()
                .map_err(|error| LibraryError::Plugin(error.to_string()))?,
            (DECORATOR_CATEGORY, DECORATOR_APPLY_OPERATION) => self
                .get_decorator_plugin(component_id)
                .ok_or_else(|| {
                    LibraryError::Plugin(format!(
                        "Operation {category}/{component_id}/{operation} not found"
                    ))
                })?
                .descriptor()
                .map_err(|error| LibraryError::Plugin(error.to_string()))?,
            (TRANSFORM_CATEGORY, TRANSFORM_APPLY_OPERATION)
                if component_id == TRANSFORM_COMPONENT_ID =>
            {
                crate::plugin::transforms::descriptor()
                    .map_err(|error| LibraryError::Plugin(error.to_string()))?
            }
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
        self.create_operation_node(STYLE_CATEGORY, component_id, STYLE_APPLY_OPERATION)
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
        self.create_operation_node(EFFECTOR_CATEGORY, component_id, EFFECTOR_APPLY_OPERATION)
    }

    pub fn create_decorator_operation_node(
        &self,
        component_id: &str,
    ) -> Result<crate::model::Node, LibraryError> {
        self.create_operation_node(DECORATOR_CATEGORY, component_id, DECORATOR_APPLY_OPERATION)
    }

    /// Creates the native whole-Shape absolute placement operation. Its four
    /// properties are complete at construction; callers may then author a
    /// context-specific position and anchor through normal Node mutations.
    pub fn create_transform_operation_node(&self) -> Result<crate::model::Node, LibraryError> {
        self.create_operation_node(
            TRANSFORM_CATEGORY,
            TRANSFORM_COMPONENT_ID,
            TRANSFORM_APPLY_OPERATION,
        )
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
            STYLE_APPLY_OPERATION,
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                log::warn!(
                    "Style plugin {component_id} has no valid operation descriptor: {error}; producing NoOutput"
                );
                return crate::model::project::EvalOutput::NoOutput;
            }
        };
        let Some(properties) = materialize_validated_operation_properties(
            context,
            descriptor.properties(),
            properties,
            eval_time,
            &format!("Style {component_id}"),
        ) else {
            return crate::model::project::EvalOutput::NoOutput;
        };
        plugin
            .evaluate_source(context, source_id, &properties, eval_time)
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
            EFFECTOR_APPLY_OPERATION,
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                log::warn!(
                    "Effector plugin {component_id} has no valid operation descriptor: {error}; producing NoOutput"
                );
                return crate::model::project::EvalOutput::NoOutput;
            }
        };
        let Some(properties) = materialize_validated_operation_properties(
            context,
            descriptor.properties(),
            properties,
            eval_time,
            &format!("Effector {component_id}"),
        ) else {
            return crate::model::project::EvalOutput::NoOutput;
        };
        plugin
            .evaluate_source(context, source_id, &properties, eval_time)
            .map(crate::model::project::EvalOutput::Produced)
            .unwrap_or(crate::model::project::EvalOutput::NoOutput)
    }

    /// Evaluates the native whole-Shape absolute placement. This stays on the
    /// same descriptor/port/property contract as runtime plugin operations,
    /// while avoiding an ABI call in the render hot path.
    pub fn evaluate_transform_operation(
        &self,
        context: &crate::plugin::FrameEvaluationContext,
        component_id: &str,
        properties: &crate::model::property::PropertyMap,
        eval_time: f64,
    ) -> crate::model::project::EvalOutput<crate::model::frame::transform::Transform> {
        let descriptor = match self.operation_descriptor(
            TRANSFORM_CATEGORY,
            component_id,
            TRANSFORM_APPLY_OPERATION,
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                log::warn!(
                    "Transform operation {component_id} is unavailable: {error}; producing NoOutput"
                );
                return crate::model::project::EvalOutput::NoOutput;
            }
        };
        crate::plugin::transforms::evaluate_source(
            context,
            descriptor.properties(),
            properties,
            eval_time,
        )
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
            DECORATOR_APPLY_OPERATION,
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                log::warn!(
                    "Decorator plugin {component_id} has no valid operation descriptor: {error}; producing NoOutput"
                );
                return crate::model::project::EvalOutput::NoOutput;
            }
        };
        let Some(properties) = materialize_validated_operation_properties(
            context,
            descriptor.properties(),
            properties,
            eval_time,
            &format!("Decorator {component_id}"),
        ) else {
            return crate::model::project::EvalOutput::NoOutput;
        };
        plugin
            .evaluate_source(context, source_id, &properties, eval_time)
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
        self.operation_descriptor(EFFECTOR_CATEGORY, id, EFFECTOR_APPLY_OPERATION)
            .map(|descriptor| descriptor.properties().to_vec())
            .unwrap_or_default()
    }

    pub fn get_decorator_properties(&self, id: &str) -> Vec<PropertyDefinition> {
        self.operation_descriptor(DECORATOR_CATEGORY, id, DECORATOR_APPLY_OPERATION)
            .map(|descriptor| descriptor.properties().to_vec())
            .unwrap_or_default()
    }

    pub fn get_style_properties(&self, id: &str) -> Vec<PropertyDefinition> {
        self.get_style_plugin(id)
            .map(|p| p.properties())
            .unwrap_or_default()
    }

    /// Returns descriptors reported by successfully loaded ABI-v1 bundles.
    pub fn get_runtime_plugin_descriptors(&self) -> Vec<RuntimePluginDescriptor> {
        self.read_registry().runtime_plugins.descriptors()
    }

    /// Creates a runtime property evaluator instance with every descriptor
    /// default materialized. Unknown evaluator IDs remain ordinary Project
    /// data, but cannot be created through this definition-backed factory.
    pub fn create_property_instance(
        &self,
        evaluator_id: &str,
    ) -> Result<crate::model::property::Property, LibraryError> {
        self.read_registry()
            .runtime_plugins
            .create_property(evaluator_id)
    }

    /// Invokes a descriptor-declared low-bandwidth operation through the
    /// generic JSON control plane. Effectors, property evaluators, and
    /// config-only Style/Decorator evaluators have ABI-v1 host adapters.
    /// Frame/resource-heavy categories require a separately versioned typed
    /// extension table and host-owned handles.
    pub fn invoke_runtime_plugin(
        &self,
        category: &str,
        component_id: &str,
        operation: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, LibraryError> {
        let component = {
            let inner = self.read_registry();
            inner.runtime_plugins.component(category, component_id)?
        };
        component.invoke(operation, payload)
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
                effect_plugins,
                load_plugins,
                effector_plugins,
                decorator_plugins,
                style_plugins,
                property_evaluators,
                ..
            } = &mut *inner;
            match runtime_plugins.register_bundle(
                pending,
                RuntimeRegistrationTargets {
                    effect_plugins,
                    load_plugins,
                    effector_plugins,
                    decorator_plugins,
                    style_plugins,
                    property_evaluators,
                },
            ) {
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
        let plugin = {
            let inner = self.read_registry();
            inner.effect_plugins.get(key).cloned()
        };
        if let Some(plugin) = plugin {
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

    /// Load a resource (image or video frame).
    pub fn load_resource(
        &self,
        request: &LoadRequest,
        cache: &CacheManager,
    ) -> Result<LoadResponse, LibraryError> {
        let plugins = {
            let inner = self.read_registry();
            inner.load_plugins.values().cloned().collect::<Vec<_>>()
        };
        for plugin in plugins {
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
    pub fn get_metadata(&self, path: &str) -> Result<Option<AssetMetadata>, LibraryError> {
        Ok(self
            .get_available_streams(path)?
            .and_then(|streams| streams.into_iter().next()))
    }

    /// Get all available streams/resources from a file. `Ok(None)` means every
    /// loader declined the path; a loader that claims the path but cannot open
    /// it returns its exact failure instead of being disguised as unsupported.
    pub fn get_available_streams(
        &self,
        path: &str,
    ) -> Result<Option<Vec<AssetMetadata>>, LibraryError> {
        let plugins = {
            let inner = self.read_registry();
            inner.load_plugins.values().cloned().collect::<Vec<_>>()
        };
        for plugin in plugins {
            match plugin.open(path) {
                Ok(streams) => return Ok(Some(streams)),
                Err(LoadPluginError::Unsupported) => {}
                Err(LoadPluginError::Failed(error)) => {
                    log::error!(
                        "Load plugin '{}' failed to inspect path {:?}: {}",
                        plugin.id(),
                        path,
                        error
                    );
                    return Err(error);
                }
            }
        }
        Ok(None)
    }

    pub fn probe_asset_kind(&self, path: &str) -> Result<AssetKind, LibraryError> {
        Ok(self
            .get_metadata(path)?
            .map(|m| m.kind)
            .unwrap_or(AssetKind::Other))
    }

    pub fn get_duration(&self, path: &str) -> Result<Option<f64>, LibraryError> {
        Ok(self.get_metadata(path)?.and_then(|m| m.duration))
    }

    pub fn get_fps(&self, path: &str) -> Result<Option<f64>, LibraryError> {
        Ok(self.get_metadata(path)?.and_then(|m| m.fps))
    }

    pub fn get_dimensions(&self, path: &str) -> Result<Option<(u32, u32)>, LibraryError> {
        Ok(self
            .get_metadata(path)?
            .and_then(|m| match (m.width, m.height) {
                (Some(w), Some(h)) => Some((w, h)),
                _ => None,
            }))
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
        register: impl FnOnce(&mut PluginRegistry, Arc<T>) -> Option<Arc<T>>,
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

        let replaced = {
            let mut inner = self.write_registry();
            let replaced = register(&mut inner, plugin);
            inner.dynamic_libraries.push(library);
            replaced
        };
        drop(replaced);
        Ok(())
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
                |inner, plugin| inner.effect_plugins.register(plugin),
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
                |inner, plugin| inner.load_plugins.register(plugin),
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
                |inner, plugin| inner.export_plugins.register(plugin),
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
                |inner, plugin| inner.entity_converter_plugins.register(plugin),
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

#[cfg(test)]
mod tests {
    use super::*;
    use ordered_float::OrderedFloat;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use crate::model::frame::color::Color;
    use crate::model::frame::draw_type::DrawStyle;
    use crate::model::frame::entity::StyleConfig;
    use crate::model::project::{Composition, EvalOutput, Project};
    use crate::model::property::{Property, PropertyMap, PropertyUiType, PropertyValue};
    use crate::plugin::{
        FrameEvaluationContext, OperationDescriptor, OperationDescriptorError, PropertyEvaluator,
        PropertyPlugin,
    };

    #[test]
    fn every_bundled_property_definition_is_valid_and_operations_are_materialized() {
        fn check_definitions(
            scope: &str,
            definitions: &[PropertyDefinition],
            failures: &mut Vec<String>,
        ) {
            for definition in definitions {
                if let Err(error) = definition.validate_definition() {
                    failures.push(format!("{scope} property '{}': {error}", definition.name()));
                }
            }
        }

        let manager = PluginManager::default();
        let sksl_directory =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets/plugins/sksl");
        manager
            .load_sksl_plugins_from_directory(&sksl_directory)
            .expect("bundled SkSL directory should be readable");

        let expected_sksl_ids = std::fs::read_dir(&sksl_directory)
            .expect("bundled SkSL directory should exist")
            .filter_map(Result::ok)
            .map(|entry| entry.path().join("config.toml"))
            .filter(|path| path.is_file())
            .map(|path| {
                let source = std::fs::read_to_string(&path)
                    .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
                toml::from_str::<crate::plugin::effects::sksl_plugin::SkslPluginConfig>(&source)
                    .unwrap_or_else(|error| panic!("cannot parse {}: {error}", path.display()))
                    .id
            })
            .collect::<HashSet<_>>();

        let mut failures = Vec::new();
        check_definitions(
            "Clip timing",
            crate::model::Clip::timing_property_definitions(),
            &mut failures,
        );
        check_definitions(
            "native Time Modulo",
            crate::model::ValueContent::TimeModulo.property_definitions(),
            &mut failures,
        );
        let transform_definitions = crate::plugin::transforms::property_definitions();
        check_definitions("native Transform", &transform_definitions, &mut failures);
        let mut operation_contracts = Vec::<(&'static str, String, usize)>::new();
        operation_contracts.push((
            TRANSFORM_CATEGORY,
            TRANSFORM_COMPONENT_ID.to_string(),
            transform_definitions.len(),
        ));
        let registered_effect_ids;
        {
            let registry = manager.read_registry();
            registered_effect_ids = registry
                .effect_plugins
                .values()
                .map(|plugin| plugin.id().to_string())
                .collect::<HashSet<_>>();

            for plugin in registry.entity_converter_plugins.values() {
                let definitions = plugin.get_property_definitions(1920, 1080, 640, 360);
                check_definitions(
                    &format!("converter {}", plugin.id()),
                    &definitions,
                    &mut failures,
                );
            }
            for kind in ["video", "image", "text", "shape", "solid", "sksl"] {
                if !registry
                    .entity_converter_plugins
                    .values()
                    .any(|plugin| plugin.supports_kind(kind))
                {
                    failures.push(format!("built-in converter kind {kind} is not registered"));
                }
            }
            for plugin in registry.export_plugins.values() {
                let definitions = plugin.properties();
                check_definitions(
                    &format!("exporter {}", plugin.id()),
                    &definitions,
                    &mut failures,
                );
            }

            for plugin in registry.effect_plugins.values() {
                let definitions = plugin.properties();
                check_definitions(
                    &format!("effect {}", plugin.id()),
                    &definitions,
                    &mut failures,
                );
                operation_contracts.push(("effect", plugin.id().to_string(), definitions.len()));
            }
            for plugin in registry.effector_plugins.values() {
                let definitions = plugin.properties();
                check_definitions(
                    &format!("effector {}", plugin.id()),
                    &definitions,
                    &mut failures,
                );
                operation_contracts.push(("effector", plugin.id().to_string(), definitions.len()));
            }
            for plugin in registry.decorator_plugins.values() {
                let definitions = plugin.properties();
                check_definitions(
                    &format!("decorator {}", plugin.id()),
                    &definitions,
                    &mut failures,
                );
                operation_contracts.push(("decorator", plugin.id().to_string(), definitions.len()));
            }
            for plugin in registry.style_plugins.values() {
                match plugin.descriptor() {
                    Ok(descriptor) => {
                        check_definitions(
                            &format!("style {}", plugin.id()),
                            descriptor.properties(),
                            &mut failures,
                        );
                        operation_contracts.push((
                            "style",
                            plugin.id().to_string(),
                            descriptor.properties().len(),
                        ));
                    }
                    Err(error) => failures.push(format!(
                        "style {} descriptor is invalid: {error}",
                        plugin.id()
                    )),
                }
            }
        }

        for missing in expected_sksl_ids.difference(&registered_effect_ids) {
            failures.push(format!("bundled SkSL effect {missing} was not registered"));
        }

        for (category, component_id, expected_property_count) in operation_contracts {
            let result = match category {
                "effect" => manager.create_effect_operation_node(&component_id),
                "effector" => manager.create_effector_operation_node(&component_id),
                "decorator" => manager.create_decorator_operation_node(&component_id),
                "style" => manager.create_style_operation_node(&component_id),
                TRANSFORM_CATEGORY => manager.create_transform_operation_node(),
                unknown => {
                    failures.push(format!(
                        "operation contract uses unknown category {unknown} for {component_id}"
                    ));
                    continue;
                }
            };
            match result {
                Ok(node) => {
                    let actual_property_count = node.properties().iter().count();
                    if actual_property_count != expected_property_count {
                        failures.push(format!(
                            "{category} {component_id} initialized {actual_property_count} of {expected_property_count} properties"
                        ));
                    }
                }
                Err(error) => failures.push(format!(
                    "{category} {component_id} cannot create a complete Node: {error}"
                )),
            }
        }

        assert!(
            failures.is_empty(),
            "bundled property definition failures:\n{}",
            failures.join("\n")
        );
    }

    struct StatefulEvaluator {
        evaluations: Arc<AtomicUsize>,
    }

    impl PropertyEvaluator for StatefulEvaluator {
        fn evaluate(
            &self,
            _property: &Property,
            _time: f64,
            _context: &crate::plugin::EvaluationContext,
        ) -> PropertyValue {
            let value = self.evaluations.fetch_add(1, Ordering::SeqCst) + 1;
            PropertyValue::Number(OrderedFloat(value as f64))
        }
    }

    struct StatefulPropertyPlugin {
        evaluations: Arc<AtomicUsize>,
    }

    impl Plugin for StatefulPropertyPlugin {
        fn id(&self) -> &str {
            "stateful-test"
        }

        fn name(&self) -> String {
            "Stateful Test".to_string()
        }

        fn category(&self) -> String {
            "Tests".to_string()
        }

        fn version(&self) -> (u32, u32, u32) {
            (0, 1, 0)
        }
    }

    impl PropertyPlugin for StatefulPropertyPlugin {
        fn get_evaluator_instance(&self) -> Arc<dyn PropertyEvaluator> {
            Arc::new(StatefulEvaluator {
                evaluations: Arc::clone(&self.evaluations),
            })
        }
    }

    struct EvaluatedValueStylePlugin;

    const REENTRANT_EFFECT_ID: &str = "reentrant-drop-effect";

    struct ReentrantDropEffect {
        manager: Arc<PluginManager>,
        callback_completed: Arc<std::sync::atomic::AtomicBool>,
    }

    impl Drop for ReentrantDropEffect {
        fn drop(&mut self) {
            let replacement_is_visible = self
                .manager
                .get_effect_plugin(REENTRANT_EFFECT_ID)
                .is_some();
            self.callback_completed
                .store(replacement_is_visible, Ordering::SeqCst);
        }
    }

    struct ReplacementEffect;

    struct MetadataProbeLoader {
        id: &'static str,
        failure: Option<&'static str>,
    }

    impl Plugin for MetadataProbeLoader {
        fn id(&self) -> &str {
            self.id
        }

        fn name(&self) -> String {
            "Metadata Probe Loader".to_string()
        }

        fn category(&self) -> String {
            "Tests".to_string()
        }

        fn version(&self) -> (u32, u32, u32) {
            (0, 1, 0)
        }
    }

    impl LoadPlugin for MetadataProbeLoader {
        fn open(&self, path: &str) -> crate::plugin::loaders::LoadPluginResult<Vec<AssetMetadata>> {
            match self.failure {
                Some(cause) => Err(LoadPluginError::Failed(LibraryError::Plugin(format!(
                    "{cause}: {path}"
                )))),
                None => Err(LoadPluginError::Unsupported),
            }
        }

        fn load(
            &self,
            _request: &LoadRequest,
            _cache: &CacheManager,
        ) -> crate::plugin::loaders::LoadPluginResult<LoadResponse> {
            Err(LoadPluginError::Unsupported)
        }
    }

    macro_rules! impl_reentrant_test_effect {
        ($effect:ty) => {
            impl Plugin for $effect {
                fn id(&self) -> &str {
                    REENTRANT_EFFECT_ID
                }

                fn name(&self) -> String {
                    "Reentrant Drop Effect".to_string()
                }

                fn category(&self) -> String {
                    "Tests".to_string()
                }

                fn version(&self) -> (u32, u32, u32) {
                    (0, 1, 0)
                }
            }

            impl EffectPlugin for $effect {
                fn apply(
                    &self,
                    input: &crate::rendering::renderer::RenderOutput,
                    _params: &HashMap<String, PropertyValue>,
                    _gpu_context: Option<&mut crate::rendering::skia_utils::GpuContext>,
                ) -> Result<crate::rendering::renderer::RenderOutput, LibraryError> {
                    Ok(input.clone())
                }

                fn properties(&self) -> Vec<PropertyDefinition> {
                    Vec::new()
                }
            }
        };
    }

    impl_reentrant_test_effect!(ReentrantDropEffect);
    impl_reentrant_test_effect!(ReplacementEffect);

    impl Plugin for EvaluatedValueStylePlugin {
        fn id(&self) -> &str {
            "evaluated-value-style"
        }

        fn name(&self) -> String {
            "Evaluated Value Style".to_string()
        }

        fn category(&self) -> String {
            "Tests".to_string()
        }

        fn version(&self) -> (u32, u32, u32) {
            (0, 1, 0)
        }
    }

    impl StylePlugin for EvaluatedValueStylePlugin {
        fn descriptor(&self) -> Result<OperationDescriptor, OperationDescriptorError> {
            OperationDescriptor::style(
                self.id(),
                self.name(),
                vec![PropertyDefinition::new(
                    "value",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 100.0,
                        step: 1.0,
                        suffix: String::new(),
                        min_hard_limit: true,
                        max_hard_limit: true,
                    },
                    "Value",
                    PropertyValue::from(0.0),
                )],
            )
        }

        fn evaluate_source(
            &self,
            context: &FrameEvaluationContext,
            source_id: uuid::Uuid,
            properties: &PropertyMap,
            eval_time: f64,
        ) -> Option<StyleConfig> {
            Some(StyleConfig {
                id: source_id,
                style: DrawStyle::Fill {
                    color: Color::white(),
                    offset: context.evaluate_number(properties, "value", eval_time, -1.0),
                },
            })
        }
    }

    #[test]
    fn operation_validation_materializes_stateful_values_before_plugin_evaluation() {
        let evaluations = Arc::new(AtomicUsize::new(0));
        let manager = PluginManager::default();
        manager.register_property_plugin(Arc::new(StatefulPropertyPlugin {
            evaluations: Arc::clone(&evaluations),
        }));
        manager.register_style_plugin(Arc::new(EvaluatedValueStylePlugin));
        let mut node = manager
            .create_style_operation_node("evaluated-value-style")
            .expect("test Style descriptor creates a Node");
        node.set_property(
            "value".to_string(),
            Property {
                evaluator: "stateful-test".to_string(),
                properties: std::collections::HashMap::new(),
            },
        )
        .expect("descriptor initializes the value property");

        let (composition, track) = Composition::new("Main", 640, 360, 30.0, 1.0);
        let composition_id = composition.id;
        let mut project = Project::new("stateful operation property");
        project.add_track(track);
        project.add_composition(composition);
        let composition = project
            .get_composition(composition_id)
            .expect("test composition exists");
        let property_evaluators = manager.get_property_evaluators();
        let context = FrameEvaluationContext {
            project: &project,
            composition,
            property_evaluators: &property_evaluators,
            plugin_manager: &manager,
            resolved_inputs: None,
        };

        let output = manager.evaluate_style_operation(
            &context,
            "evaluated-value-style",
            node.id,
            node.properties(),
            0.0,
        );
        let EvalOutput::Produced(StyleConfig {
            style: DrawStyle::Fill { offset, .. },
            ..
        }) = output
        else {
            panic!("valid stateful property should produce a Style config")
        };
        assert_eq!(offset, 1.0);
        assert_eq!(
            evaluations.load(Ordering::SeqCst),
            1,
            "validation must not invoke an authored evaluator again inside plugin code"
        );
    }

    #[test]
    fn replacing_effect_drops_old_plugin_after_manager_write_lock_is_released()
    -> Result<(), Box<dyn std::error::Error>> {
        let manager = Arc::new(PluginManager::new());
        let callback_completed = Arc::new(std::sync::atomic::AtomicBool::new(false));
        manager.register_effect(Arc::new(ReentrantDropEffect {
            manager: Arc::clone(&manager),
            callback_completed: Arc::clone(&callback_completed),
        }));

        let (completed_tx, completed_rx) = std::sync::mpsc::channel();
        let worker_manager = Arc::clone(&manager);
        let worker = std::thread::spawn(move || {
            worker_manager.register_effect(Arc::new(ReplacementEffect));
            completed_tx.send(()).map_err(|error| error.to_string())
        });

        completed_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        let worker_result = worker
            .join()
            .map_err(|_| std::io::Error::other("replacement worker panicked"))?;
        worker_result.map_err(std::io::Error::other)?;
        assert!(
            callback_completed.load(Ordering::SeqCst),
            "the old plugin destructor must be able to read the committed replacement"
        );
        Ok(())
    }

    #[test]
    fn metadata_probe_preserves_claimed_loader_failure() -> Result<(), Box<dyn std::error::Error>> {
        let manager = PluginManager::new();
        manager.register_load_plugin(Arc::new(MetadataProbeLoader {
            id: "declining-metadata-loader",
            failure: None,
        }));
        assert!(
            manager
                .get_available_streams("/fixtures/custom.asset")?
                .is_none(),
            "all Unsupported responses must remain an unclaimed path"
        );

        manager.register_load_plugin(Arc::new(MetadataProbeLoader {
            id: "claiming-metadata-loader",
            failure: Some("fixture header is truncated"),
        }));
        let Err(error) = manager.get_available_streams("/fixtures/custom.asset") else {
            return Err(std::io::Error::other(
                "a claimed metadata failure must not become Ok(None)",
            )
            .into());
        };
        let message = error.to_string();
        assert!(message.contains("fixture header is truncated"));
        assert!(message.contains("/fixtures/custom.asset"));
        assert!(!message.contains("No compatible load plugin"));
        Ok(())
    }
}
