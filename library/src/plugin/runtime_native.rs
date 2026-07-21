//! Stable C-ABI native plugin host.

mod abi;
mod adapters;
mod bundle;
mod descriptor;
mod property_wire;
mod registry;
mod rgba8;

#[cfg(test)]
mod tests;

pub(crate) use bundle::{
    discover_manifests, open_bundle, resolve_bundle, resolve_manifest_identity,
};
pub(crate) use registry::{
    RuntimeBundleClaim, RuntimeBundleState, RuntimePluginRegistry, RuntimeRegistrationTargets,
};
pub use registry::{RuntimePluginDescriptor, RuntimePluginScanReport};

const BUNDLE_MANIFEST_NAME: &str = "ruvie-plugin.toml";
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
const RUNTIME_EFFECT_TIME_PROPERTY: &str = "u_time";
