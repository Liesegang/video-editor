//! Generic plugin repository and registry.

use std::collections::HashMap;
use std::sync::Arc;

use libloading::Library;

use crate::builtin::effects::EffectPlugin;
use crate::builtin::exporters::ExportPlugin;
use crate::builtin::loaders::LoadRepository;
use crate::plugin::evaluator::PropertyEvaluatorRegistry;
use crate::plugin::node_types::NodeTypeDefinition;
use crate::plugin::traits::Plugin;
use crate::plugin::{DecoratorPlugin, EffectorPlugin, StylePlugin};

/// Generic container for plugins of a specific type.
pub struct PluginRepository<T: ?Sized> {
    pub plugins: HashMap<String, Arc<T>>,
}

impl<T: ?Sized + Plugin> PluginRepository<T> {
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }

    pub fn register(&mut self, plugin: Arc<T>) {
        self.plugins.insert(plugin.id().to_string(), plugin);
    }

    pub fn get(&self, id: &str) -> Option<&Arc<T>> {
        self.plugins.get(id)
    }

    pub fn values(&self) -> impl Iterator<Item = &Arc<T>> {
        self.plugins.values()
    }
}

/// Internal registry holding all plugin repositories.
pub(crate) struct PluginRegistry {
    pub effect_plugins: PluginRepository<dyn EffectPlugin>,
    pub load_plugins: LoadRepository,
    pub export_plugins: PluginRepository<dyn ExportPlugin>,
    pub effector_plugins: PluginRepository<dyn EffectorPlugin>,
    pub decorator_plugins: PluginRepository<dyn DecoratorPlugin>,
    pub style_plugins: PluginRepository<dyn StylePlugin>,
    pub property_evaluators: PropertyEvaluatorRegistry,
    pub node_types: HashMap<String, NodeTypeDefinition>,
    pub dynamic_libraries: Vec<Library>,
}
