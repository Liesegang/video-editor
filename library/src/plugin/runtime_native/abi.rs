use std::mem::{align_of, size_of};
use std::path::Path;
use std::sync::Arc;

use libloading::{Library, Symbol};
use ruvie_plugin_api::{
    ComponentDescriptorV1, EFFECT_CPU_RGBA8_EXTENSION_V1, InvokeRequestV1,
    LOADER_CPU_RGBA8_EXTENSION_V1, MAX_PLUGIN_PAYLOAD_BYTES, RUVIE_PLUGIN_ABI_V1,
    RUVIE_PLUGIN_ENTRY_V1, RuvieBuffer, RuvieBytesView, RuvieCallResult, RuvieEffectCpuRgba8ApiV1,
    RuvieExtensionResultV1, RuvieLoaderCpuRgba8ApiV1, RuviePluginApiV1, STATUS_OK,
    STATUS_UNSUPPORTED,
};

use crate::error::LibraryError;

#[repr(C)]
struct AbiTableHeader {
    abi_version: u32,
    struct_size: usize,
}

pub(super) fn copy_abi_table<T: Copy>(
    pointer: *const std::ffi::c_void,
    label: &str,
) -> Result<T, LibraryError> {
    if pointer.is_null() {
        return Err(LibraryError::Plugin(format!(
            "{label} returned a null ABI table"
        )));
    }
    let required_alignment = align_of::<T>();
    if !(pointer as usize).is_multiple_of(required_alignment) {
        return Err(LibraryError::Plugin(format!(
            "{label} returned a misaligned ABI table; host requires {required_alignment}-byte alignment"
        )));
    }

    let header = pointer.cast::<AbiTableHeader>();
    // SAFETY: The pointer is non-null and aligned for `T`, which begins with
    // this ABI header. Native plugins are trusted to return readable table
    // storage; raw field reads avoid creating a reference before the complete
    // table size has been validated.
    let abi_version = unsafe { std::ptr::addr_of!((*header).abi_version).read() };
    // SAFETY: Same validated header prefix as above.
    let struct_size = unsafe { std::ptr::addr_of!((*header).struct_size).read() };
    if abi_version != RUVIE_PLUGIN_ABI_V1 || struct_size < size_of::<T>() {
        return Err(LibraryError::Plugin(format!(
            "{label} ABI mismatch: version {abi_version}, table {struct_size} bytes; host requires v{} and at least {} bytes",
            RUVIE_PLUGIN_ABI_V1,
            size_of::<T>()
        )));
    }

    // SAFETY: Nullness, alignment, version, and complete table size were
    // checked above. `T: Copy`, so the plugin retains ownership of its static
    // table while the host takes an inert value copy.
    Ok(unsafe { pointer.cast::<T>().read() })
}

pub(super) struct RuntimeLibrary {
    pub(super) api: RuviePluginApiV1,
    pub(super) _library: Library,
}

impl RuntimeLibrary {
    pub(super) fn open(path: &Path) -> Result<Self, LibraryError> {
        // SAFETY: Loading native code is restricted to an explicitly configured
        // manifest bundle. Native plugins are trusted in-process extensions.
        let library = unsafe { Library::new(path)? };
        // SAFETY: The symbol is validated by checking the returned table's ABI
        // version, size, and required callbacks before any callback is invoked.
        let entry: Symbol<unsafe extern "C" fn() -> *const RuviePluginApiV1> =
            unsafe { library.get(RUVIE_PLUGIN_ENTRY_V1)? };
        // SAFETY: Calling the versioned entry symbol is the ABI-v1 contract.
        let api_ptr = unsafe { entry() };
        let api = copy_abi_table::<RuviePluginApiV1>(
            api_ptr.cast(),
            &format!("Runtime plugin {}", path.display()),
        )?;
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

    pub(super) fn descriptor<T: serde::de::DeserializeOwned>(&self) -> Result<T, LibraryError> {
        let descriptor = self.api.descriptor_json.ok_or_else(|| {
            LibraryError::Plugin("Runtime plugin descriptor callback is missing".to_string())
        })?;
        // SAFETY: Callback presence and table version were validated at load.
        let result = unsafe { descriptor(self.api.context) };
        let bytes = self.copy_and_free(result)?;
        serde_json::from_slice(&bytes).map_err(LibraryError::Json)
    }

    pub(super) fn invoke(
        &self,
        request: &InvokeRequestV1,
    ) -> Result<serde_json::Value, LibraryError> {
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

    pub(super) fn effect_cpu_rgba8_extension(
        &self,
    ) -> Result<RuvieEffectCpuRgba8ApiV1, LibraryError> {
        let api =
            self.query_extension::<RuvieEffectCpuRgba8ApiV1>(EFFECT_CPU_RGBA8_EXTENSION_V1)?;
        if api.create_instance.is_none()
            || api.process.is_none()
            || api.release_instance.is_none()
            || api.free_frame.is_none()
        {
            return Err(LibraryError::Plugin(format!(
                "Runtime extension {EFFECT_CPU_RGBA8_EXTENSION_V1} is missing a required callback"
            )));
        }
        Ok(api)
    }

    pub(super) fn loader_cpu_rgba8_extension(
        &self,
    ) -> Result<RuvieLoaderCpuRgba8ApiV1, LibraryError> {
        let api =
            self.query_extension::<RuvieLoaderCpuRgba8ApiV1>(LOADER_CPU_RGBA8_EXTENSION_V1)?;
        if api.open.is_none() || api.load.is_none() || api.free_frame.is_none() {
            return Err(LibraryError::Plugin(format!(
                "Runtime extension {LOADER_CPU_RGBA8_EXTENSION_V1} is missing a required callback"
            )));
        }
        Ok(api)
    }

    fn query_extension<T: Copy>(&self, name: &str) -> Result<T, LibraryError> {
        let query = self.api.query_extension.ok_or_else(|| {
            LibraryError::Plugin(format!(
                "Runtime plugin does not expose query_extension required by {name}"
            ))
        })?;
        let name_bytes = name.as_bytes();
        // SAFETY: The loaded base table was validated. The extension name is a
        // borrowed host slice alive for the call; plugins return a table that
        // remains valid for the lifetime of the loaded library.
        let pointer = unsafe { query(self.api.context, RuvieBytesView::from_slice(name_bytes)) };
        if pointer.is_null() {
            return Err(LibraryError::Plugin(format!(
                "Runtime plugin does not implement extension {name}"
            )));
        }
        copy_abi_table::<T>(pointer, &format!("Runtime extension {name}"))
    }

    pub(super) fn consume_extension_result(
        &self,
        result: RuvieExtensionResultV1,
    ) -> Result<ExtensionStatus, LibraryError> {
        let message = self.copy_plugin_buffer(
            result.message,
            MAX_PLUGIN_PAYLOAD_BYTES,
            "extension message",
        )?;
        match result.status {
            STATUS_OK => Ok(ExtensionStatus::Ok),
            STATUS_UNSUPPORTED => Ok(ExtensionStatus::Unsupported(
                String::from_utf8_lossy(&message).into_owned(),
            )),
            status => Err(LibraryError::Plugin(format!(
                "Runtime plugin extension call failed with status {status}: {}",
                String::from_utf8_lossy(&message)
            ))),
        }
    }

    fn copy_and_free(&self, result: RuvieCallResult) -> Result<Vec<u8>, LibraryError> {
        let bytes = self.copy_plugin_buffer(
            result.buffer,
            MAX_PLUGIN_PAYLOAD_BYTES,
            "JSON/control payload",
        )?;
        if result.status != STATUS_OK {
            return Err(LibraryError::Plugin(format!(
                "Runtime plugin call failed with status {}: {}",
                result.status,
                String::from_utf8_lossy(&bytes)
            )));
        }
        Ok(bytes)
    }

    fn copy_plugin_buffer(
        &self,
        buffer: RuvieBuffer,
        maximum_len: usize,
        label: &str,
    ) -> Result<Vec<u8>, LibraryError> {
        let RuvieBuffer { ptr, len, capacity } = buffer;
        let structurally_reclaimable =
            (!ptr.is_null() && capacity >= len) || (ptr.is_null() && len == 0 && capacity == 0);
        let invalid = len > maximum_len
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
                unsafe { free(self.api.context, buffer) };
            }
            // A null pointer with non-zero length/capacity or len > capacity
            // cannot be passed to the reference Vec-based deallocator safely.
            // Such a trusted native plugin has already violated the ABI; the
            // host reports it and intentionally cannot reclaim that buffer.
            return Err(LibraryError::Plugin(format!(
                "Runtime plugin returned an invalid {label} buffer (len={len}, capacity={capacity})"
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
        unsafe { free(self.api.context, buffer) };
        Ok(bytes)
    }
}

#[derive(Debug)]
pub(super) enum ExtensionStatus {
    Ok,
    Unsupported(String),
}

#[derive(Clone)]
pub(crate) struct RuntimeComponent {
    pub(super) descriptor: ComponentDescriptorV1,
    pub(super) library: Arc<RuntimeLibrary>,
}

impl RuntimeComponent {
    pub(crate) fn invoke(
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
