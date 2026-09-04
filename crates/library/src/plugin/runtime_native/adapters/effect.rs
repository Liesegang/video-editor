use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex, MutexGuard};

use lru::LruCache;
use ruvie_plugin_api::{
    RuvieBytesView, RuvieEffectCpuRgba8ApiV1, RuvieOwnedRgba8FrameV1, RuviePropertyMapViewV1,
};

use super::super::RUNTIME_EFFECT_TIME_PROPERTY;
use super::super::abi::{ExtensionStatus, RuntimeComponent, RuntimeLibrary};
use super::super::property_wire::property_views;
use super::super::rgba8::{copy_owned_frame, reclaim_owned_frame, rgba8_view};
use super::parse_semver_triplet;
use crate::error::LibraryError;
use crate::model::property::{PropertyDefinition, PropertyValue};
use crate::plugin::{EffectPlugin, Plugin};
const RUNTIME_EFFECT_INSTANCE_CACHE_SIZE: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(in crate::plugin::runtime_native) struct EffectConfigKey(
    pub(in crate::plugin::runtime_native) Vec<(String, PropertyValue)>,
);

struct RuntimeEffectInstance {
    handle: u64,
    api: RuvieEffectCpuRgba8ApiV1,
    // Retain the dynamic library until the release callback has returned.
    _library: Arc<RuntimeLibrary>,
}

impl Drop for RuntimeEffectInstance {
    fn drop(&mut self) {
        if let Some(release) = self.api.release_instance {
            // SAFETY: A non-zero handle is returned by this same extension's
            // create callback and released exactly once when its Arc expires.
            unsafe { release(self.api.context, self.handle) };
        }
    }
}

pub(in crate::plugin::runtime_native) struct RuntimeEffectPlugin {
    pub(in crate::plugin::runtime_native) component: RuntimeComponent,
    pub(in crate::plugin::runtime_native) definitions: Vec<PropertyDefinition>,
    pub(in crate::plugin::runtime_native) api: RuvieEffectCpuRgba8ApiV1,
    instances: Mutex<LruCache<EffectConfigKey, Arc<RuntimeEffectInstance>>>,
}

impl RuntimeEffectPlugin {
    pub(in crate::plugin::runtime_native) fn new(
        component: RuntimeComponent,
        definitions: Vec<PropertyDefinition>,
        api: RuvieEffectCpuRgba8ApiV1,
    ) -> Result<Self, LibraryError> {
        let capacity =
            NonZeroUsize::new(RUNTIME_EFFECT_INSTANCE_CACHE_SIZE).unwrap_or(NonZeroUsize::MIN);
        Ok(Self {
            component,
            definitions,
            api,
            instances: Mutex::new(LruCache::new(capacity)),
        })
    }

    fn lock_instances(
        &self,
    ) -> MutexGuard<'_, LruCache<EffectConfigKey, Arc<RuntimeEffectInstance>>> {
        self.instances.lock().unwrap_or_else(|poisoned| {
            log::error!(
                "Runtime Effect '{}' instance cache was poisoned; recovering committed entries",
                self.id()
            );
            poisoned.into_inner()
        })
    }

    pub(in crate::plugin::runtime_native) fn config_key(
        &self,
        params: &HashMap<String, PropertyValue>,
    ) -> Result<EffectConfigKey, LibraryError> {
        let mut entries = Vec::with_capacity(self.definitions.len());
        for definition in &self.definitions {
            let value = params
                .get(definition.name())
                .unwrap_or_else(|| definition.default_value());
            definition.validate_value(value).map_err(|error| {
                LibraryError::Plugin(format!(
                    "Runtime Effect '{}.{}' received an invalid value: {error}",
                    self.id(),
                    definition.name()
                ))
            })?;
            entries.push((definition.name().to_string(), value.clone()));
        }
        Ok(EffectConfigKey(entries))
    }

    fn instance_for(
        &self,
        key: &EffectConfigKey,
    ) -> Result<Arc<RuntimeEffectInstance>, LibraryError> {
        if let Some(instance) = self.lock_instances().get(key).cloned() {
            return Ok(instance);
        }

        // Plugin code runs outside the instance-cache and manager locks. A
        // concurrent miss may create the same immutable config twice; the
        // loser is safely released after the second cache check.
        let properties = property_views(&key.0)?;
        let map = RuviePropertyMapViewV1 {
            ptr: if properties.is_empty() {
                std::ptr::null()
            } else {
                properties.as_ptr()
            },
            len: properties.len(),
        };
        let create = self.api.create_instance.ok_or_else(|| {
            LibraryError::Plugin(format!(
                "Runtime Effect '{}' create callback is missing",
                self.id()
            ))
        })?;
        let mut handle = 0_u64;
        // SAFETY: All property views borrow `key`, which remains alive and
        // immutable through the callback. `handle` is writable host memory.
        let result = unsafe {
            create(
                self.api.context,
                RuvieBytesView::from_slice(self.id().as_bytes()),
                map,
                &mut handle,
            )
        };
        match self.component.library.consume_extension_result(result)? {
            ExtensionStatus::Ok => {}
            ExtensionStatus::Unsupported(message) => {
                return Err(LibraryError::Plugin(format!(
                    "Runtime Effect '{}' declined its declared config: {message}",
                    self.id()
                )));
            }
        }
        if handle == 0 {
            return Err(LibraryError::Plugin(format!(
                "Runtime Effect '{}' returned an invalid zero instance handle",
                self.id()
            )));
        }
        let created = Arc::new(RuntimeEffectInstance {
            handle,
            api: self.api,
            _library: Arc::clone(&self.component.library),
        });
        let mut instances = self.lock_instances();
        if let Some(existing) = instances.get(key).cloned() {
            return Ok(existing);
        }
        let evicted = instances.put(key.clone(), Arc::clone(&created));
        drop(instances);
        // Releasing an opaque plugin instance is a plugin callback. Never run
        // it while the local cache mutex is held, so a callback can safely
        // re-enter another operation on this adapter.
        drop(evicted);
        Ok(created)
    }
}

impl Plugin for RuntimeEffectPlugin {
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
        "Native ABI v1 / CPU RGBA8".to_string()
    }
}

impl EffectPlugin for RuntimeEffectPlugin {
    fn apply(
        &self,
        input: &crate::rendering::renderer::RenderOutput,
        params: &HashMap<String, PropertyValue>,
        _gpu_context: Option<&mut crate::rendering::skia_utils::GpuContext>,
    ) -> Result<crate::rendering::renderer::RenderOutput, LibraryError> {
        let crate::rendering::renderer::RenderOutput::Image(input) = input else {
            return Err(LibraryError::Plugin(format!(
                "Runtime Effect '{}' requires a CPU Image input",
                self.id()
            )));
        };
        let input_view = rgba8_view(input)?;
        let time = match params.get(RUNTIME_EFFECT_TIME_PROPERTY) {
            Some(PropertyValue::Number(value)) if value.is_finite() => value.into_inner(),
            Some(_) => {
                return Err(LibraryError::Plugin(format!(
                    "Runtime Effect '{}' received an invalid render time",
                    self.id()
                )));
            }
            None => 0.0,
        };
        let key = self.config_key(params)?;
        let instance = self.instance_for(&key)?;
        let process = self.api.process.ok_or_else(|| {
            LibraryError::Plugin(format!(
                "Runtime Effect '{}' process callback is missing",
                self.id()
            ))
        })?;
        let mut output = RuvieOwnedRgba8FrameV1::empty();
        // SAFETY: Input pixels are borrowed for the call; output is writable
        // host memory initialized to the empty ownership state. The instance
        // Arc retains both its handle and the dynamic library.
        let result = unsafe {
            process(
                self.api.context,
                instance.handle,
                time,
                &input_view,
                &mut output,
            )
        };
        let status = self.component.library.consume_extension_result(result);
        match status {
            Ok(ExtensionStatus::Ok) => {}
            Ok(ExtensionStatus::Unsupported(message)) => {
                reclaim_owned_frame(self.api.context, self.api.free_frame, output);
                return Err(LibraryError::Plugin(format!(
                    "Runtime Effect '{}' declined an RGBA8 frame: {message}",
                    self.id()
                )));
            }
            Err(error) => {
                reclaim_owned_frame(self.api.context, self.api.free_frame, output);
                return Err(LibraryError::Plugin(format!(
                    "Runtime Effect '{}' failed: {error}",
                    self.id()
                )));
            }
        }
        let image = copy_owned_frame(self.api.context, self.api.free_frame, output)?;
        Ok(crate::rendering::renderer::RenderOutput::Image(image))
    }

    fn properties(&self) -> Vec<PropertyDefinition> {
        self.definitions.clone()
    }
}
