//! ABI-v1 runtime plugin discovery, registration, and invocation.

use std::path::{Path, PathBuf};

use crate::error::LibraryError;
use crate::plugin::repository::PluginRegistry;
use crate::plugin::runtime_native::{
    RuntimeBundleClaim, RuntimeBundleState, RuntimePluginDescriptor, RuntimePluginScanReport,
    RuntimeRegistrationTargets, discover_manifests, open_bundle, resolve_bundle,
    resolve_manifest_identity,
};

use super::PluginManager;

impl PluginManager {
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
                    self.bump_render_revision();
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
}
