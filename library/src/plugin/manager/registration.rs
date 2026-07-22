//! Runtime registration mutations and their render-authority invalidation.

use std::sync::Arc;

use crate::plugin::effects::EffectPlugin;
use crate::plugin::exporters::ExportPlugin;
use crate::plugin::loaders::LoadPlugin;
use crate::plugin::{
    DecoratorPlugin, EffectorPlugin, EntityConverterPlugin, PathEffectPlugin, PropertyPlugin,
    StylePlugin,
};

use super::PluginManager;

impl PluginManager {
    pub fn register_effect(&self, plugin: Arc<dyn EffectPlugin>) {
        let replaced = {
            let mut registry = self.write_registry();
            let replaced = registry.effect_plugins.register(plugin);
            self.bump_render_revision();
            replaced
        };
        drop(replaced);
    }

    pub fn register_load_plugin(&self, plugin: Arc<dyn LoadPlugin>) {
        let replaced = {
            let mut registry = self.write_registry();
            let replaced = registry.load_plugins.register(plugin);
            self.bump_render_revision();
            replaced
        };
        drop(replaced);
    }

    pub fn register_export_plugin(&self, plugin: Arc<dyn ExportPlugin>) {
        let replaced = {
            let mut registry = self.write_registry();
            let replaced = registry.export_plugins.register(plugin);
            self.bump_render_revision();
            replaced
        };
        drop(replaced);
    }

    pub fn register_entity_converter_plugin(&self, plugin: Arc<dyn EntityConverterPlugin>) {
        let replaced = {
            let mut registry = self.write_registry();
            let replaced = registry.entity_converter_plugins.register(plugin);
            self.bump_render_revision();
            replaced
        };
        drop(replaced);
    }

    pub fn register_property_plugin(&self, plugin: Arc<dyn PropertyPlugin>) {
        let evaluator_id = plugin.id();
        let evaluator = plugin.get_evaluator_instance();
        let replaced = {
            let mut registry = self.write_registry();
            let replaced = registry
                .property_evaluators
                .register(evaluator_id, evaluator);
            self.bump_render_revision();
            replaced
        };
        drop(replaced);
    }

    pub fn register_effector_plugin(&self, plugin: Arc<dyn EffectorPlugin>) {
        let replaced = {
            let mut registry = self.write_registry();
            let replaced = registry.effector_plugins.register(plugin);
            self.bump_render_revision();
            replaced
        };
        drop(replaced);
    }

    pub fn register_decorator_plugin(&self, plugin: Arc<dyn DecoratorPlugin>) {
        let replaced = {
            let mut registry = self.write_registry();
            let replaced = registry.decorator_plugins.register(plugin);
            self.bump_render_revision();
            replaced
        };
        drop(replaced);
    }

    pub fn register_style_plugin(&self, plugin: Arc<dyn StylePlugin>) {
        let replaced = {
            let mut registry = self.write_registry();
            let replaced = registry.style_plugins.register(plugin);
            self.bump_render_revision();
            replaced
        };
        drop(replaced);
    }

    pub fn register_path_effect_plugin(&self, plugin: Arc<dyn PathEffectPlugin>) {
        let replaced = {
            let mut registry = self.write_registry();
            let replaced = registry.path_effect_plugins.register(plugin);
            self.bump_render_revision();
            replaced
        };
        drop(replaced);
    }
}
