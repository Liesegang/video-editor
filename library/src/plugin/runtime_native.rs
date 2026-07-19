//! Stable C-ABI native plugin host.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use libloading::{Library, Symbol};
use ordered_float::OrderedFloat;
use ruvie_plugin_api::{
    BackplateShapeV1, ColorV1, ComponentDescriptorV1, DECORATOR_CATEGORY, DECORATOR_EVALUATE_V1,
    DecoratorEvaluateRequestV1, DecoratorOutputV1, DecoratorTargetV1, EFFECTOR_CATEGORY,
    EFFECTOR_EVALUATE_V1, EffectorEvaluateRequestV1, EffectorOutputV1, EffectorTargetV1, InsetsV1,
    InvokeRequestV1, MAX_PLUGIN_PAYLOAD_BYTES, OpacityModeV1, PROPERTY_CATEGORY,
    PROPERTY_EVALUATE_V1, PluginDescriptorV1, PropertyEvaluateRequestV1,
    PropertyEvaluateResponseV1, PropertyUiV1, PropertyValueV1, RUVIE_PLUGIN_ABI_V1,
    RUVIE_PLUGIN_ENTRY_V1, RuvieBuffer, RuvieBytesView, RuvieCallResult, RuviePluginApiV1,
    STATUS_OK, STYLE_CATEGORY, STYLE_EVALUATE_V1, StrokeCapV1, StrokeJoinV1,
    StyleEvaluateRequestV1, StyleOutputV1,
};
use serde::Deserialize;

use crate::error::LibraryError;
use crate::model::property::{
    Property, PropertyDefinition, PropertyUiType, PropertyValue, Vec2, Vec3, Vec4,
};
use crate::plugin::entity_converter::FrameEvaluationContext;
use crate::plugin::evaluator::{EvaluationContext, PropertyEvaluator, PropertyEvaluatorRegistry};
use crate::plugin::repository::PluginRepository;
use crate::plugin::{DecoratorPlugin, EffectorPlugin, Plugin, PluginCategory, StylePlugin};

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
        effector_plugins: &mut PluginRepository<dyn EffectorPlugin>,
        decorator_plugins: &mut PluginRepository<dyn DecoratorPlugin>,
        style_plugins: &mut PluginRepository<dyn StylePlugin>,
        property_evaluators: &mut PropertyEvaluatorRegistry,
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
                RuntimeAdapter::Effector(_) if effector_plugins.get(&key.1).is_some() => {
                    return Err(LibraryError::Plugin(format!(
                        "Effector plugin ID '{}' is already registered",
                        key.1
                    )));
                }
                RuntimeAdapter::Property(_) if property_evaluators.contains(&key.1) => {
                    return Err(LibraryError::Plugin(format!(
                        "Property evaluator ID '{}' is already registered",
                        key.1
                    )));
                }
                RuntimeAdapter::Decorator(_) if decorator_plugins.get(&key.1).is_some() => {
                    return Err(LibraryError::Plugin(format!(
                        "Decorator plugin ID '{}' is already registered",
                        key.1
                    )));
                }
                RuntimeAdapter::Style(_) if style_plugins.get(&key.1).is_some() => {
                    return Err(LibraryError::Plugin(format!(
                        "Style plugin ID '{}' is already registered",
                        key.1
                    )));
                }
                RuntimeAdapter::Effector(_)
                | RuntimeAdapter::Property(_)
                | RuntimeAdapter::Decorator(_)
                | RuntimeAdapter::Style(_) => {}
            }
        }

        // Everything below is an infallible commit of the prepared bundle.
        let mut registered = Vec::with_capacity(prepared.len());
        for component in prepared {
            match component.adapter {
                RuntimeAdapter::Effector(plugin) => effector_plugins.register(plugin),
                RuntimeAdapter::Property(evaluator) => {
                    property_evaluators.register(&component.key.1, evaluator);
                }
                RuntimeAdapter::Decorator(plugin) => decorator_plugins.register(plugin),
                RuntimeAdapter::Style(plugin) => style_plugins.register(plugin),
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
                    component: component.clone(),
                    definitions,
                })),
                STYLE_CATEGORY => RuntimeAdapter::Style(Arc::new(RuntimeStylePlugin {
                    component: component.clone(),
                    definitions,
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

    fn evaluate_source(
        &self,
        context: &FrameEvaluationContext,
        _source_id: uuid::Uuid,
        properties: &crate::model::property::PropertyMap,
        eval_time: f64,
    ) -> Option<crate::core::ensemble::types::EffectorConfig> {
        let mut resolved_properties = properties.clone();
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

fn resolved_config_properties(
    context: &FrameEvaluationContext,
    definitions: &[PropertyDefinition],
    properties: &crate::model::property::PropertyMap,
    eval_time: f64,
    operation_label: &str,
) -> Option<BTreeMap<String, PropertyValueV1>> {
    if !eval_time.is_finite() {
        log::error!("{operation_label} received a non-finite evaluation time");
        return None;
    }
    let evaluated = context.evaluate_operation_properties(
        definitions,
        properties,
        eval_time,
        operation_label,
    )?;
    let mut wire_properties = BTreeMap::new();
    for definition in definitions {
        let Some(value) = evaluated.get(definition.name()) else {
            log::error!(
                "{operation_label} did not resolve declared property '{}'",
                definition.name()
            );
            return None;
        };
        let value = match property_value_to_wire(value) {
            Ok(value) => value,
            Err(error) => {
                log::error!(
                    "{operation_label} property '{}' cannot cross ABI v1: {error}",
                    definition.name()
                );
                return None;
            }
        };
        wire_properties.insert(definition.name().to_string(), value);
    }
    Some(wire_properties)
}

struct RuntimeStylePlugin {
    component: RuntimeComponent,
    definitions: Vec<PropertyDefinition>,
}

impl Plugin for RuntimeStylePlugin {
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

impl StylePlugin for RuntimeStylePlugin {
    fn descriptor(
        &self,
    ) -> Result<crate::plugin::OperationDescriptor, crate::plugin::OperationDescriptorError> {
        crate::plugin::OperationDescriptor::style(self.id(), self.name(), self.definitions.clone())
    }

    fn evaluate_source(
        &self,
        context: &FrameEvaluationContext,
        source_id: uuid::Uuid,
        properties: &crate::model::property::PropertyMap,
        eval_time: f64,
    ) -> Option<crate::model::frame::entity::StyleConfig> {
        let label = format!("Runtime Style '{}'", self.id());
        let properties =
            resolved_config_properties(context, &self.definitions, properties, eval_time, &label)?;
        let payload = match serde_json::to_value(StyleEvaluateRequestV1 {
            time: eval_time,
            fps: context.evaluation_fps(),
            properties,
        }) {
            Ok(payload) => payload,
            Err(error) => {
                log::error!("Failed to encode {label}: {error}");
                return None;
            }
        };
        let response = match self.component.invoke(STYLE_EVALUATE_V1, payload) {
            Ok(response) => response,
            Err(error) => {
                log::error!("{label} failed: {error}");
                return None;
            }
        };
        safe_style_config_from_response(response, source_id, &label)
    }
}

fn safe_style_config_from_response(
    response: serde_json::Value,
    source_id: uuid::Uuid,
    operation_label: &str,
) -> Option<crate::model::frame::entity::StyleConfig> {
    match style_config_from_response(response, source_id) {
        Ok(output) => output,
        Err(error) => {
            log::error!("{operation_label} returned an invalid config: {error}");
            None
        }
    }
}

fn style_config_from_response(
    response: serde_json::Value,
    source_id: uuid::Uuid,
) -> Result<Option<crate::model::frame::entity::StyleConfig>, LibraryError> {
    let output = serde_json::from_value(response).map_err(|error| {
        LibraryError::Plugin(format!("Runtime Style response is invalid: {error}"))
    })?;
    style_config_from_wire(output, source_id)
}

fn style_config_from_wire(
    output: StyleOutputV1,
    source_id: uuid::Uuid,
) -> Result<Option<crate::model::frame::entity::StyleConfig>, LibraryError> {
    use crate::model::frame::draw_type::{CapType, DrawStyle, JoinType};

    let invalid = |detail: &str| LibraryError::Plugin(format!("Runtime Style output {detail}"));
    let style = match output {
        StyleOutputV1::NoOutput => return Ok(None),
        StyleOutputV1::Fill { color, offset } => {
            if !finite_render_scalar(offset) || !finite_render_scalar(offset * 2.0) {
                return Err(invalid("has an unsafe Fill offset"));
            }
            DrawStyle::Fill {
                color: color_from_wire(color),
                offset,
            }
        }
        StyleOutputV1::Stroke {
            color,
            width,
            offset,
            cap,
            join,
            miter,
            dash_array,
            dash_offset,
        } => {
            let effective_width = (width + offset * 2.0).max(0.0);
            if !finite_render_scalar(width)
                || !finite_render_scalar(offset)
                || !finite_render_scalar(effective_width)
                || !finite_render_scalar(miter)
                || !finite_render_scalar(dash_offset)
                || width < 0.0
                || miter < 0.0
                || !valid_stroke_dash_array(&dash_array)
            {
                return Err(invalid("has invalid Stroke numeric fields"));
            }
            DrawStyle::Stroke {
                color: color_from_wire(color),
                width,
                offset,
                cap: match cap {
                    StrokeCapV1::Round => CapType::Round,
                    StrokeCapV1::Square => CapType::Square,
                    StrokeCapV1::Butt => CapType::Butt,
                },
                join: match join {
                    StrokeJoinV1::Round => JoinType::Round,
                    StrokeJoinV1::Bevel => JoinType::Bevel,
                    StrokeJoinV1::Miter => JoinType::Miter,
                },
                miter,
                dash_array,
                dash_offset,
            }
        }
    };
    Ok(Some(crate::model::frame::entity::StyleConfig {
        id: source_id,
        style,
    }))
}

fn valid_stroke_dash_array(values: &[f64]) -> bool {
    values.is_empty()
        || (values.len().is_multiple_of(2)
            && values
                .iter()
                .all(|value| finite_render_scalar(*value) && *value > 0.0))
}

fn finite_render_scalar(value: f64) -> bool {
    value.is_finite() && (value as f32).is_finite()
}

struct RuntimeDecoratorPlugin {
    component: RuntimeComponent,
    definitions: Vec<PropertyDefinition>,
}

impl Plugin for RuntimeDecoratorPlugin {
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

impl DecoratorPlugin for RuntimeDecoratorPlugin {
    fn properties(&self) -> Vec<PropertyDefinition> {
        self.definitions.clone()
    }

    fn evaluate_source(
        &self,
        context: &FrameEvaluationContext,
        _source_id: uuid::Uuid,
        properties: &crate::model::property::PropertyMap,
        eval_time: f64,
    ) -> Option<crate::core::ensemble::types::DecoratorConfig> {
        let label = format!("Runtime Decorator '{}'", self.id());
        let properties =
            resolved_config_properties(context, &self.definitions, properties, eval_time, &label)?;
        let payload = match serde_json::to_value(DecoratorEvaluateRequestV1 {
            time: eval_time,
            fps: context.evaluation_fps(),
            properties,
        }) {
            Ok(payload) => payload,
            Err(error) => {
                log::error!("Failed to encode {label}: {error}");
                return None;
            }
        };
        let response = match self.component.invoke(DECORATOR_EVALUATE_V1, payload) {
            Ok(response) => response,
            Err(error) => {
                log::error!("{label} failed: {error}");
                return None;
            }
        };
        safe_decorator_config_from_response(response, &label)
    }
}

fn safe_decorator_config_from_response(
    response: serde_json::Value,
    operation_label: &str,
) -> Option<crate::core::ensemble::types::DecoratorConfig> {
    match decorator_config_from_response(response) {
        Ok(output) => output,
        Err(error) => {
            log::error!("{operation_label} returned an invalid config: {error}");
            None
        }
    }
}

fn decorator_config_from_response(
    response: serde_json::Value,
) -> Result<Option<crate::core::ensemble::types::DecoratorConfig>, LibraryError> {
    let output = serde_json::from_value(response).map_err(|error| {
        LibraryError::Plugin(format!("Runtime Decorator response is invalid: {error}"))
    })?;
    decorator_config_from_wire(output)
}

fn decorator_config_from_wire(
    output: DecoratorOutputV1,
) -> Result<Option<crate::core::ensemble::types::DecoratorConfig>, LibraryError> {
    use crate::core::ensemble::decorators::{BackplateShape, BackplateTarget};
    use crate::core::ensemble::types::DecoratorConfig;

    let DecoratorOutputV1::Backplate {
        target,
        shape,
        color,
        padding,
        corner_radius,
    } = output
    else {
        return Ok(None);
    };
    let InsetsV1 {
        top,
        right,
        bottom,
        left,
    } = padding;
    if !top.is_finite()
        || !right.is_finite()
        || !bottom.is_finite()
        || !left.is_finite()
        || !corner_radius.is_finite()
        || corner_radius < 0.0
    {
        return Err(LibraryError::Plugin(
            "Runtime Decorator output has invalid Backplate numeric fields".to_string(),
        ));
    }
    Ok(Some(DecoratorConfig::Backplate {
        target: match target {
            DecoratorTargetV1::Block => BackplateTarget::Block,
            DecoratorTargetV1::Line => BackplateTarget::Line,
            DecoratorTargetV1::Char => BackplateTarget::Char,
        },
        shape: match shape {
            BackplateShapeV1::Rect => BackplateShape::Rect,
            BackplateShapeV1::RoundedRect => BackplateShape::RoundedRect,
            BackplateShapeV1::Circle => BackplateShape::Circle,
        },
        color: color_from_wire(color),
        padding: (top, right, bottom, left),
        corner_radius,
    }))
}

fn color_from_wire(color: ColorV1) -> crate::model::frame::color::Color {
    crate::model::frame::color::Color {
        r: color.r,
        g: color.g,
        b: color.b,
        a: color.a,
    }
}

struct RuntimePropertyEvaluator {
    component: RuntimeComponent,
    definitions: Vec<PropertyDefinition>,
    output_default: PropertyValue,
}

impl RuntimePropertyEvaluator {
    fn fallback(&self, detail: impl std::fmt::Display) -> PropertyValue {
        log::error!(
            "Runtime property evaluator '{}' failed: {detail}",
            self.component.descriptor.id
        );
        self.output_default.clone()
    }
}

impl PropertyEvaluator for RuntimePropertyEvaluator {
    fn evaluate(
        &self,
        property: &Property,
        time: f64,
        context: &EvaluationContext,
    ) -> PropertyValue {
        let mut properties = BTreeMap::new();
        for definition in &self.definitions {
            let value = property
                .properties
                .get(definition.name())
                .unwrap_or_else(|| definition.default_value());
            if let Err(error) = definition.validate_value(value) {
                return self.fallback(format!(
                    "property '{}' is invalid: {error}",
                    definition.name()
                ));
            }
            let value = match property_value_to_wire(value) {
                Ok(value) => value,
                Err(error) => {
                    return self.fallback(format!(
                        "property '{}' cannot cross ABI v1: {error}",
                        definition.name()
                    ));
                }
            };
            properties.insert(definition.name().to_string(), value);
        }
        let payload = match serde_json::to_value(PropertyEvaluateRequestV1 {
            time,
            fps: context.fps,
            properties,
        }) {
            Ok(payload) => payload,
            Err(error) => return self.fallback(format!("request encoding failed: {error}")),
        };
        let response = match self.component.invoke(PROPERTY_EVALUATE_V1, payload) {
            Ok(response) => response,
            Err(error) => return self.fallback(error),
        };
        let response: PropertyEvaluateResponseV1 = match serde_json::from_value(response) {
            Ok(response) => response,
            Err(error) => return self.fallback(format!("invalid response: {error}")),
        };
        let value = match property_value_from_wire(&response.value) {
            Ok(value) => value,
            Err(error) => return self.fallback(format!("invalid response value: {error}")),
        };
        if std::mem::discriminant(&value) != std::mem::discriminant(&self.output_default) {
            return self.fallback("response type differs from output_default");
        }
        value
    }
}

fn property_value_to_wire(value: &PropertyValue) -> Result<PropertyValueV1, &'static str> {
    match value {
        PropertyValue::Number(value) if value.is_finite() => Ok(PropertyValueV1::Number {
            value: value.into_inner(),
        }),
        PropertyValue::Number(_) => Err("number must be finite"),
        PropertyValue::Integer(value) => Ok(PropertyValueV1::Integer { value: *value }),
        PropertyValue::String(value) => Ok(PropertyValueV1::String {
            value: value.clone(),
        }),
        PropertyValue::Boolean(value) => Ok(PropertyValueV1::Boolean { value: *value }),
        PropertyValue::Vec2(value) if value.x.is_finite() && value.y.is_finite() => {
            Ok(PropertyValueV1::Vec2 {
                x: value.x.into_inner(),
                y: value.y.into_inner(),
            })
        }
        PropertyValue::Vec3(value)
            if value.x.is_finite() && value.y.is_finite() && value.z.is_finite() =>
        {
            Ok(PropertyValueV1::Vec3 {
                x: value.x.into_inner(),
                y: value.y.into_inner(),
                z: value.z.into_inner(),
            })
        }
        PropertyValue::Vec4(value)
            if value.x.is_finite()
                && value.y.is_finite()
                && value.z.is_finite()
                && value.w.is_finite() =>
        {
            Ok(PropertyValueV1::Vec4 {
                x: value.x.into_inner(),
                y: value.y.into_inner(),
                z: value.z.into_inner(),
                w: value.w.into_inner(),
            })
        }
        PropertyValue::Vec2(_) | PropertyValue::Vec3(_) | PropertyValue::Vec4(_) => {
            Err("vector components must be finite")
        }
        PropertyValue::Color(value) => Ok(PropertyValueV1::Color {
            r: value.r,
            g: value.g,
            b: value.b,
            a: value.a,
        }),
        PropertyValue::Array(_) | PropertyValue::Map(_) => {
            Err("array and map values are not supported by ABI v1")
        }
    }
}

fn property_value_from_wire(value: &PropertyValueV1) -> Result<PropertyValue, LibraryError> {
    let non_finite =
        || LibraryError::Plugin("Runtime property value contains a non-finite number".to_string());
    match value {
        PropertyValueV1::Number { value } => value
            .is_finite()
            .then_some(PropertyValue::Number(OrderedFloat(*value)))
            .ok_or_else(non_finite),
        PropertyValueV1::Integer { value } => Ok(PropertyValue::Integer(*value)),
        PropertyValueV1::String { value } => Ok(PropertyValue::String(value.clone())),
        PropertyValueV1::Boolean { value } => Ok(PropertyValue::Boolean(*value)),
        PropertyValueV1::Vec2 { x, y } => {
            if !x.is_finite() || !y.is_finite() {
                return Err(non_finite());
            }
            Ok(PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(*x),
                y: OrderedFloat(*y),
            }))
        }
        PropertyValueV1::Vec3 { x, y, z } => {
            if !x.is_finite() || !y.is_finite() || !z.is_finite() {
                return Err(non_finite());
            }
            Ok(PropertyValue::Vec3(Vec3 {
                x: OrderedFloat(*x),
                y: OrderedFloat(*y),
                z: OrderedFloat(*z),
            }))
        }
        PropertyValueV1::Vec4 { x, y, z, w } => {
            if !x.is_finite() || !y.is_finite() || !z.is_finite() || !w.is_finite() {
                return Err(non_finite());
            }
            Ok(PropertyValue::Vec4(Vec4 {
                x: OrderedFloat(*x),
                y: OrderedFloat(*y),
                z: OrderedFloat(*z),
                w: OrderedFloat(*w),
            }))
        }
        PropertyValueV1::Color { r, g, b, a } => {
            Ok(PropertyValue::Color(crate::model::frame::color::Color {
                r: *r,
                g: *g,
                b: *b,
                a: *a,
            }))
        }
    }
}

fn property_output_default(
    component: &ComponentDescriptorV1,
) -> Result<PropertyValue, LibraryError> {
    let value = component.output_default.as_ref().ok_or_else(|| {
        LibraryError::Plugin(format!(
            "Runtime property '{}' must declare output_default",
            component.id
        ))
    })?;
    property_value_from_wire(value).map_err(|error| {
        LibraryError::Plugin(format!(
            "Runtime property '{}' has an invalid output_default: {error}",
            component.id
        ))
    })
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
        match component.category.as_str() {
            EFFECTOR_CATEGORY => {
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
                if component.output_default.is_some() {
                    return Err(LibraryError::Plugin(format!(
                        "Runtime effector '{}' must not declare output_default",
                        component.id
                    )));
                }
            }
            PROPERTY_CATEGORY => {
                if !component
                    .operations
                    .iter()
                    .any(|operation| operation == PROPERTY_EVALUATE_V1)
                {
                    return Err(LibraryError::Plugin(format!(
                        "Runtime property '{}' does not declare {PROPERTY_EVALUATE_V1}",
                        component.id
                    )));
                }
                let _ = property_output_default(component)?;
            }
            STYLE_CATEGORY => {
                if !component
                    .operations
                    .iter()
                    .any(|operation| operation == STYLE_EVALUATE_V1)
                {
                    return Err(LibraryError::Plugin(format!(
                        "Runtime Style '{}' does not declare {STYLE_EVALUATE_V1}",
                        component.id
                    )));
                }
                if component.output_default.is_some() {
                    return Err(LibraryError::Plugin(format!(
                        "Runtime Style '{}' must not declare output_default",
                        component.id
                    )));
                }
            }
            DECORATOR_CATEGORY => {
                if !component
                    .operations
                    .iter()
                    .any(|operation| operation == DECORATOR_EVALUATE_V1)
                {
                    return Err(LibraryError::Plugin(format!(
                        "Runtime Decorator '{}' does not declare {DECORATOR_EVALUATE_V1}",
                        component.id
                    )));
                }
                if component.output_default.is_some() {
                    return Err(LibraryError::Plugin(format!(
                        "Runtime Decorator '{}' must not declare output_default",
                        component.id
                    )));
                }
            }
            unsupported => {
                return Err(LibraryError::Plugin(format!(
                    "Runtime plugin component '{}/{}' uses category '{unsupported}', but ABI v1 integrates only '{EFFECTOR_CATEGORY}', '{PROPERTY_CATEGORY}', '{STYLE_CATEGORY}', and '{DECORATOR_CATEGORY}'; the entire bundle was rejected",
                    descriptor.name, component.id
                )));
            }
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
            output_default: None,
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

    fn property_component(output_default: Option<PropertyValueV1>) -> ComponentDescriptorV1 {
        ComponentDescriptorV1 {
            id: "example.runtime_property".to_string(),
            name: "Runtime Property".to_string(),
            category: PROPERTY_CATEGORY.to_string(),
            group: "Tests".to_string(),
            version: "1.0.0".to_string(),
            operations: vec![PROPERTY_EVALUATE_V1.to_string()],
            properties: vec![PropertyDefinitionV1 {
                name: "amplitude".to_string(),
                label: "Amplitude".to_string(),
                ui: PropertyUiV1::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 1.0,
                    suffix: String::new(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                default: serde_json::json!(1.0),
            }],
            output_default,
        }
    }

    fn style_component() -> ComponentDescriptorV1 {
        ComponentDescriptorV1 {
            id: "example.runtime_fill".to_string(),
            name: "Runtime Fill".to_string(),
            category: STYLE_CATEGORY.to_string(),
            group: "Tests".to_string(),
            version: "1.2.3".to_string(),
            operations: vec![STYLE_EVALUATE_V1.to_string()],
            properties: vec![
                PropertyDefinitionV1 {
                    name: "color".to_string(),
                    label: "Color".to_string(),
                    ui: PropertyUiV1::Color,
                    default: serde_json::json!({"r": 10, "g": 20, "b": 30, "a": 255}),
                },
                PropertyDefinitionV1 {
                    name: "offset".to_string(),
                    label: "Offset".to_string(),
                    ui: PropertyUiV1::Float {
                        min: -100.0,
                        max: 100.0,
                        step: 1.0,
                        suffix: "px".to_string(),
                        min_hard_limit: false,
                        max_hard_limit: false,
                    },
                    default: serde_json::json!(2.0),
                },
            ],
            output_default: None,
        }
    }

    fn decorator_component() -> ComponentDescriptorV1 {
        ComponentDescriptorV1 {
            id: "example.runtime_backplate".to_string(),
            name: "Runtime Backplate".to_string(),
            category: DECORATOR_CATEGORY.to_string(),
            group: "Tests".to_string(),
            version: "2.3.4".to_string(),
            operations: vec![DECORATOR_EVALUATE_V1.to_string()],
            properties: vec![
                PropertyDefinitionV1 {
                    name: "target".to_string(),
                    label: "Target".to_string(),
                    ui: PropertyUiV1::Dropdown {
                        options: vec!["Block".to_string(), "Line".to_string(), "Char".to_string()],
                    },
                    default: serde_json::json!("Block"),
                },
                PropertyDefinitionV1 {
                    name: "padding".to_string(),
                    label: "Padding".to_string(),
                    ui: PropertyUiV1::Vec4 {
                        suffix: "px".to_string(),
                    },
                    default: serde_json::json!({"x": 1.0, "y": 2.0, "z": 3.0, "w": 4.0}),
                },
            ],
            output_default: None,
        }
    }

    fn config_descriptor() -> PluginDescriptorV1 {
        PluginDescriptorV1 {
            name: "Runtime config test".to_string(),
            vendor: "Tests".to_string(),
            version: "1.0.0".to_string(),
            components: vec![style_component(), decorator_component()],
        }
    }

    fn descriptor_with(component: ComponentDescriptorV1) -> PluginDescriptorV1 {
        PluginDescriptorV1 {
            name: "Runtime property test".to_string(),
            vendor: "Tests".to_string(),
            version: "1.0.0".to_string(),
            components: vec![component],
        }
    }

    unsafe extern "C" fn invalid_property_response(
        _context: *mut std::ffi::c_void,
        _request: RuvieBytesView,
    ) -> RuvieCallResult {
        RuvieCallResult::ok_json(&serde_json::json!({
            "value": {"type": "future_unknown_value", "value": 99}
        }))
    }

    unsafe extern "C" fn failing_property_response(
        _context: *mut std::ffi::c_void,
        _request: RuvieBytesView,
    ) -> RuvieCallResult {
        RuvieCallResult::error(
            ruvie_plugin_api::STATUS_PLUGIN_ERROR,
            "intentional evaluator failure",
        )
    }

    unsafe extern "C" fn test_free_buffer(_context: *mut std::ffi::c_void, buffer: RuvieBuffer) {
        // SAFETY: `invalid_property_response` allocated this buffer with the
        // SDK helper, and the host returns it to this callback exactly once.
        unsafe { ruvie_plugin_api::free_owned_buffer(buffer) };
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
        unsupported.id = "example.unsupported_loader".to_string();
        unsupported.category = "loader".to_string();
        let descriptor = PluginDescriptorV1 {
            name: "Mixed".to_string(),
            vendor: "Tests".to_string(),
            version: "1.0.0".to_string(),
            components: vec![supported, unsupported],
        };
        let error = validate_descriptor(&descriptor)
            .expect_err("an unintegrated category must reject the bundle")
            .to_string();
        assert!(error.contains("uses category 'loader'"));
        assert!(error.contains("'style'"));
        assert!(error.contains("'decorator'"));
        assert!(error.contains("entire bundle was rejected"));
    }

    #[test]
    fn config_categories_register_typed_descriptor_backed_nodes_atomically() {
        use crate::model::NodeContent;
        use crate::model::project::{
            IMAGE_OUTPUT_PORT, PortDataType, SHAPE_INPUT_PORT, SHAPE_OUTPUT_PORT, TIME_PORT,
        };

        let mut registry = RuntimePluginRegistry::new();
        let mut effectors: PluginRepository<dyn EffectorPlugin> = PluginRepository::new();
        let mut decorators: PluginRepository<dyn DecoratorPlugin> = PluginRepository::new();
        let mut styles: PluginRepository<dyn StylePlugin> = PluginRepository::new();
        let mut property_evaluators = PropertyEvaluatorRegistry::new();
        let registered = registry
            .register_bundle(
                pending_bundle(config_descriptor()),
                &mut effectors,
                &mut decorators,
                &mut styles,
                &mut property_evaluators,
            )
            .expect("the complete low-bandwidth config bundle registers");
        assert_eq!(
            registered,
            vec![
                (
                    STYLE_CATEGORY.to_string(),
                    "example.runtime_fill".to_string()
                ),
                (
                    DECORATOR_CATEGORY.to_string(),
                    "example.runtime_backplate".to_string()
                ),
            ]
        );

        let style_descriptor = styles
            .get("example.runtime_fill")
            .expect("runtime Style adapter is in the Style repository")
            .descriptor()
            .expect("runtime Style descriptor is valid");
        let style_node = style_descriptor
            .create_node()
            .expect("runtime Style descriptor creates a Node");
        assert_eq!(style_node.properties.iter().count(), 2);
        let NodeContent::PluginOperation(style_operation) = style_node.content else {
            panic!("Style descriptor must create PluginOperation content")
        };
        assert_eq!(style_operation.category, crate::plugin::STYLE_CATEGORY);
        assert_eq!(
            style_operation.operation,
            crate::plugin::STYLE_APPLY_OPERATION
        );
        for (key, data_type) in [
            (TIME_PORT, PortDataType::Number),
            (SHAPE_INPUT_PORT, PortDataType::Shape),
            (IMAGE_OUTPUT_PORT, PortDataType::Image),
        ] {
            assert!(
                style_operation
                    .declared_ports
                    .iter()
                    .any(|port| port.key == key && port.data_type == data_type),
                "Style operation is missing typed port {key}"
            );
        }

        let decorator_descriptor = decorators
            .get("example.runtime_backplate")
            .expect("runtime Decorator adapter is in the Decorator repository")
            .descriptor()
            .expect("runtime Decorator descriptor is valid");
        let decorator_node = decorator_descriptor
            .create_node()
            .expect("runtime Decorator descriptor creates a Node");
        assert_eq!(decorator_node.properties.iter().count(), 2);
        let NodeContent::PluginOperation(decorator_operation) = decorator_node.content else {
            panic!("Decorator descriptor must create PluginOperation content")
        };
        assert_eq!(
            decorator_operation.category,
            crate::plugin::DECORATOR_CATEGORY
        );
        assert_eq!(
            decorator_operation.operation,
            crate::plugin::DECORATOR_APPLY_OPERATION
        );
        for (key, data_type) in [
            (TIME_PORT, PortDataType::Number),
            (SHAPE_INPUT_PORT, PortDataType::Shape),
            (SHAPE_OUTPUT_PORT, PortDataType::Shape),
        ] {
            assert!(
                decorator_operation
                    .declared_ports
                    .iter()
                    .any(|port| port.key == key && port.data_type == data_type),
                "Decorator operation is missing typed port {key}"
            );
        }
    }

    #[test]
    fn style_wire_conversion_covers_fill_and_stroke_and_rejects_invalid_output() {
        use crate::model::frame::draw_type::{CapType, DrawStyle, JoinType};

        let source_id = uuid::Uuid::new_v4();
        let fill = style_config_from_wire(
            StyleOutputV1::Fill {
                color: ColorV1 {
                    r: 1,
                    g: 2,
                    b: 3,
                    a: 4,
                },
                offset: 2.5,
            },
            source_id,
        )
        .expect("finite Fill converts")
        .expect("Fill produces a config");
        assert_eq!(fill.id, source_id, "the host owns Style config identity");
        assert_eq!(
            fill.style,
            DrawStyle::Fill {
                color: crate::model::frame::color::Color {
                    r: 1,
                    g: 2,
                    b: 3,
                    a: 4,
                },
                offset: 2.5,
            }
        );

        let stroke = style_config_from_wire(
            StyleOutputV1::Stroke {
                color: ColorV1 {
                    r: 5,
                    g: 6,
                    b: 7,
                    a: 8,
                },
                width: 3.0,
                offset: -1.0,
                cap: StrokeCapV1::Butt,
                join: StrokeJoinV1::Bevel,
                miter: 4.0,
                dash_array: vec![2.0, 1.0],
                dash_offset: 0.5,
            },
            source_id,
        )
        .expect("finite Stroke converts")
        .expect("Stroke produces a config");
        assert_eq!(
            stroke.style,
            DrawStyle::Stroke {
                color: crate::model::frame::color::Color {
                    r: 5,
                    g: 6,
                    b: 7,
                    a: 8,
                },
                width: 3.0,
                offset: -1.0,
                cap: CapType::Butt,
                join: JoinType::Bevel,
                miter: 4.0,
                dash_array: vec![2.0, 1.0],
                dash_offset: 0.5,
            }
        );

        assert!(
            style_config_from_wire(
                StyleOutputV1::Fill {
                    color: ColorV1 {
                        r: 0,
                        g: 0,
                        b: 0,
                        a: 0,
                    },
                    offset: f64::INFINITY,
                },
                source_id,
            )
            .is_err(),
            "non-finite output must not reach host StyleConfig"
        );
        assert!(
            style_config_from_wire(
                StyleOutputV1::Fill {
                    color: ColorV1 {
                        r: 0,
                        g: 0,
                        b: 0,
                        a: 255,
                    },
                    offset: f64::MAX,
                },
                source_id,
            )
            .is_err(),
            "finite f64 that overflows the renderer's scalar must be NoOutput"
        );
        assert!(
            style_config_from_wire(
                StyleOutputV1::Stroke {
                    color: ColorV1 {
                        r: 0,
                        g: 0,
                        b: 0,
                        a: 255,
                    },
                    width: f32::MAX as f64,
                    offset: f32::MAX as f64,
                    cap: StrokeCapV1::Round,
                    join: StrokeJoinV1::Round,
                    miter: 4.0,
                    dash_array: Vec::new(),
                    dash_offset: 0.0,
                },
                source_id,
            )
            .is_err(),
            "derived effective width must remain a finite renderer scalar"
        );
        for invalid_dash in [vec![1.0], vec![1.0, 0.0], vec![1.0, -1.0]] {
            assert!(
                style_config_from_wire(
                    StyleOutputV1::Stroke {
                        color: ColorV1 {
                            r: 0,
                            g: 0,
                            b: 0,
                            a: 255,
                        },
                        width: 1.0,
                        offset: 0.0,
                        cap: StrokeCapV1::Round,
                        join: StrokeJoinV1::Round,
                        miter: 4.0,
                        dash_array: invalid_dash,
                        dash_offset: 0.0,
                    },
                    source_id,
                )
                .is_err(),
                "unsafe dash config must become NoOutput"
            );
        }
        assert!(valid_stroke_dash_array(&[]));
        assert!(valid_stroke_dash_array(&[2.0, 1.0]));
        assert!(
            skia_safe::PathEffect::dash(&[1.0], 0.0).is_none(),
            "Skia rejects an odd number of dash intervals"
        );
        assert!(
            skia_safe::PathEffect::dash(&[0.0, 0.0], 0.0).is_none(),
            "Skia rejects an all-zero dash definition"
        );
        assert!(
            skia_safe::PathEffect::dash(&[2.0, 1.0], 0.0).is_some(),
            "the ABI's accepted dash shape is executable by Skia"
        );
        assert!(
            style_config_from_response(
                serde_json::json!({
                    "type": "fill",
                    "color": {"r": 0, "g": 0, "b": 0, "a": 255},
                    "offset": 0.0,
                    "undeclared": true
                }),
                source_id,
            )
            .is_err(),
            "undeclared plugin output fields are rejected"
        );
        assert!(
            style_config_from_response(serde_json::json!({"type": "future_style"}), source_id)
                .is_err(),
            "unknown output variants are rejected"
        );
        assert!(
            safe_style_config_from_response(
                serde_json::json!({
                    "type": "stroke",
                    "color": {"r": 0, "g": 0, "b": 0, "a": 255},
                    "width": 1.0,
                    "offset": 0.0,
                    "cap": "round",
                    "join": "round",
                    "miter": 4.0,
                    "dash_array": [1.0],
                    "dash_offset": 0.0
                }),
                source_id,
                "test runtime Style"
            )
            .is_none(),
            "a decoded but unsafe dash response must fail safely as NoOutput"
        );
    }

    #[test]
    fn decorator_wire_conversion_covers_backplate_without_exposing_parts() {
        use crate::core::ensemble::decorators::{BackplateShape, BackplateTarget};
        use crate::core::ensemble::types::DecoratorConfig;

        let output = decorator_config_from_wire(DecoratorOutputV1::Backplate {
            target: DecoratorTargetV1::Char,
            shape: BackplateShapeV1::RoundedRect,
            color: ColorV1 {
                r: 10,
                g: 20,
                b: 30,
                a: 40,
            },
            padding: InsetsV1 {
                top: 1.0,
                right: 2.0,
                bottom: 3.0,
                left: 4.0,
            },
            corner_radius: 5.0,
        })
        .expect("finite Backplate converts")
        .expect("Backplate produces a config");
        assert_eq!(
            output,
            DecoratorConfig::Backplate {
                target: BackplateTarget::Char,
                shape: BackplateShape::RoundedRect,
                color: crate::model::frame::color::Color {
                    r: 10,
                    g: 20,
                    b: 30,
                    a: 40,
                },
                padding: (1.0, 2.0, 3.0, 4.0),
                corner_radius: 5.0,
            }
        );

        assert!(
            decorator_config_from_wire(DecoratorOutputV1::Backplate {
                target: DecoratorTargetV1::Block,
                shape: BackplateShapeV1::Rect,
                color: ColorV1 {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 255,
                },
                padding: InsetsV1 {
                    top: f32::NAN,
                    right: 0.0,
                    bottom: 0.0,
                    left: 0.0,
                },
                corner_radius: 0.0,
            })
            .is_err(),
            "non-finite Backplate output must not reach the renderer"
        );
        assert!(
            decorator_config_from_response(serde_json::json!({
                "type": "backplate",
                "target": "parts",
                "shape": "rect",
                "color": {"r": 0, "g": 0, "b": 0, "a": 255},
                "padding": {"top": 0.0, "right": 0.0, "bottom": 0.0, "left": 0.0},
                "corner_radius": 0.0
            }))
            .is_err(),
            "the unsupported Parts target is not an ABI-v1 config"
        );
        assert!(
            safe_decorator_config_from_response(
                serde_json::json!({"type": "future_decorator"}),
                "test runtime Decorator"
            )
            .is_none(),
            "unknown Decorator output must fail safely as NoOutput"
        );
    }

    #[test]
    fn malformed_late_decorator_does_not_partially_register_an_earlier_style() {
        let mut malformed_decorator = decorator_component();
        malformed_decorator.properties[1].default =
            serde_json::json!({"x": 1.0, "y": 2.0, "z": 3.0, "w": 4.0, "extra": 5.0});
        let descriptor = PluginDescriptorV1 {
            name: "Atomic mixed config".to_string(),
            vendor: "Tests".to_string(),
            version: "1.0.0".to_string(),
            components: vec![style_component(), malformed_decorator],
        };
        let mut registry = RuntimePluginRegistry::new();
        let mut effectors: PluginRepository<dyn EffectorPlugin> = PluginRepository::new();
        let mut decorators: PluginRepository<dyn DecoratorPlugin> = PluginRepository::new();
        let mut styles: PluginRepository<dyn StylePlugin> = PluginRepository::new();
        let mut property_evaluators = PropertyEvaluatorRegistry::new();

        let error = registry
            .register_bundle(
                pending_bundle(descriptor),
                &mut effectors,
                &mut decorators,
                &mut styles,
                &mut property_evaluators,
            )
            .expect_err("a malformed later Decorator must reject the whole bundle")
            .to_string();
        assert!(error.contains("expected exactly finite number fields"));
        assert!(registry.components.is_empty());
        assert!(registry.descriptors.is_empty());
        assert!(registry.libraries.is_empty());
        assert!(effectors.plugins.is_empty());
        assert!(decorators.plugins.is_empty());
        assert!(styles.plugins.is_empty());
    }

    #[test]
    fn config_categories_require_their_versioned_operation_and_no_default_output() {
        for (mut component, operation) in [
            (style_component(), STYLE_EVALUATE_V1),
            (decorator_component(), DECORATOR_EVALUATE_V1),
        ] {
            component.operations.clear();
            let error = validate_descriptor(&descriptor_with(component.clone()))
                .expect_err("config component without its versioned evaluator is invalid")
                .to_string();
            assert!(error.contains(operation));

            component.operations.push(operation.to_string());
            component.output_default = Some(PropertyValueV1::Boolean { value: false });
            let error = validate_descriptor(&descriptor_with(component))
                .expect_err("NoOutput categories cannot declare a fabricated default")
                .to_string();
            assert!(error.contains("must not declare output_default"));
        }
    }

    #[test]
    fn property_category_requires_a_valid_explicit_output_default() {
        let valid = property_component(Some(PropertyValueV1::Number { value: 0.0 }));
        validate_descriptor(&descriptor_with(valid))
            .expect("property category and typed fail-safe are integrated in ABI v1");

        let missing = property_component(None);
        let error = validate_descriptor(&descriptor_with(missing))
            .expect_err("property evaluator without a fail-safe must be rejected")
            .to_string();
        assert!(error.contains("must declare output_default"));

        let non_finite = property_component(Some(PropertyValueV1::Number { value: f64::NAN }));
        let error = validate_descriptor(&descriptor_with(non_finite))
            .expect_err("non-finite fail-safe cannot cross JSON ABI v1")
            .to_string();
        assert!(error.contains("non-finite"));
    }

    #[test]
    fn invalid_property_response_logs_and_uses_descriptor_fail_safe() {
        let descriptor = property_component(Some(PropertyValueV1::Number { value: 7.0 }));
        let component = RuntimeComponent {
            descriptor: descriptor.clone(),
            library: Arc::new(RuntimeLibrary {
                api: RuviePluginApiV1 {
                    abi_version: RUVIE_PLUGIN_ABI_V1,
                    struct_size: size_of::<RuviePluginApiV1>(),
                    context: std::ptr::null_mut(),
                    descriptor_json: None,
                    invoke_json: Some(invalid_property_response),
                    free_buffer: Some(test_free_buffer),
                    query_extension: None,
                },
                _library: current_process_library(),
            }),
        };
        let evaluator = RuntimePropertyEvaluator {
            component,
            definitions: property_definitions(&descriptor).expect("test definition is valid"),
            output_default: PropertyValue::Number(OrderedFloat(7.0)),
        };
        let property = Property {
            evaluator: descriptor.id,
            properties: HashMap::new(),
        };
        let siblings = crate::model::property::PropertyMap::new();
        let context = EvaluationContext {
            property_map: &siblings,
            fps: 30.0,
        };
        assert_eq!(
            evaluator.evaluate(&property, 0.0, &context),
            PropertyValue::Number(OrderedFloat(7.0)),
            "invalid plugin output must use the descriptor-declared fail-safe"
        );
    }

    #[test]
    fn property_invocation_failure_uses_only_the_declared_fail_safe() {
        let descriptor = property_component(Some(PropertyValueV1::Number { value: 11.0 }));
        let evaluator = RuntimePropertyEvaluator {
            component: RuntimeComponent {
                descriptor: descriptor.clone(),
                library: Arc::new(RuntimeLibrary {
                    api: RuviePluginApiV1 {
                        abi_version: RUVIE_PLUGIN_ABI_V1,
                        struct_size: size_of::<RuviePluginApiV1>(),
                        context: std::ptr::null_mut(),
                        descriptor_json: None,
                        invoke_json: Some(failing_property_response),
                        free_buffer: Some(test_free_buffer),
                        query_extension: None,
                    },
                    _library: current_process_library(),
                }),
            },
            definitions: property_definitions(&descriptor).expect("test definition is valid"),
            output_default: PropertyValue::Number(OrderedFloat(11.0)),
        };
        let property = Property {
            evaluator: descriptor.id,
            properties: HashMap::new(),
        };
        let siblings = crate::model::property::PropertyMap::new();
        let context = EvaluationContext {
            property_map: &siblings,
            fps: 30.0,
        };
        assert_eq!(
            evaluator.evaluate(&property, 0.0, &context),
            PropertyValue::Number(OrderedFloat(11.0)),
            "plugin errors must not be disguised as an invented zero/default"
        );
    }

    #[test]
    fn late_definition_failure_does_not_partially_commit_a_bundle() {
        let resolved = ResolvedBundle {
            manifest_path: PathBuf::from("/runtime-plugin-test/ruvie-plugin.toml"),
            library_path: PathBuf::from("/runtime-plugin-test/plugin.test"),
        };
        let mut registry = RuntimePluginRegistry::new();
        let mut effectors: PluginRepository<dyn EffectorPlugin> = PluginRepository::new();
        let mut decorators: PluginRepository<dyn DecoratorPlugin> = PluginRepository::new();
        let mut styles: PluginRepository<dyn StylePlugin> = PluginRepository::new();
        let mut property_evaluators = PropertyEvaluatorRegistry::new();
        assert_eq!(
            registry.claim_bundle(&resolved),
            RuntimeBundleClaim::Claimed
        );

        let error = registry
            .register_bundle(
                pending_bundle(two_component_descriptor(serde_json::json!(101.0))),
                &mut effectors,
                &mut decorators,
                &mut styles,
                &mut property_evaluators,
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
        assert!(decorators.plugins.is_empty());
        assert!(styles.plugins.is_empty());
        assert!(!property_evaluators.contains("example.first"));

        assert_eq!(
            registry.claim_bundle(&resolved),
            RuntimeBundleClaim::Claimed
        );
        let registered = registry
            .register_bundle(
                pending_bundle(two_component_descriptor(serde_json::json!(50.0))),
                &mut effectors,
                &mut decorators,
                &mut styles,
                &mut property_evaluators,
            )
            .expect("a corrected rescan must not hit a stale partial-ID collision");
        assert_eq!(registered.len(), 2);
        assert!(effectors.get("example.first").is_some());
        assert!(effectors.get("example.second").is_some());
    }
}
