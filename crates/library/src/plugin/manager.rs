//! Plugin manager for registering, loading, and accessing plugins.

mod bundled;
mod dynamic_loading;
mod effects;
mod ensemble_operations;
mod registration;
mod runtime_plugins;
mod shape_operations;

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use crate::cache::CacheManager;
use crate::error::LibraryError;
use crate::model::asset::AssetKind;
use crate::model::property::{ColorValue, PropertyDefinition, PropertyUiType, PropertyValue};
use crate::plugin::EntityConverterPlugin;
use crate::util::local_file::DirectRegularFile;

use crate::plugin::PluginCategory;
use crate::plugin::effects::EffectPlugin;
use crate::plugin::evaluator::PropertyEvaluatorRegistry;
use crate::plugin::exporters::{ExportPlugin, ExportSettings};
use crate::plugin::loaders::{
    AssetMetadata, LoadPluginError, LoadRepository, LoadRequest, LoadResponse,
};
use crate::plugin::repository::{PluginRegistry, PluginRepository};
use crate::plugin::runtime_native::RuntimePluginRegistry;

use crate::plugin::{
    DECORATOR_APPLY_OPERATION, DECORATOR_CATEGORY, DecoratorPlugin, EFFECT_APPLY_OPERATION,
    EFFECT_CATEGORY, EFFECTOR_APPLY_OPERATION, EFFECTOR_CATEGORY, EffectorPlugin,
    IMAGE_OPACITY_STYLE_COMPONENT_ID, IMAGE_TRANSFORM_COMPONENT_ID, OperationDescriptor,
    PATH_EFFECT_APPLY_OPERATION, PATH_EFFECT_CATEGORY, PathEffectPlugin, Plugin,
    SHAPE_TRANSFORM_COMPONENT_ID, STYLE_APPLY_OPERATION, STYLE_CATEGORY, StylePlugin,
    TRANSFORM_APPLY_OPERATION, TRANSFORM_CATEGORY,
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

/// Validate values that have already been sampled by an authoring/runtime
/// boundary. Graph evaluation and compiled Module evaluation both enter
/// operation implementations through this one typed contract.
fn validated_operation_values(
    definitions: &[PropertyDefinition],
    values: &HashMap<String, PropertyValue>,
    operation_label: &str,
) -> Option<HashMap<String, PropertyValue>> {
    if definitions.len() != values.len() {
        log::warn!("{operation_label} received undeclared evaluated properties");
        return None;
    }
    let mut validated = HashMap::with_capacity(definitions.len());
    for definition in definitions {
        let Some(value) = values.get(definition.name()) else {
            log::warn!(
                "{operation_label} property {} is missing from evaluated inputs",
                definition.name()
            );
            return None;
        };
        let value = if matches!(definition.ui_type(), PropertyUiType::ColorValue)
            && let PropertyValue::Color(color) = value
        {
            PropertyValue::ColorValue(ColorValue::from_straight_srgba8(color))
        } else {
            value.clone()
        };
        if let Err(error) = definition.validate_value(&value) {
            log::warn!(
                "{operation_label} property {} has an invalid evaluated value: {error}",
                definition.name()
            );
            return None;
        }
        validated.insert(definition.name().to_string(), value);
    }
    Some(validated)
}

fn evaluated_operation<'a>(
    values: &'a HashMap<String, PropertyValue>,
    eval_time: f64,
    fps: f64,
    resolution: (u64, u64),
) -> Option<crate::plugin::EvaluatedOperation<'a>> {
    if !eval_time.is_finite()
        || !fps.is_finite()
        || fps <= 0.0
        || resolution.0 == 0
        || resolution.1 == 0
    {
        return None;
    }
    Some(crate::plugin::EvaluatedOperation::new(
        values, eval_time, fps, resolution,
    ))
}

/// Main plugin manager.
pub struct PluginManager {
    inner: RwLock<PluginRegistry>,
    bundled_operations: bundled::BundledOperationInventory,
    render_revision: AtomicU64,
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
                path_effect_plugins: PluginRepository::new(),
                property_evaluators: PropertyEvaluatorRegistry::new(),
                dynamic_libraries: Vec::new(),
                runtime_plugins: RuntimePluginRegistry::new(),
            }),
            bundled_operations: bundled::BundledOperationInventory::default(),
            render_revision: AtomicU64::new(1),
        }
    }

    /// Monotonic invalidation token for every runtime mutation that can alter
    /// frame evaluation, loading, effects, or color ingress.
    pub fn render_revision(&self) -> u64 {
        self.render_revision.load(Ordering::Acquire)
    }

    fn bump_render_revision(&self) {
        self.render_revision.fetch_add(1, Ordering::Release);
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

    pub fn get_path_effect_plugin(&self, id: &str) -> Option<Arc<dyn PathEffectPlugin>> {
        let inner = self.read_registry();
        inner.path_effect_plugins.get(id).cloned()
    }

    pub fn get_effect_plugin(&self, id: &str) -> Option<Arc<dyn EffectPlugin>> {
        let inner = self.read_registry();
        inner.effect_plugins.get(id).cloned()
    }

    fn get_export_plugin(&self, id: &str) -> Option<Arc<dyn ExportPlugin>> {
        let inner = self.read_registry();
        inner.export_plugins.get(id).cloned()
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
            (PATH_EFFECT_CATEGORY, PATH_EFFECT_APPLY_OPERATION) => self
                .get_path_effect_plugin(component_id)
                .ok_or_else(|| {
                    LibraryError::Plugin(format!(
                        "Operation {category}/{component_id}/{operation} not found"
                    ))
                })?
                .descriptor()
                .map_err(|error| LibraryError::Plugin(error.to_string()))?,
            (TRANSFORM_CATEGORY, TRANSFORM_APPLY_OPERATION) => match component_id {
                SHAPE_TRANSFORM_COMPONENT_ID => crate::plugin::transforms::shape_descriptor(),
                IMAGE_TRANSFORM_COMPONENT_ID => crate::plugin::transforms::image_descriptor(),
                _ => {
                    return Err(LibraryError::Plugin(format!(
                        "Operation {category}/{component_id}/{operation} not found"
                    )));
                }
            }
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
        self.create_operation_node(STYLE_CATEGORY, component_id, STYLE_APPLY_OPERATION)
    }

    pub fn create_image_opacity_style_operation_node(
        &self,
    ) -> Result<crate::model::Node, LibraryError> {
        self.create_style_operation_node(IMAGE_OPACITY_STYLE_COMPONENT_ID)
    }

    /// Resolves the native Image Opacity value through descriptor-backed
    /// properties, scalar wires, keyframes, and Expression evaluation.
    pub fn evaluate_image_opacity_style_operation(
        &self,
        context: &crate::plugin::FrameEvaluationContext,
        properties: &crate::model::property::PropertyMap,
        eval_time: f64,
    ) -> crate::model::project::EvalOutput<f64> {
        let descriptor = match self.operation_descriptor(
            STYLE_CATEGORY,
            IMAGE_OPACITY_STYLE_COMPONENT_ID,
            STYLE_APPLY_OPERATION,
        ) {
            Ok(descriptor) => descriptor,
            Err(error) => {
                log::warn!("Image Opacity descriptor is unavailable: {error}; producing NoOutput");
                return crate::model::project::EvalOutput::NoOutput;
            }
        };
        let Some(evaluated) = context.evaluate_operation_properties(
            descriptor.properties(),
            properties,
            eval_time,
            "Image Opacity",
        ) else {
            return crate::model::project::EvalOutput::NoOutput;
        };
        let Some(opacity) = evaluated
            .get("opacity")
            .and_then(|value| value.get_as::<f64>())
            .filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
        else {
            log::warn!("Image Opacity evaluated outside [0, 1]; producing NoOutput");
            return crate::model::project::EvalOutput::NoOutput;
        };
        crate::model::project::EvalOutput::Produced(opacity)
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

    pub fn create_path_effect_operation_node(
        &self,
        component_id: &str,
    ) -> Result<crate::model::Node, LibraryError> {
        self.create_operation_node(
            PATH_EFFECT_CATEGORY,
            component_id,
            PATH_EFFECT_APPLY_OPERATION,
        )
    }

    /// Creates the native whole-Shape absolute placement operation. Its four
    /// properties are complete at construction; callers may then author a
    /// context-specific position and anchor through normal Node mutations.
    pub fn create_shape_transform_operation_node(
        &self,
    ) -> Result<crate::model::Node, LibraryError> {
        self.create_operation_node(
            TRANSFORM_CATEGORY,
            SHAPE_TRANSFORM_COMPONENT_ID,
            TRANSFORM_APPLY_OPERATION,
        )
    }

    /// Creates the native whole-Image placement operation. Its child may be
    /// any Image-producing subtree; descendant identities remain unchanged.
    pub fn create_image_transform_operation_node(
        &self,
    ) -> Result<crate::model::Node, LibraryError> {
        self.create_operation_node(
            TRANSFORM_CATEGORY,
            IMAGE_TRANSFORM_COMPONENT_ID,
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

    pub fn get_available_effectors(&self) -> Vec<String> {
        let inner = self.read_registry();
        inner.effector_plugins.plugins.keys().cloned().collect()
    }

    pub fn get_available_decorators(&self) -> Vec<String> {
        let inner = self.read_registry();
        inner.decorator_plugins.plugins.keys().cloned().collect()
    }

    pub fn get_available_styles(&self) -> Vec<String> {
        let inner = self.read_registry();
        inner.style_plugins.plugins.keys().cloned().collect()
    }

    pub fn get_available_path_effects(&self) -> Vec<String> {
        let inner = self.read_registry();
        inner.path_effect_plugins.plugins.keys().cloned().collect()
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

    pub fn get_path_effect_properties(&self, id: &str) -> Vec<PropertyDefinition> {
        self.operation_descriptor(PATH_EFFECT_CATEGORY, id, PATH_EFFECT_APPLY_OPERATION)
            .map(|descriptor| descriptor.properties().to_vec())
            .unwrap_or_default()
    }

    /// Set the priority order for loader plugins.
    pub fn set_loader_priority(&self, order: Vec<String>) {
        let mut inner = self.write_registry();
        inner.load_plugins.set_priority_order(order);
        self.bump_render_revision();
    }

    /// Get the current loader plugin priority order.
    pub fn get_loader_priority(&self) -> Vec<String> {
        let inner = self.read_registry();
        inner.load_plugins.get_priority_order().to_vec()
    }

    /// Get list of all registered loader plugins (id, name).
    pub fn get_loader_plugins(&self) -> Vec<(String, String)> {
        let plugins = {
            let inner = self.read_registry();
            inner.load_plugins.snapshot()
        };
        plugins
            .into_iter()
            .map(|(id, plugin)| (id, plugin.name()))
            .collect()
    }

    /// Load a resource (image or video frame).
    pub fn load_resource(
        &self,
        request: &LoadRequest,
        cache: &CacheManager,
    ) -> Result<LoadResponse, LibraryError> {
        // This is the automatic Project/Preview boundary. Do not move this
        // policy into `get_available_streams`: explicit import/relink probing
        // is a separate editor action. Keep the verified handle alive across
        // dispatch so FIFOs/devices and final-component symlinks are rejected
        // before any built-in or third-party loader callback can run.
        let _locator_guard = DirectRegularFile::open(request.path()).map_err(|error| {
            LibraryError::Plugin(format!(
                "Automatic media load rejected locator {:?}: {error}",
                request.path()
            ))
        })?;
        let plugins = {
            let inner = self.read_registry();
            inner.load_plugins.snapshot()
        };
        for (plugin_id, plugin) in plugins {
            match plugin.load(request, cache) {
                Ok(response) => return Ok(response),
                Err(LoadPluginError::Unsupported) => {}
                Err(LoadPluginError::Failed(error)) => {
                    log::error!(
                        "Load plugin '{}' failed for request {:?}: {}",
                        plugin_id,
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
            inner.load_plugins.snapshot()
        };
        for (plugin_id, plugin) in plugins {
            match plugin.open(path) {
                Ok(streams) => return Ok(Some(streams)),
                Err(LoadPluginError::Unsupported) => {}
                Err(LoadPluginError::Failed(error)) => {
                    log::error!(
                        "Load plugin '{}' failed to inspect path {:?}: {}",
                        plugin_id,
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

    pub fn export_frame(
        &self,
        exporter_id: &str,
        path: &str,
        frame: &crate::plugin::ExportFrame,
        settings: &ExportSettings,
    ) -> Result<(), LibraryError> {
        self.get_export_plugin(exporter_id)
            .ok_or_else(|| LibraryError::Plugin(format!("Exporter '{exporter_id}' not found")))?
            .export_frame(path, frame, settings)
    }

    pub fn get_export_plugin_properties(
        &self,
        exporter_id: &str,
    ) -> Option<Vec<PropertyDefinition>> {
        self.get_export_plugin(exporter_id)
            .map(|plugin| plugin.properties())
    }

    pub fn finish_export(
        &self,
        exporter_id: &str,
        path: &str,
        settings: &ExportSettings,
    ) -> Result<(), LibraryError> {
        self.get_export_plugin(exporter_id)
            .ok_or_else(|| LibraryError::Plugin(format!("Exporter '{exporter_id}' not found")))?
            .finish_export(path, settings)
    }

    pub fn get_property_evaluators(&self) -> Arc<PropertyEvaluatorRegistry> {
        let inner = self.read_registry();
        Arc::new(inner.property_evaluators.clone())
    }

    pub fn get_entity_converter(&self, kind: &str) -> Option<Arc<dyn EntityConverterPlugin>> {
        let plugins = {
            let inner = self.read_registry();
            inner
                .entity_converter_plugins
                .values()
                .cloned()
                .collect::<Vec<_>>()
        };
        for plugin in plugins {
            if plugin.supports_kind(kind) {
                return Some(plugin);
            }
        }
        None
    }

    pub fn get_inspector_definitions(&self, _kind: &str) -> Vec<PropertyDefinition> {
        // Inspector plugins removed. Return empty or implement static logic if needed.
        Vec::new()
    }

    pub fn get_available_effects(&self) -> Vec<(String, String, String)> {
        let plugins = {
            let inner = self.read_registry();
            inner.effect_plugins.snapshot()
        };
        plugins
            .into_iter()
            .map(|(id, plugin)| (id, plugin.name(), plugin.category()))
            .collect()
    }

    pub fn get_effect_properties(&self, effect_id: &str) -> Vec<PropertyDefinition> {
        self.operation_descriptor(EFFECT_CATEGORY, effect_id, EFFECT_APPLY_OPERATION)
            .map(|descriptor| descriptor.properties().to_vec())
            .unwrap_or_default()
    }

    pub fn get_available_exporters(&self) -> Vec<(String, String)> {
        let plugins = {
            let inner = self.read_registry();
            inner.export_plugins.snapshot()
        };
        plugins
            .into_iter()
            .map(|(id, plugin)| (id, plugin.name()))
            .collect()
    }

    pub fn get_all_plugins(&self) -> Vec<PluginInfo> {
        let (
            effects,
            loaders,
            exporters,
            entity_converters,
            effectors,
            decorators,
            styles,
            path_effects,
        ) = {
            let inner = self.read_registry();
            (
                inner.effect_plugins.snapshot(),
                inner.load_plugins.snapshot(),
                inner.export_plugins.snapshot(),
                inner.entity_converter_plugins.snapshot(),
                inner.effector_plugins.snapshot(),
                inner.decorator_plugins.snapshot(),
                inner.style_plugins.snapshot(),
                inner.path_effect_plugins.snapshot(),
            )
        };
        let mut plugins = Vec::new();
        plugins.extend(
            effects
                .into_iter()
                .map(|(id, plugin)| plugin_info(id, plugin.as_ref(), PluginCategory::Effect)),
        );
        plugins.extend(
            loaders
                .into_iter()
                .map(|(id, plugin)| plugin_info(id, plugin.as_ref(), PluginCategory::Load)),
        );
        plugins.extend(
            exporters
                .into_iter()
                .map(|(id, plugin)| plugin_info(id, plugin.as_ref(), PluginCategory::Export)),
        );
        plugins.extend(
            entity_converters.into_iter().map(|(id, plugin)| {
                plugin_info(id, plugin.as_ref(), PluginCategory::EntityConverter)
            }),
        );
        plugins.extend(
            effectors
                .into_iter()
                .map(|(id, plugin)| plugin_info(id, plugin.as_ref(), PluginCategory::Effector)),
        );
        plugins.extend(
            decorators
                .into_iter()
                .map(|(id, plugin)| plugin_info(id, plugin.as_ref(), PluginCategory::Decorator)),
        );
        plugins.extend(
            styles
                .into_iter()
                .map(|(id, plugin)| plugin_info(id, plugin.as_ref(), PluginCategory::Style)),
        );
        plugins.extend(
            path_effects
                .into_iter()
                .map(|(id, plugin)| plugin_info(id, plugin.as_ref(), PluginCategory::PathEffect)),
        );

        plugins.sort_by(|a, b| a.id.cmp(&b.id));
        plugins
    }
}

fn plugin_info<T: ?Sized + Plugin>(
    id: String,
    plugin: &T,
    plugin_type: PluginCategory,
) -> PluginInfo {
    let version = plugin.version();
    PluginInfo {
        id,
        name: plugin.name(),
        plugin_type,
        category: plugin.category(),
        version: format!("{}.{}.{}", version.0, version.1, version.2),
        impl_type: plugin.impl_type(),
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
mod automatic_locator_tests;
#[cfg(test)]
mod callback_lock_tests;
#[cfg(test)]
mod tests;
