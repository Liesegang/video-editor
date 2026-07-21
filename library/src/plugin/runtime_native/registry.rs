use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ruvie_plugin_api::{
    DECORATOR_CATEGORY, EFFECT_CATEGORY, EFFECT_CPU_RGBA8_EXTENSION_V1, EFFECTOR_CATEGORY,
    LOADER_CATEGORY, LOADER_CPU_RGBA8_EXTENSION_V1, PROPERTY_CATEGORY, PluginDescriptorV1,
    STYLE_CATEGORY,
};

use super::abi::{RuntimeComponent, RuntimeLibrary};
use super::adapters::{
    RuntimeDecoratorPlugin, RuntimeDecoratorProtocol, RuntimeEffectPlugin, RuntimeEffectorPlugin,
    RuntimeLoaderPlugin, RuntimePropertyEvaluator, RuntimeStylePlugin,
};
use super::bundle::{PendingBundle, ResolvedBundle};
use super::descriptor::{property_definitions, validate_descriptor};
use super::property_wire::property_output_default;
use crate::error::LibraryError;
use crate::model::property::Property;
use crate::plugin::evaluator::{PropertyEvaluator, PropertyEvaluatorRegistry};
use crate::plugin::repository::PluginRepository;
use crate::plugin::{
    DecoratorPlugin, EffectPlugin, EffectorPlugin, LoadPlugin, LoadRepository, StylePlugin,
};

#[derive(Clone, Debug)]
pub struct RuntimePluginDescriptor {
    pub manifest_path: PathBuf,
    pub library_path: PathBuf,
    pub descriptor: PluginDescriptorV1,
}

#[derive(Clone, Debug, Default)]
pub struct RuntimePluginScanReport {
    pub discovered_manifests: usize,
    pub loaded_bundles: Vec<PathBuf>,
    pub already_loaded_bundles: Vec<PathBuf>,
    pub in_flight_bundles: Vec<PathBuf>,
    pub registered_components: Vec<(String, String)>,
    pub failures: Vec<(PathBuf, String)>,
}

pub(crate) struct RuntimePluginRegistry {
    pub(super) libraries: HashMap<PathBuf, Arc<RuntimeLibrary>>,
    pub(super) loaded_manifests: HashSet<PathBuf>,
    in_flight_manifests: HashSet<PathBuf>,
    in_flight_libraries: HashSet<PathBuf>,
    pub(super) components: HashMap<(String, String), RuntimeComponent>,
    pub(super) descriptors: Vec<RuntimePluginDescriptor>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeBundleState {
    Unseen,
    InFlight,
    Loaded,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeBundleClaim {
    Claimed,
    InFlight,
    AlreadyLoaded,
}

impl RuntimePluginRegistry {
    pub fn new() -> Self {
        Self {
            libraries: HashMap::new(),
            loaded_manifests: HashSet::new(),
            in_flight_manifests: HashSet::new(),
            in_flight_libraries: HashSet::new(),
            components: HashMap::new(),
            descriptors: Vec::new(),
        }
    }

    pub fn manifest_state(&self, manifest_path: &Path) -> RuntimeBundleState {
        if self.loaded_manifests.contains(manifest_path) {
            RuntimeBundleState::Loaded
        } else if self.in_flight_manifests.contains(manifest_path) {
            RuntimeBundleState::InFlight
        } else {
            RuntimeBundleState::Unseen
        }
    }

    pub fn claim_bundle(&mut self, bundle: &ResolvedBundle) -> RuntimeBundleClaim {
        if self.loaded_manifests.contains(&bundle.manifest_path)
            || self.libraries.contains_key(&bundle.library_path)
        {
            // Multiple manifests may intentionally point at the same immutable
            // loaded library. Remember the alias so a later rescan does not
            // need the on-disk library to remain present.
            self.loaded_manifests.insert(bundle.manifest_path.clone());
            return RuntimeBundleClaim::AlreadyLoaded;
        }
        if self.in_flight_manifests.contains(&bundle.manifest_path)
            || self.in_flight_libraries.contains(&bundle.library_path)
        {
            return RuntimeBundleClaim::InFlight;
        }
        self.in_flight_manifests
            .insert(bundle.manifest_path.clone());
        self.in_flight_libraries.insert(bundle.library_path.clone());
        RuntimeBundleClaim::Claimed
    }

    pub fn cancel_bundle_load(&mut self, bundle: &ResolvedBundle) {
        self.in_flight_manifests.remove(&bundle.manifest_path);
        self.in_flight_libraries.remove(&bundle.library_path);
    }

    pub fn descriptors(&self) -> Vec<RuntimePluginDescriptor> {
        self.descriptors.clone()
    }

    pub(crate) fn component(
        &self,
        category: &str,
        component_id: &str,
    ) -> Result<RuntimeComponent, LibraryError> {
        self.components
            .get(&(category.to_string(), component_id.to_string()))
            .cloned()
            .ok_or_else(|| {
                LibraryError::Plugin(format!(
                    "Runtime plugin component '{category}/{component_id}' is not available"
                ))
            })
    }

    pub fn create_property(&self, evaluator_id: &str) -> Result<Property, LibraryError> {
        let key = (PROPERTY_CATEGORY.to_string(), evaluator_id.to_string());
        let component = self.components.get(&key).ok_or_else(|| {
            LibraryError::Plugin(format!(
                "Runtime property evaluator '{evaluator_id}' is not available"
            ))
        })?;
        let definitions = property_definitions(&component.descriptor)?;
        Ok(Property {
            evaluator: evaluator_id.to_string(),
            properties: definitions
                .into_iter()
                .map(|definition| {
                    (
                        definition.name().to_string(),
                        definition.default_value().clone(),
                    )
                })
                .collect(),
        })
    }

    pub fn register_bundle(
        &mut self,
        pending: PendingBundle,
        targets: RuntimeRegistrationTargets<'_>,
    ) -> Result<Vec<(String, String)>, LibraryError> {
        // Prepare every fallible definition conversion and adapter before
        // touching either repository. A malformed later component must never
        // leave an earlier component from the same bundle registered.
        let prepared = prepare_runtime_components(&pending)?;

        let mut local_keys = HashSet::new();
        for component in &prepared {
            let key = component.key.clone();
            if !local_keys.insert(key.clone()) {
                return Err(LibraryError::Plugin(format!(
                    "Duplicate runtime plugin component '{}/{}' in {}",
                    key.0,
                    key.1,
                    pending.manifest_path.display()
                )));
            }
            if self.components.contains_key(&key) {
                return Err(LibraryError::Plugin(format!(
                    "Runtime plugin component '{}/{}' is already registered",
                    key.0, key.1
                )));
            }
            match &component.adapter {
                RuntimeAdapter::Effector(_) if targets.effector_plugins.get(&key.1).is_some() => {
                    return Err(LibraryError::Plugin(format!(
                        "Effector plugin ID '{}' is already registered",
                        key.1
                    )));
                }
                RuntimeAdapter::Property(_) if targets.property_evaluators.contains(&key.1) => {
                    return Err(LibraryError::Plugin(format!(
                        "Property evaluator ID '{}' is already registered",
                        key.1
                    )));
                }
                RuntimeAdapter::Decorator(_) if targets.decorator_plugins.get(&key.1).is_some() => {
                    return Err(LibraryError::Plugin(format!(
                        "Decorator plugin ID '{}' is already registered",
                        key.1
                    )));
                }
                RuntimeAdapter::Style(_) if targets.style_plugins.get(&key.1).is_some() => {
                    return Err(LibraryError::Plugin(format!(
                        "Style plugin ID '{}' is already registered",
                        key.1
                    )));
                }
                RuntimeAdapter::Effect(_) if targets.effect_plugins.get(&key.1).is_some() => {
                    return Err(LibraryError::Plugin(format!(
                        "Effect plugin ID '{}' is already registered",
                        key.1
                    )));
                }
                RuntimeAdapter::Loader(_) if targets.load_plugins.get(&key.1).is_some() => {
                    return Err(LibraryError::Plugin(format!(
                        "Loader plugin ID '{}' is already registered",
                        key.1
                    )));
                }
                RuntimeAdapter::Effector(_)
                | RuntimeAdapter::Property(_)
                | RuntimeAdapter::Decorator(_)
                | RuntimeAdapter::Style(_)
                | RuntimeAdapter::Effect(_)
                | RuntimeAdapter::Loader(_) => {}
            }
        }

        // Everything below is an infallible commit of the prepared bundle.
        let mut registered = Vec::with_capacity(prepared.len());
        for component in prepared {
            match component.adapter {
                RuntimeAdapter::Effector(plugin) => assert!(
                    targets.effector_plugins.register(plugin).is_none(),
                    "runtime Effector registration preflight must reject replacements"
                ),
                RuntimeAdapter::Property(evaluator) => {
                    assert!(
                        targets
                            .property_evaluators
                            .register(&component.key.1, evaluator)
                            .is_none(),
                        "runtime Property registration preflight must reject replacements"
                    );
                }
                RuntimeAdapter::Decorator(plugin) => assert!(
                    targets.decorator_plugins.register(plugin).is_none(),
                    "runtime Decorator registration preflight must reject replacements"
                ),
                RuntimeAdapter::Style(plugin) => assert!(
                    targets.style_plugins.register(plugin).is_none(),
                    "runtime Style registration preflight must reject replacements"
                ),
                RuntimeAdapter::Effect(plugin) => assert!(
                    targets.effect_plugins.register(plugin).is_none(),
                    "runtime Effect registration preflight must reject replacements"
                ),
                RuntimeAdapter::Loader(plugin) => assert!(
                    targets.load_plugins.register(plugin).is_none(),
                    "runtime Loader registration preflight must reject replacements"
                ),
            }
            registered.push(component.key.clone());
            self.components.insert(component.key, component.component);
        }

        self.descriptors.push(RuntimePluginDescriptor {
            manifest_path: pending.manifest_path.clone(),
            library_path: pending.library_path.clone(),
            descriptor: pending.descriptor,
        });
        self.in_flight_manifests.remove(&pending.manifest_path);
        self.in_flight_libraries.remove(&pending.library_path);
        self.loaded_manifests.insert(pending.manifest_path);
        self.libraries.insert(pending.library_path, pending.library);
        Ok(registered)
    }
}

pub(crate) struct RuntimeRegistrationTargets<'a> {
    pub effect_plugins: &'a mut PluginRepository<dyn EffectPlugin>,
    pub load_plugins: &'a mut LoadRepository,
    pub effector_plugins: &'a mut PluginRepository<dyn EffectorPlugin>,
    pub decorator_plugins: &'a mut PluginRepository<dyn DecoratorPlugin>,
    pub style_plugins: &'a mut PluginRepository<dyn StylePlugin>,
    pub property_evaluators: &'a mut PropertyEvaluatorRegistry,
}

struct PreparedRuntimeComponent {
    key: (String, String),
    component: RuntimeComponent,
    adapter: RuntimeAdapter,
}

enum RuntimeAdapter {
    Effector(Arc<dyn EffectorPlugin>),
    Property(Arc<dyn PropertyEvaluator>),
    Decorator(Arc<dyn DecoratorPlugin>),
    Style(Arc<dyn StylePlugin>),
    Effect(Arc<dyn EffectPlugin>),
    Loader(Arc<dyn LoadPlugin>),
}

fn prepare_runtime_components(
    pending: &PendingBundle,
) -> Result<Vec<PreparedRuntimeComponent>, LibraryError> {
    validate_descriptor(&pending.descriptor)?;
    pending
        .descriptor
        .components
        .iter()
        .map(|descriptor| {
            let component = RuntimeComponent {
                descriptor: descriptor.clone(),
                library: Arc::clone(&pending.library),
            };
            let definitions = property_definitions(descriptor)?;
            let adapter = match descriptor.category.as_str() {
                EFFECTOR_CATEGORY => RuntimeAdapter::Effector(Arc::new(RuntimeEffectorPlugin {
                    component: component.clone(),
                    definitions,
                })),
                PROPERTY_CATEGORY => {
                    let output_default = property_output_default(descriptor)?;
                    RuntimeAdapter::Property(Arc::new(RuntimePropertyEvaluator {
                        component: component.clone(),
                        definitions,
                        output_default,
                    }))
                }
                DECORATOR_CATEGORY => RuntimeAdapter::Decorator(Arc::new(RuntimeDecoratorPlugin {
                    protocol: RuntimeDecoratorProtocol::negotiate(descriptor).ok_or_else(|| {
                        LibraryError::Plugin(format!(
                            "Runtime Decorator '{}' has no supported evaluator operation",
                            descriptor.id
                        ))
                    })?,
                    component: component.clone(),
                    definitions,
                })),
                STYLE_CATEGORY => RuntimeAdapter::Style(Arc::new(RuntimeStylePlugin {
                    component: component.clone(),
                    definitions,
                })),
                EFFECT_CATEGORY => RuntimeAdapter::Effect(Arc::new(RuntimeEffectPlugin::new(
                    component.clone(),
                    definitions,
                    pending.effect_api.ok_or_else(|| {
                        LibraryError::Plugin(format!(
                            "Runtime Effect '{}' has no {EFFECT_CPU_RGBA8_EXTENSION_V1} table",
                            descriptor.id
                        ))
                    })?,
                )?)),
                LOADER_CATEGORY => RuntimeAdapter::Loader(Arc::new(RuntimeLoaderPlugin {
                    component: component.clone(),
                    api: pending.loader_api.ok_or_else(|| {
                        LibraryError::Plugin(format!(
                            "Runtime Loader '{}' has no {LOADER_CPU_RGBA8_EXTENSION_V1} table",
                            descriptor.id
                        ))
                    })?,
                })),
                _ => {
                    return Err(LibraryError::Plugin(format!(
                        "Runtime plugin component '{}/{}' has no ABI-v1 host adapter",
                        descriptor.category, descriptor.id
                    )));
                }
            };
            Ok(PreparedRuntimeComponent {
                key: (descriptor.category.clone(), descriptor.id.clone()),
                component,
                adapter,
            })
        })
        .collect()
}
