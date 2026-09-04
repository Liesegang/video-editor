//! Generic plugin repository and registry.

use std::collections::HashMap;
use std::sync::Arc;

use libloading::Library;

use crate::plugin::EntityConverterPlugin;
use crate::plugin::effects::EffectPlugin;
use crate::plugin::evaluator::PropertyEvaluatorRegistry;
use crate::plugin::exporters::ExportPlugin;
use crate::plugin::loaders::LoadRepository;
use crate::plugin::runtime_native::RuntimePluginRegistry;
use crate::plugin::traits::Plugin;
use crate::plugin::{DecoratorPlugin, EffectorPlugin, PathEffectPlugin, StylePlugin};

/// Generic container for plugins of a specific type.
pub struct PluginRepository<T: ?Sized> {
    pub plugins: HashMap<String, Arc<T>>,
}

impl<T: ?Sized + Plugin> Default for PluginRepository<T> {
    fn default() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }
}

impl<T: ?Sized + Plugin> PluginRepository<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a plugin under an identity resolved before the caller takes
    /// its registry lock and returns the replaced instance, if any.
    ///
    /// Callers holding a manager lock must drop the returned `Arc` only after
    /// releasing that lock: plugin destructors may call back into the manager.
    /// [`Plugin::id`] is executable plugin code. Requiring the key here keeps
    /// repositories as inert storage and prevents registration from invoking
    /// a re-entrant callback while the manager write lock is held.
    pub fn register(&mut self, id: String, plugin: Arc<T>) -> Option<Arc<T>> {
        self.plugins.insert(id, plugin)
    }

    pub fn get(&self, id: &str) -> Option<&Arc<T>> {
        self.plugins.get(id)
    }

    pub fn values(&self) -> impl Iterator<Item = &Arc<T>> {
        self.plugins.values()
    }

    /// Clones immutable endpoints and their registry-owned identities so a
    /// caller can release its registry lock before invoking plugin code.
    pub fn snapshot(&self) -> Vec<(String, Arc<T>)> {
        self.plugins
            .iter()
            .map(|(id, plugin)| (id.clone(), Arc::clone(plugin)))
            .collect()
    }
}

/// Internal registry holding all plugin repositories.
pub(crate) struct PluginRegistry {
    pub effect_plugins: PluginRepository<dyn EffectPlugin>,
    pub load_plugins: LoadRepository,
    pub export_plugins: PluginRepository<dyn ExportPlugin>,
    pub entity_converter_plugins: PluginRepository<dyn EntityConverterPlugin>,
    pub effector_plugins: PluginRepository<dyn EffectorPlugin>,
    pub decorator_plugins: PluginRepository<dyn DecoratorPlugin>,
    pub style_plugins: PluginRepository<dyn StylePlugin>,
    pub path_effect_plugins: PluginRepository<dyn PathEffectPlugin>,
    pub property_evaluators: PropertyEvaluatorRegistry,
    pub dynamic_libraries: Vec<Library>,
    pub runtime_plugins: RuntimePluginRegistry,
}
