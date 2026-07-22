//! First-party plugin inventory and its static catalog boundary.
//!
//! Public registration and runtime discovery are extension points: operations
//! arriving through either path are executable Project data, but are never
//! required in the repository-owned `node_list.yml`. Only identities recorded
//! while constructing [`PluginManager::default`] belong to the bundled truth
//! gate.

use std::sync::Arc;

use crate::error::LibraryError;
use crate::plugin::effects::{
    BlurEffectPlugin, DilateEffectPlugin, DropShadowEffectPlugin, EffectPlugin, ErodeEffectPlugin,
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
use crate::plugin::{
    DECORATOR_APPLY_OPERATION, DECORATOR_CATEGORY, DecoratorPlugin, EFFECT_APPLY_OPERATION,
    EFFECT_CATEGORY, EFFECTOR_APPLY_OPERATION, EFFECTOR_CATEGORY, EffectorPlugin,
    IMAGE_TRANSFORM_COMPONENT_ID, OperationDescriptor, PATH_EFFECT_APPLY_OPERATION,
    PATH_EFFECT_CATEGORY, PathEffectPlugin, SHAPE_TRANSFORM_COMPONENT_ID, STYLE_APPLY_OPERATION,
    STYLE_CATEGORY, StylePlugin, TRANSFORM_APPLY_OPERATION, TRANSFORM_CATEGORY,
};

use super::PluginManager;

#[derive(Clone, Debug, PartialEq, Eq)]
struct OperationIdentity {
    category: String,
    component_id: String,
    operation: String,
}

impl OperationIdentity {
    fn new(category: &str, component_id: &str, operation: &str) -> Self {
        Self {
            category: category.to_string(),
            component_id: component_id.to_string(),
            operation: operation.to_string(),
        }
    }
}

/// Membership ledger for descriptors shipped by this binary. This is kept
/// separate from the mutable plugin repositories so third-party registration
/// cannot silently expand the static first-party catalog contract.
#[derive(Default)]
pub(super) struct BundledOperationInventory {
    identities: Vec<OperationIdentity>,
}

impl BundledOperationInventory {
    fn record(&mut self, category: &str, component_id: &str, operation: &str) {
        let identity = OperationIdentity::new(category, component_id, operation);
        assert!(
            !self.identities.contains(&identity),
            "duplicate bundled operation identity {category}/{component_id}/{operation}"
        );
        self.identities.push(identity);
    }

    fn descriptors(
        &self,
        manager: &PluginManager,
    ) -> Result<Vec<OperationDescriptor>, LibraryError> {
        self.identities
            .iter()
            .map(|identity| {
                manager.operation_descriptor(
                    &identity.category,
                    &identity.component_id,
                    &identity.operation,
                )
            })
            .collect()
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        let mut manager = Self::new();

        manager.register_bundled_effect(Arc::new(BlurEffectPlugin::new()));
        manager.register_bundled_effect(Arc::new(PixelSorterPlugin::new()));
        manager.register_bundled_effect(Arc::new(DilateEffectPlugin::new()));
        manager.register_bundled_effect(Arc::new(ErodeEffectPlugin::new()));
        manager.register_bundled_effect(Arc::new(DropShadowEffectPlugin::new()));
        manager.register_bundled_effect(Arc::new(MagnifierEffectPlugin::new()));
        manager.register_bundled_effect(Arc::new(TileEffectPlugin::new()));

        manager.register_load_plugin(Arc::new(NativeImageLoader::new()));
        manager.register_load_plugin(Arc::new(FfmpegVideoLoader::new()));

        manager.register_export_plugin(Arc::new(PngExportPlugin::new()));
        manager.register_export_plugin(Arc::new(FfmpegExportPlugin::new()));

        manager.register_property_plugin(Arc::new(ConstantPropertyPlugin::new()));
        manager.register_property_plugin(Arc::new(KeyframePropertyPlugin::new()));
        manager.register_property_plugin(Arc::new(ExpressionPropertyPlugin::new()));

        manager.register_entity_converter_plugin(Arc::new(VideoEntityConverterPlugin::new()));
        manager.register_entity_converter_plugin(Arc::new(ImageEntityConverterPlugin::new()));
        manager.register_entity_converter_plugin(Arc::new(TextEntityConverterPlugin::new()));
        manager.register_entity_converter_plugin(Arc::new(ShapeEntityConverterPlugin::new()));
        manager.register_entity_converter_plugin(Arc::new(SolidEntityConverterPlugin::new()));
        manager.register_entity_converter_plugin(Arc::new(SkSLEntityConverterPlugin::new()));

        manager
            .register_bundled_effector(Arc::new(crate::plugin::effectors::TransformEffectorPlugin));
        manager
            .register_bundled_effector(Arc::new(crate::plugin::effectors::StepDelayEffectorPlugin));
        manager
            .register_bundled_effector(Arc::new(crate::plugin::effectors::RandomizeEffectorPlugin));
        manager
            .register_bundled_effector(Arc::new(crate::plugin::effectors::OpacityEffectorPlugin));

        manager.register_bundled_decorator(Arc::new(
            crate::plugin::decorators::BackplateDecoratorPlugin,
        ));

        manager.register_bundled_style(Arc::new(crate::plugin::styles::FillStylePlugin));
        manager.register_bundled_style(Arc::new(crate::plugin::styles::StrokeStylePlugin));
        manager.register_bundled_style(Arc::new(crate::plugin::styles::ImageOpacityStylePlugin));

        manager.register_bundled_path_effect(Arc::new(
            crate::plugin::path_effects::DashPathEffectPlugin,
        ));
        manager.register_bundled_path_effect(Arc::new(
            crate::plugin::path_effects::CornerPathEffectPlugin,
        ));
        manager.register_bundled_path_effect(Arc::new(
            crate::plugin::path_effects::DiscretePathEffectPlugin,
        ));
        manager.register_bundled_path_effect(Arc::new(
            crate::plugin::path_effects::TrimPathEffectPlugin,
        ));

        // Transform execution is native for performance, but its persisted
        // contract is the same descriptor-backed plugin operation contract.
        manager.bundled_operations.record(
            TRANSFORM_CATEGORY,
            SHAPE_TRANSFORM_COMPONENT_ID,
            TRANSFORM_APPLY_OPERATION,
        );
        manager.bundled_operations.record(
            TRANSFORM_CATEGORY,
            IMAGE_TRANSFORM_COMPONENT_ID,
            TRANSFORM_APPLY_OPERATION,
        );

        manager
    }
}

impl PluginManager {
    fn register_bundled_effect(&mut self, plugin: Arc<dyn EffectPlugin>) {
        self.bundled_operations
            .record(EFFECT_CATEGORY, plugin.id(), EFFECT_APPLY_OPERATION);
        self.register_effect(plugin);
    }

    fn register_bundled_effector(&mut self, plugin: Arc<dyn EffectorPlugin>) {
        self.bundled_operations
            .record(EFFECTOR_CATEGORY, plugin.id(), EFFECTOR_APPLY_OPERATION);
        self.register_effector_plugin(plugin);
    }

    fn register_bundled_decorator(&mut self, plugin: Arc<dyn DecoratorPlugin>) {
        self.bundled_operations
            .record(DECORATOR_CATEGORY, plugin.id(), DECORATOR_APPLY_OPERATION);
        self.register_decorator_plugin(plugin);
    }

    fn register_bundled_style(&mut self, plugin: Arc<dyn StylePlugin>) {
        self.bundled_operations
            .record(STYLE_CATEGORY, plugin.id(), STYLE_APPLY_OPERATION);
        self.register_style_plugin(plugin);
    }

    fn register_bundled_path_effect(&mut self, plugin: Arc<dyn PathEffectPlugin>) {
        self.bundled_operations.record(
            PATH_EFFECT_CATEGORY,
            plugin.id(),
            PATH_EFFECT_APPLY_OPERATION,
        );
        self.register_path_effect_plugin(plugin);
    }

    /// Enumerates the descriptor contracts bundled with this binary.
    ///
    /// Operations registered later through public registration or runtime
    /// discovery remain executable but are intentionally excluded: external
    /// plugins must not be forced into the repository's static node catalog.
    /// Resolution uses the same path as graph authoring and rendering.
    pub fn bundled_operation_descriptors(&self) -> Result<Vec<OperationDescriptor>, LibraryError> {
        self.bundled_operations.descriptors(self)
    }
}
