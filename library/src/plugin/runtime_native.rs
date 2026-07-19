//! Stable C-ABI native plugin host.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use libloading::{Library, Symbol};
use ordered_float::OrderedFloat;
use ruvie_plugin_api::{
    ComponentDescriptorV1, EFFECTOR_CATEGORY, EFFECTOR_EVALUATE_V1, EffectorEvaluateRequestV1,
    EffectorOutputV1, EffectorTargetV1, InvokeRequestV1, MAX_PLUGIN_PAYLOAD_BYTES, OpacityModeV1,
    PluginDescriptorV1, PropertyUiV1, RUVIE_PLUGIN_ABI_V1, RUVIE_PLUGIN_ENTRY_V1, RuvieBuffer,
    RuvieBytesView, RuvieCallResult, RuviePluginApiV1, STATUS_OK,
};
use serde::Deserialize;

use crate::error::LibraryError;
use crate::model::ensemble::EffectorInstance;
use crate::model::property::{PropertyDefinition, PropertyUiType, PropertyValue, Vec2, Vec3, Vec4};
use crate::plugin::entity_converter::FrameEvaluationContext;
use crate::plugin::repository::PluginRepository;
use crate::plugin::{EffectorPlugin, Plugin, PluginCategory};

const BUNDLE_MANIFEST_NAME: &str = "ruvie-plugin.toml";
const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleManifest {
    manifest_version: u32,
    library: PlatformLibraries,
}

#[derive(Debug, Deserialize)]
#[allow(
    dead_code,
    reason = "all platform manifest keys are deserialized, while one is selected per host build"
)]
#[serde(deny_unknown_fields)]
struct PlatformLibraries {
    macos: Option<String>,
    linux: Option<String>,
    windows: Option<String>,
}

impl PlatformLibraries {
    fn current(&self) -> Option<&str> {
        #[cfg(target_os = "macos")]
        {
            self.macos.as_deref()
        }
        #[cfg(target_os = "linux")]
        {
            self.linux.as_deref()
        }
        #[cfg(target_os = "windows")]
        {
            self.windows.as_deref()
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            None
        }
    }
}

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
    libraries: HashMap<PathBuf, Arc<RuntimeLibrary>>,
    loaded_manifests: HashSet<PathBuf>,
    in_flight_manifests: HashSet<PathBuf>,
    in_flight_libraries: HashSet<PathBuf>,
    components: HashMap<(String, String), RuntimeComponent>,
    descriptors: Vec<RuntimePluginDescriptor>,
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

    pub fn invoke(
        &self,
        category: &str,
        component_id: &str,
        operation: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, LibraryError> {
        let key = (category.to_string(), component_id.to_string());
        let component = self.components.get(&key).ok_or_else(|| {
            LibraryError::Plugin(format!(
                "Runtime plugin component '{category}/{component_id}' is not available"
            ))
        })?;
        component.invoke(operation, payload)
    }

    pub fn register_bundle(
        &mut self,
        pending: PendingBundle,
        effector_plugins: &mut PluginRepository<dyn EffectorPlugin>,
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
            if effector_plugins.get(&key.1).is_some() {
                return Err(LibraryError::Plugin(format!(
                    "Effector plugin ID '{}' is already registered",
                    key.1
                )));
            }
        }

        // Everything below is an infallible commit of the prepared bundle.
        let mut registered = Vec::with_capacity(prepared.len());
        for component in prepared {
            effector_plugins.register(component.effector_plugin);
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

struct PreparedRuntimeComponent {
    key: (String, String),
    component: RuntimeComponent,
    effector_plugin: Arc<dyn EffectorPlugin>,
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
            let effector_plugin: Arc<dyn EffectorPlugin> = Arc::new(RuntimeEffectorPlugin {
                component: component.clone(),
                definitions,
            });
            Ok(PreparedRuntimeComponent {
                key: (descriptor.category.clone(), descriptor.id.clone()),
                component,
                effector_plugin,
            })
        })
        .collect()
}

pub(crate) fn discover_manifests(path: &Path) -> Result<Vec<PathBuf>, LibraryError> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    if path.is_file() {
        if path.file_name().and_then(|name| name.to_str()) == Some(BUNDLE_MANIFEST_NAME) {
            return Ok(vec![path.to_path_buf()]);
        }
        return Err(LibraryError::Plugin(format!(
            "Runtime plugin path {} is a file but not {BUNDLE_MANIFEST_NAME}",
            path.display()
        )));
    }

    let direct = path.join(BUNDLE_MANIFEST_NAME);
    if direct.is_file() {
        return Ok(vec![direct]);
    }

    let mut manifests = Vec::new();
    for entry in std::fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            let manifest = entry.path().join(BUNDLE_MANIFEST_NAME);
            if manifest.is_file() {
                manifests.push(manifest);
            }
        }
    }
    manifests.sort();
    Ok(manifests)
}

pub(crate) struct PendingBundle {
    manifest_path: PathBuf,
    library_path: PathBuf,
    descriptor: PluginDescriptorV1,
    library: Arc<RuntimeLibrary>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedBundle {
    pub manifest_path: PathBuf,
    pub library_path: PathBuf,
}

/// Resolves only the stable manifest identity. This deliberately happens
/// before reading the manifest or touching its library so an already-loaded
/// bundle remains idempotent even if its installed files are later damaged.
pub(crate) fn resolve_manifest_identity(manifest_path: &Path) -> Result<PathBuf, LibraryError> {
    let manifest_path = manifest_path.canonicalize()?;
    if manifest_path.file_name().and_then(|name| name.to_str()) != Some(BUNDLE_MANIFEST_NAME) {
        return Err(LibraryError::Plugin(format!(
            "Runtime plugin manifest must be named {BUNDLE_MANIFEST_NAME}: {}",
            manifest_path.display()
        )));
    }
    Ok(manifest_path)
}

/// Parses a manifest and resolves its platform library without loading native
/// code or invoking any plugin callback.
pub(crate) fn resolve_bundle(manifest_path: &Path) -> Result<ResolvedBundle, LibraryError> {
    let metadata = std::fs::metadata(manifest_path)?;
    if metadata.len() > MAX_MANIFEST_BYTES {
        return Err(LibraryError::Plugin(format!(
            "Runtime plugin manifest {} exceeds {} bytes",
            manifest_path.display(),
            MAX_MANIFEST_BYTES
        )));
    }
    let manifest_text = std::fs::read_to_string(manifest_path)?;
    let manifest: BundleManifest = toml::from_str(&manifest_text).map_err(|error| {
        LibraryError::Plugin(format!(
            "Invalid runtime plugin manifest {}: {error}",
            manifest_path.display()
        ))
    })?;
    if manifest.manifest_version != 1 {
        return Err(LibraryError::Plugin(format!(
            "Unsupported runtime bundle manifest version {} in {} (host supports 1)",
            manifest.manifest_version,
            manifest_path.display()
        )));
    }
    let library_name = manifest.library.current().ok_or_else(|| {
        LibraryError::Plugin(format!(
            "Runtime plugin manifest {} has no library for this operating system",
            manifest_path.display()
        ))
    })?;
    let bundle_dir = manifest_path.parent().ok_or_else(|| {
        LibraryError::Plugin(format!(
            "Runtime plugin manifest {} has no bundle directory",
            manifest_path.display()
        ))
    })?;
    let library_path = bundle_dir.join(library_name).canonicalize()?;
    if !library_path.starts_with(bundle_dir) {
        return Err(LibraryError::Plugin(format!(
            "Runtime plugin library {} escapes bundle directory {}",
            library_path.display(),
            bundle_dir.display()
        )));
    }
    validate_library_extension(&library_path)?;

    Ok(ResolvedBundle {
        manifest_path: manifest_path.to_path_buf(),
        library_path,
    })
}

/// Loads and validates a bundle only after the manager has claimed its
/// resolved identity. Entry/descriptor callbacks therefore cannot race with a
/// concurrent rescan of the same manifest or library.
pub(crate) fn open_bundle(bundle: &ResolvedBundle) -> Result<PendingBundle, LibraryError> {
    let library = Arc::new(RuntimeLibrary::open(&bundle.library_path)?);
    let descriptor: PluginDescriptorV1 = library.descriptor()?;
    validate_descriptor(&descriptor)?;
    Ok(PendingBundle {
        manifest_path: bundle.manifest_path.clone(),
        library_path: bundle.library_path.clone(),
        descriptor,
        library,
    })
}

fn validate_library_extension(path: &Path) -> Result<(), LibraryError> {
    let extension = path.extension().and_then(|value| value.to_str());
    #[cfg(target_os = "macos")]
    let valid = extension == Some("dylib");
    #[cfg(target_os = "linux")]
    let valid = extension == Some("so");
    #[cfg(target_os = "windows")]
    let valid = extension == Some("dll");
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let valid = false;
    if valid {
        Ok(())
    } else {
        Err(LibraryError::Plugin(format!(
            "Runtime plugin library {} has the wrong extension for this platform",
            path.display()
        )))
    }
}

struct RuntimeLibrary {
    api: RuviePluginApiV1,
    _library: Library,
}

impl RuntimeLibrary {
    fn open(path: &Path) -> Result<Self, LibraryError> {
        // SAFETY: Loading native code is restricted to an explicitly configured
        // manifest bundle. Native plugins are trusted in-process extensions.
        let library = unsafe { Library::new(path)? };
        // SAFETY: The symbol is validated by checking the returned table's ABI
        // version, size, and required callbacks before any callback is invoked.
        let entry: Symbol<unsafe extern "C" fn() -> *const RuviePluginApiV1> =
            unsafe { library.get(RUVIE_PLUGIN_ENTRY_V1)? };
        // SAFETY: Calling the versioned entry symbol is the ABI-v1 contract.
        let api_ptr = unsafe { entry() };
        if api_ptr.is_null() {
            return Err(LibraryError::Plugin(format!(
                "Runtime plugin entry returned null: {}",
                path.display()
            )));
        }
        // SAFETY: The pointer is non-null and points to the static ABI table
        // returned by the just-loaded library. We copy only after checking the
        // leading version/size fields available in every v1 table.
        let abi_version = unsafe { (*api_ptr).abi_version };
        // SAFETY: Same static table and field-prefix guarantee as above.
        let struct_size = unsafe { (*api_ptr).struct_size };
        if abi_version != RUVIE_PLUGIN_ABI_V1 || struct_size < size_of::<RuviePluginApiV1>() {
            return Err(LibraryError::Plugin(format!(
                "Runtime plugin ABI mismatch in {}: version {abi_version}, table {struct_size} bytes; host requires v{} and at least {} bytes",
                path.display(),
                RUVIE_PLUGIN_ABI_V1,
                size_of::<RuviePluginApiV1>()
            )));
        }
        // SAFETY: Version and complete v1 table size were validated.
        let api = unsafe { *api_ptr };
        if api.descriptor_json.is_none() || api.invoke_json.is_none() || api.free_buffer.is_none() {
            return Err(LibraryError::Plugin(format!(
                "Runtime plugin {} is missing a required ABI-v1 callback",
                path.display()
            )));
        }
        Ok(Self {
            api,
            _library: library,
        })
    }

    fn descriptor<T: serde::de::DeserializeOwned>(&self) -> Result<T, LibraryError> {
        let descriptor = self.api.descriptor_json.ok_or_else(|| {
            LibraryError::Plugin("Runtime plugin descriptor callback is missing".to_string())
        })?;
        // SAFETY: Callback presence and table version were validated at load.
        let result = unsafe { descriptor(self.api.context) };
        let bytes = self.copy_and_free(result)?;
        serde_json::from_slice(&bytes).map_err(LibraryError::Json)
    }

    fn invoke(&self, request: &InvokeRequestV1) -> Result<serde_json::Value, LibraryError> {
        let request_bytes = serde_json::to_vec(request)?;
        if request_bytes.len() > MAX_PLUGIN_PAYLOAD_BYTES {
            return Err(LibraryError::Plugin(format!(
                "Runtime plugin request exceeds {} bytes",
                MAX_PLUGIN_PAYLOAD_BYTES
            )));
        }
        let invoke = self.api.invoke_json.ok_or_else(|| {
            LibraryError::Plugin("Runtime plugin invoke callback is missing".to_string())
        })?;
        // SAFETY: Callback presence/table version were validated. The borrowed
        // bytes remain alive and immutable for the duration of this call.
        let result =
            unsafe { invoke(self.api.context, RuvieBytesView::from_slice(&request_bytes)) };
        let bytes = self.copy_and_free(result)?;
        serde_json::from_slice(&bytes).map_err(LibraryError::Json)
    }

    fn copy_and_free(&self, result: RuvieCallResult) -> Result<Vec<u8>, LibraryError> {
        let RuvieBuffer { ptr, len, capacity } = result.buffer;
        let structurally_reclaimable =
            (!ptr.is_null() && capacity >= len) || (ptr.is_null() && len == 0 && capacity == 0);
        let invalid = len > MAX_PLUGIN_PAYLOAD_BYTES
            || capacity < len
            || (len > 0 && ptr.is_null())
            || (ptr.is_null() && capacity > 0);
        if invalid {
            if structurally_reclaimable {
                let free = self.api.free_buffer.ok_or_else(|| {
                    LibraryError::Plugin("Runtime plugin free callback is missing".to_string())
                })?;
                // SAFETY: Although the payload is rejected (for example due to
                // size), its pointer/len/capacity still satisfy the allocator
                // round-trip contract, so returning ownership avoids a leak.
                unsafe { free(self.api.context, result.buffer) };
            }
            // A null pointer with non-zero length/capacity or len > capacity
            // cannot be passed to the reference Vec-based deallocator safely.
            // Such a trusted native plugin has already violated the ABI; the
            // host reports it and intentionally cannot reclaim that buffer.
            return Err(LibraryError::Plugin(format!(
                "Runtime plugin returned an invalid buffer (len={len}, capacity={capacity})"
            )));
        }
        let bytes = if len == 0 {
            Vec::new()
        } else {
            // SAFETY: The buffer contract, non-null pointer, and len/capacity
            // invariants were validated. It is copied before plugin deallocation.
            unsafe { std::slice::from_raw_parts(ptr.cast_const(), len) }.to_vec()
        };
        let free = self.api.free_buffer.ok_or_else(|| {
            LibraryError::Plugin("Runtime plugin free callback is missing".to_string())
        })?;
        // SAFETY: Ownership is returned once to the same loaded plugin that
        // allocated this exact buffer.
        unsafe { free(self.api.context, result.buffer) };
        if result.status != STATUS_OK {
            return Err(LibraryError::Plugin(format!(
                "Runtime plugin call failed with status {}: {}",
                result.status,
                String::from_utf8_lossy(&bytes)
            )));
        }
        Ok(bytes)
    }
}

#[derive(Clone)]
struct RuntimeComponent {
    descriptor: ComponentDescriptorV1,
    library: Arc<RuntimeLibrary>,
}

impl RuntimeComponent {
    fn invoke(
        &self,
        operation: &str,
        payload: serde_json::Value,
    ) -> Result<serde_json::Value, LibraryError> {
        if !self
            .descriptor
            .operations
            .iter()
            .any(|candidate| candidate == operation)
        {
            return Err(LibraryError::Plugin(format!(
                "Runtime plugin component '{}/{}' does not declare operation '{operation}'",
                self.descriptor.category, self.descriptor.id
            )));
        }
        self.library.invoke(&InvokeRequestV1 {
            component_id: self.descriptor.id.clone(),
            category: self.descriptor.category.clone(),
            operation: operation.to_string(),
            payload,
        })
    }
}

struct RuntimeEffectorPlugin {
    component: RuntimeComponent,
    definitions: Vec<PropertyDefinition>,
}

impl Plugin for RuntimeEffectorPlugin {
    fn id(&self) -> &str {
        &self.component.descriptor.id
    }

    fn name(&self) -> String {
        self.component.descriptor.name.clone()
    }

    fn category(&self) -> String {
        self.component.descriptor.group.clone()
    }

    fn version(&self) -> (u32, u32, u32) {
        parse_semver_triplet(&self.component.descriptor.version)
    }

    fn impl_type(&self) -> String {
        "Native ABI v1".to_string()
    }
}

impl EffectorPlugin for RuntimeEffectorPlugin {
    fn properties(&self) -> Vec<PropertyDefinition> {
        self.definitions.clone()
    }

    fn convert(
        &self,
        context: &FrameEvaluationContext,
        instance: &EffectorInstance,
        eval_time: f64,
    ) -> Option<crate::core::ensemble::types::EffectorConfig> {
        // Keep authored/unknown fields, and recover any missing known fields
        // in-memory from the descriptor. The authoritative Project is not
        // mutated merely by rendering an older or manually edited document.
        let mut resolved_properties = instance.properties.clone();
        for definition in &self.definitions {
            if resolved_properties.get(definition.name()).is_none() {
                resolved_properties.set(
                    definition.name().to_string(),
                    crate::model::property::Property::constant(definition.default_value().clone()),
                );
            }
        }
        let mut properties = BTreeMap::new();
        for (name, property) in resolved_properties.iter() {
            let value = context.evaluate_property_value(property, &resolved_properties, eval_time);
            properties.insert(name.clone(), serde_json::Value::from(&value));
        }
        let payload = match serde_json::to_value(EffectorEvaluateRequestV1 {
            time: eval_time,
            properties,
        }) {
            Ok(payload) => payload,
            Err(error) => {
                log::error!("Failed to encode runtime effector '{}': {error}", self.id());
                return None;
            }
        };
        let response = match self.component.invoke(EFFECTOR_EVALUATE_V1, payload) {
            Ok(response) => response,
            Err(error) => {
                log::error!("Runtime effector '{}' failed: {error}", self.id());
                return None;
            }
        };
        let output: EffectorOutputV1 = match serde_json::from_value(response) {
            Ok(output) => output,
            Err(error) => {
                log::error!(
                    "Runtime effector '{}' returned an invalid response: {error}",
                    self.id()
                );
                return None;
            }
        };
        match output {
            EffectorOutputV1::NoOutput => None,
            EffectorOutputV1::Transform {
                translate,
                rotate,
                scale,
                target,
            } => {
                if !translate.0.is_finite()
                    || !translate.1.is_finite()
                    || !rotate.is_finite()
                    || !scale.0.is_finite()
                    || !scale.1.is_finite()
                {
                    log::error!(
                        "Runtime effector '{}' returned non-finite values",
                        self.id()
                    );
                    return None;
                }
                Some(crate::core::ensemble::types::EffectorConfig::Transform {
                    translate,
                    rotate,
                    scale,
                    target: convert_target(target),
                })
            }
            EffectorOutputV1::Opacity {
                opacity,
                mode,
                target,
            } => {
                if !opacity.is_finite() {
                    log::error!(
                        "Runtime effector '{}' returned non-finite opacity",
                        self.id()
                    );
                    return None;
                }
                Some(crate::core::ensemble::types::EffectorConfig::Opacity {
                    target_opacity: opacity,
                    mode: match mode {
                        OpacityModeV1::Set => crate::core::ensemble::effectors::OpacityMode::Set,
                        OpacityModeV1::Add => crate::core::ensemble::effectors::OpacityMode::Add,
                        OpacityModeV1::Multiply => {
                            crate::core::ensemble::effectors::OpacityMode::Multiply
                        }
                    },
                    target: convert_target(target),
                })
            }
        }
    }

    fn plugin_type(&self) -> PluginCategory {
        PluginCategory::Effector
    }
}

fn convert_target(value: EffectorTargetV1) -> crate::core::ensemble::target::EffectorTarget {
    match value {
        EffectorTargetV1::Block => crate::core::ensemble::target::EffectorTarget::Block,
        EffectorTargetV1::Line => crate::core::ensemble::target::EffectorTarget::Line,
        EffectorTargetV1::Char => crate::core::ensemble::target::EffectorTarget::Char,
    }
}

fn parse_semver_triplet(value: &str) -> (u32, u32, u32) {
    let mut parts = value.split('.');
    let parse = |part: Option<&str>| part.and_then(|value| value.parse().ok()).unwrap_or(0);
    (
        parse(parts.next()),
        parse(parts.next()),
        parse(parts.next()),
    )
}

fn validate_descriptor(descriptor: &PluginDescriptorV1) -> Result<(), LibraryError> {
    if descriptor.name.trim().is_empty()
        || descriptor.vendor.trim().is_empty()
        || descriptor.version.trim().is_empty()
    {
        return Err(LibraryError::Plugin(
            "Runtime plugin descriptor name, vendor, and version must be non-empty".to_string(),
        ));
    }
    if descriptor.components.is_empty() {
        return Err(LibraryError::Plugin(
            "Runtime plugin descriptor has no components".to_string(),
        ));
    }
    for component in &descriptor.components {
        if component.id.trim().is_empty()
            || component.name.trim().is_empty()
            || component.category.trim().is_empty()
            || component.version.trim().is_empty()
        {
            return Err(LibraryError::Plugin(
                "Runtime plugin component id, name, category, and version must be non-empty"
                    .to_string(),
            ));
        }
        if component.category != EFFECTOR_CATEGORY {
            return Err(LibraryError::Plugin(format!(
                "Runtime plugin component '{}/{}' uses category '{}', but ABI v1 integrates only '{}'; the entire bundle was rejected",
                descriptor.name, component.id, component.category, EFFECTOR_CATEGORY
            )));
        }
        if !component
            .operations
            .iter()
            .any(|operation| operation == EFFECTOR_EVALUATE_V1)
        {
            return Err(LibraryError::Plugin(format!(
                "Runtime effector '{}' does not declare {EFFECTOR_EVALUATE_V1}",
                component.id
            )));
        }
        let mut names = HashSet::new();
        for property in &component.properties {
            if property.name.trim().is_empty() || property.label.trim().is_empty() {
                return Err(LibraryError::Plugin(format!(
                    "Runtime plugin component '{}' has an empty property name or label",
                    component.id
                )));
            }
            if !names.insert(&property.name) {
                return Err(LibraryError::Plugin(format!(
                    "Runtime plugin component '{}' repeats property '{}'",
                    component.id, property.name
                )));
            }
        }
        let _ = property_definitions(component)?;
    }
    Ok(())
}

fn property_definitions(
    component: &ComponentDescriptorV1,
) -> Result<Vec<PropertyDefinition>, LibraryError> {
    component
        .properties
        .iter()
        .map(|definition| {
            let ui_type = match &definition.ui {
                PropertyUiV1::Float {
                    min,
                    max,
                    step,
                    suffix,
                    min_hard_limit,
                    max_hard_limit,
                } => {
                    if !min.is_finite()
                        || !max.is_finite()
                        || !step.is_finite()
                        || min > max
                        || *step <= 0.0
                    {
                        return Err(LibraryError::Plugin(format!(
                            "Runtime property '{}.{}' has an invalid float range",
                            component.id, definition.name
                        )));
                    }
                    PropertyUiType::Float {
                        min: *min,
                        max: *max,
                        step: *step,
                        suffix: suffix.clone(),
                        min_hard_limit: *min_hard_limit,
                        max_hard_limit: *max_hard_limit,
                    }
                }
                PropertyUiV1::Integer {
                    min,
                    max,
                    suffix,
                    min_hard_limit,
                    max_hard_limit,
                } => {
                    if min > max {
                        return Err(LibraryError::Plugin(format!(
                            "Runtime property '{}.{}' has an invalid integer range",
                            component.id, definition.name
                        )));
                    }
                    PropertyUiType::Integer {
                        min: *min,
                        max: *max,
                        suffix: suffix.clone(),
                        min_hard_limit: *min_hard_limit,
                        max_hard_limit: *max_hard_limit,
                    }
                }
                PropertyUiV1::Color => PropertyUiType::Color,
                PropertyUiV1::Text => PropertyUiType::Text,
                PropertyUiV1::MultilineText => PropertyUiType::MultilineText,
                PropertyUiV1::Bool => PropertyUiType::Bool,
                PropertyUiV1::Vec2 { suffix } => PropertyUiType::Vec2 {
                    suffix: suffix.clone(),
                },
                PropertyUiV1::Vec3 { suffix } => PropertyUiType::Vec3 {
                    suffix: suffix.clone(),
                },
                PropertyUiV1::Vec4 { suffix } => PropertyUiType::Vec4 {
                    suffix: suffix.clone(),
                },
                PropertyUiV1::Dropdown { options } => {
                    let unique = options.iter().collect::<HashSet<_>>();
                    if options.is_empty()
                        || options.iter().any(|option| option.is_empty())
                        || unique.len() != options.len()
                    {
                        return Err(LibraryError::Plugin(format!(
                            "Runtime property '{}.{}' has invalid dropdown options",
                            component.id, definition.name
                        )));
                    }
                    PropertyUiType::Dropdown {
                        options: options.clone(),
                    }
                }
                PropertyUiV1::Font => PropertyUiType::Font,
            };
            let default_value = strict_default_value(component, definition)?;
            if let PropertyUiType::Dropdown { options } = &ui_type
                && let PropertyValue::String(value) = &default_value
                && !options.contains(value)
            {
                return Err(LibraryError::Plugin(format!(
                    "Runtime property '{}.{}' default is not a dropdown option",
                    component.id, definition.name
                )));
            }
            let property_definition = PropertyDefinition::new(
                &definition.name,
                ui_type,
                &definition.label,
                default_value,
            );
            property_definition
                .validate_value(property_definition.default_value())
                .map_err(|error| {
                    LibraryError::Plugin(format!(
                        "Runtime property '{}.{}' has an invalid default: {error}",
                        component.id, definition.name
                    ))
                })?;
            Ok(property_definition)
        })
        .collect()
}

fn strict_default_value(
    component: &ComponentDescriptorV1,
    definition: &ruvie_plugin_api::PropertyDefinitionV1,
) -> Result<PropertyValue, LibraryError> {
    let invalid = |detail: &str| {
        LibraryError::Plugin(format!(
            "Runtime property '{}.{}' has an invalid default: {detail}",
            component.id, definition.name
        ))
    };
    match &definition.ui {
        PropertyUiV1::Float { .. } => definition
            .default
            .as_f64()
            .filter(|value| value.is_finite())
            .map(|value| PropertyValue::Number(OrderedFloat(value)))
            .ok_or_else(|| invalid("expected a finite JSON number")),
        PropertyUiV1::Integer { .. } => definition
            .default
            .as_i64()
            .map(PropertyValue::Integer)
            .ok_or_else(|| invalid("expected a JSON integer representable as i64")),
        PropertyUiV1::Color => {
            let object = exact_object(&definition.default, &["r", "g", "b", "a"])
                .ok_or_else(|| invalid("expected exactly integer fields r, g, b, and a"))?;
            let channel = |name: &str| {
                object
                    .get(name)
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|value| u8::try_from(value).ok())
                    .ok_or_else(|| invalid("color channels must be integers in 0..=255"))
            };
            Ok(PropertyValue::Color(crate::model::frame::color::Color {
                r: channel("r")?,
                g: channel("g")?,
                b: channel("b")?,
                a: channel("a")?,
            }))
        }
        PropertyUiV1::Text
        | PropertyUiV1::MultilineText
        | PropertyUiV1::Dropdown { .. }
        | PropertyUiV1::Font => definition
            .default
            .as_str()
            .map(|value| PropertyValue::String(value.to_string()))
            .ok_or_else(|| invalid("expected a JSON string")),
        PropertyUiV1::Bool => definition
            .default
            .as_bool()
            .map(PropertyValue::Boolean)
            .ok_or_else(|| invalid("expected a JSON boolean")),
        PropertyUiV1::Vec2 { .. } => {
            let values = strict_vector(&definition.default, &["x", "y"])
                .ok_or_else(|| invalid("expected exactly finite number fields x and y"))?;
            Ok(PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(values[0]),
                y: OrderedFloat(values[1]),
            }))
        }
        PropertyUiV1::Vec3 { .. } => {
            let values = strict_vector(&definition.default, &["x", "y", "z"])
                .ok_or_else(|| invalid("expected exactly finite number fields x, y, and z"))?;
            Ok(PropertyValue::Vec3(Vec3 {
                x: OrderedFloat(values[0]),
                y: OrderedFloat(values[1]),
                z: OrderedFloat(values[2]),
            }))
        }
        PropertyUiV1::Vec4 { .. } => {
            let values = strict_vector(&definition.default, &["x", "y", "z", "w"])
                .ok_or_else(|| invalid("expected exactly finite number fields x, y, z, and w"))?;
            Ok(PropertyValue::Vec4(Vec4 {
                x: OrderedFloat(values[0]),
                y: OrderedFloat(values[1]),
                z: OrderedFloat(values[2]),
                w: OrderedFloat(values[3]),
            }))
        }
    }
}

fn exact_object<'a>(
    value: &'a serde_json::Value,
    keys: &[&str],
) -> Option<&'a serde_json::Map<String, serde_json::Value>> {
    let object = value.as_object()?;
    (object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key)))
        .then_some(object)
}

fn strict_vector(value: &serde_json::Value, keys: &[&str]) -> Option<Vec<f64>> {
    let object = exact_object(value, keys)?;
    keys.iter()
        .map(|key| object.get(*key)?.as_f64().filter(|value| value.is_finite()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ruvie_plugin_api::PropertyDefinitionV1;

    fn component(ui: PropertyUiV1, default: serde_json::Value) -> ComponentDescriptorV1 {
        ComponentDescriptorV1 {
            id: "example.strict".to_string(),
            name: "Strict Defaults".to_string(),
            category: EFFECTOR_CATEGORY.to_string(),
            group: "Tests".to_string(),
            version: "1.0.0".to_string(),
            operations: vec![EFFECTOR_EVALUATE_V1.to_string()],
            properties: vec![PropertyDefinitionV1 {
                name: "value".to_string(),
                label: "Value".to_string(),
                ui,
                default,
            }],
        }
    }

    fn default_error(ui: PropertyUiV1, default: serde_json::Value) -> String {
        property_definitions(&component(ui, default))
            .expect_err("invalid descriptor default must be rejected")
            .to_string()
    }

    fn current_process_library() -> Library {
        #[cfg(unix)]
        {
            libloading::os::unix::Library::this().into()
        }
        #[cfg(windows)]
        {
            libloading::os::windows::Library::this()
                .expect("open current process for an inert registry test handle")
                .into()
        }
        #[cfg(not(any(unix, windows)))]
        {
            panic!("runtime native plugins support only Unix and Windows hosts")
        }
    }

    fn pending_bundle(descriptor: PluginDescriptorV1) -> PendingBundle {
        PendingBundle {
            manifest_path: PathBuf::from("/runtime-plugin-test/ruvie-plugin.toml"),
            library_path: PathBuf::from("/runtime-plugin-test/plugin.test"),
            descriptor,
            library: Arc::new(RuntimeLibrary {
                api: RuviePluginApiV1 {
                    abi_version: RUVIE_PLUGIN_ABI_V1,
                    struct_size: size_of::<RuviePluginApiV1>(),
                    context: std::ptr::null_mut(),
                    descriptor_json: None,
                    invoke_json: None,
                    free_buffer: None,
                    query_extension: None,
                },
                _library: current_process_library(),
            }),
        }
    }

    fn two_component_descriptor(second_default: serde_json::Value) -> PluginDescriptorV1 {
        let bounded = PropertyUiV1::Float {
            min: 0.0,
            max: 100.0,
            step: 1.0,
            suffix: String::new(),
            min_hard_limit: true,
            max_hard_limit: true,
        };
        let mut first = component(bounded.clone(), serde_json::json!(25.0));
        first.id = "example.first".to_string();
        let mut second = component(bounded, second_default);
        second.id = "example.second".to_string();
        PluginDescriptorV1 {
            name: "Atomic bundle".to_string(),
            vendor: "Tests".to_string(),
            version: "1.0.0".to_string(),
            components: vec![first, second],
        }
    }

    #[test]
    fn strict_defaults_reject_lossy_json_conversions() {
        assert!(default_error(PropertyUiV1::Text, serde_json::Value::Null).contains("JSON string"));
        assert!(
            default_error(
                PropertyUiV1::Integer {
                    min: i64::MIN,
                    max: i64::MAX,
                    suffix: String::new(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                serde_json::json!(u64::MAX)
            )
            .contains("representable as i64")
        );
        assert!(
            default_error(
                PropertyUiV1::Color,
                serde_json::json!({"r": 256, "g": 0, "b": 0, "a": 255}),
            )
            .contains("0..=255")
        );
        assert!(
            default_error(
                PropertyUiV1::Color,
                serde_json::json!({"r": 1, "g": 2, "b": 3, "a": 4, "extra": 5}),
            )
            .contains("expected exactly")
        );
    }

    #[test]
    fn strict_defaults_enforce_hard_bounds_and_dropdown_membership() {
        assert!(
            default_error(
                PropertyUiV1::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 1.0,
                    suffix: String::new(),
                    min_hard_limit: true,
                    max_hard_limit: true,
                },
                serde_json::json!(101.0),
            )
            .contains("cannot be greater")
        );
        assert!(
            default_error(
                PropertyUiV1::Dropdown {
                    options: vec!["Block".to_string(), "Char".to_string()],
                },
                serde_json::json!("Parts"),
            )
            .contains("not a dropdown option")
        );
    }

    #[test]
    fn abi_v1_rejects_unintegrated_categories_instead_of_registering_descriptors() {
        let supported = component(PropertyUiV1::Bool, serde_json::json!(true));
        let mut unsupported = supported.clone();
        unsupported.id = "example.unsupported_style".to_string();
        unsupported.category = "style".to_string();
        let descriptor = PluginDescriptorV1 {
            name: "Mixed".to_string(),
            vendor: "Tests".to_string(),
            version: "1.0.0".to_string(),
            components: vec![supported, unsupported],
        };
        let error = validate_descriptor(&descriptor)
            .expect_err("an unintegrated category must reject the bundle")
            .to_string();
        assert!(error.contains("integrates only 'effector'"));
        assert!(error.contains("entire bundle was rejected"));
    }

    #[test]
    fn late_definition_failure_does_not_partially_commit_a_bundle() {
        let resolved = ResolvedBundle {
            manifest_path: PathBuf::from("/runtime-plugin-test/ruvie-plugin.toml"),
            library_path: PathBuf::from("/runtime-plugin-test/plugin.test"),
        };
        let mut registry = RuntimePluginRegistry::new();
        let mut effectors: PluginRepository<dyn EffectorPlugin> = PluginRepository::new();
        assert_eq!(
            registry.claim_bundle(&resolved),
            RuntimeBundleClaim::Claimed
        );

        let error = registry
            .register_bundle(
                pending_bundle(two_component_descriptor(serde_json::json!(101.0))),
                &mut effectors,
            )
            .expect_err("the second component exceeds its hard maximum")
            .to_string();
        assert!(error.contains("cannot be greater"));
        registry.cancel_bundle_load(&resolved);

        assert!(registry.components.is_empty());
        assert!(registry.descriptors.is_empty());
        assert!(registry.libraries.is_empty());
        assert!(registry.loaded_manifests.is_empty());
        assert!(effectors.plugins.is_empty());

        assert_eq!(
            registry.claim_bundle(&resolved),
            RuntimeBundleClaim::Claimed
        );
        let registered = registry
            .register_bundle(
                pending_bundle(two_component_descriptor(serde_json::json!(50.0))),
                &mut effectors,
            )
            .expect("a corrected rescan must not hit a stale partial-ID collision");
        assert_eq!(registered.len(), 2);
        assert!(effectors.get("example.first").is_some());
        assert!(effectors.get("example.second").is_some());
    }
}
