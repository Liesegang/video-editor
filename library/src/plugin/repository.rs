//! Generic plugin repository and registry.

use std::collections::HashMap;
use std::sync::Arc;

use libloading::Library;

use crate::framing::entity_converters::EntityConverterPlugin;
use crate::plugin::effects::EffectPlugin;
use crate::plugin::evaluator::PropertyEvaluatorRegistry;
use crate::plugin::exporters::ExportPlugin;
use crate::plugin::loaders::LoadPlugin;
use crate::plugin::traits::{InspectorPlugin, Plugin};

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

impl PluginRepository<dyn LoadPlugin> {
    pub fn get_sorted_plugins(&self) -> Vec<Arc<dyn LoadPlugin>> {
        let mut plugins: Vec<_> = self.plugins.values().cloned().collect();
        plugins.sort_by(|a, b| b.priority().cmp(&a.priority()));
        plugins
    }
}

/// Internal registry holding all plugin repositories.
pub(crate) struct PluginRegistry {
    pub effect_plugins: PluginRepository<dyn EffectPlugin>,
    pub load_plugins: PluginRepository<dyn LoadPlugin>,
    pub export_plugins: PluginRepository<dyn ExportPlugin>,
    pub entity_converter_plugins: PluginRepository<dyn EntityConverterPlugin>,
    pub inspector_plugins: PluginRepository<dyn InspectorPlugin>,
    pub property_evaluators: PropertyEvaluatorRegistry,
    pub dynamic_libraries: Vec<Library>,
}
