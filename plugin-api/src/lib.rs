//! Versioned, host-independent ABI and wire types for native RuViE plugins.
//!
//! This crate deliberately contains no editor or renderer implementation. A
//! plugin built after the host only needs this ABI crate and can communicate
//! with an already-built host through JSON payloads owned by the plugin.

use std::ffi::c_void;

use serde::{Deserialize, Serialize};

/// ABI version implemented by [`RuviePluginApiV1`].
pub const RUVIE_PLUGIN_ABI_V1: u32 = 1;
/// Symbol exported by every ABI-v1 native plugin library.
pub const RUVIE_PLUGIN_ENTRY_V1: &[u8] = b"ruvie_plugin_entry_v1";
/// Maximum payload that the reference host accepts from a native plugin.
pub const MAX_PLUGIN_PAYLOAD_BYTES: usize = 8 * 1024 * 1024;

pub const STATUS_OK: u32 = 0;
pub const STATUS_PLUGIN_ERROR: u32 = 1;
pub const STATUS_INVALID_REQUEST: u32 = 2;
pub const STATUS_PANIC: u32 = 3;

/// Borrowed bytes passed from the host to a plugin for one call.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RuvieBytesView {
    pub ptr: *const u8,
    pub len: usize,
}

impl RuvieBytesView {
    pub fn from_slice(value: &[u8]) -> Self {
        Self {
            ptr: value.as_ptr(),
            len: value.len(),
        }
    }
}

/// Bytes allocated by a plugin. The host copies them and calls `free_buffer`
/// from the same plugin before returning from the invocation.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RuvieBuffer {
    pub ptr: *mut u8,
    pub len: usize,
    pub capacity: usize,
}

impl RuvieBuffer {
    pub const fn empty() -> Self {
        Self {
            ptr: std::ptr::null_mut(),
            len: 0,
            capacity: 0,
        }
    }

    pub fn from_vec(mut value: Vec<u8>) -> Self {
        let buffer = Self {
            ptr: value.as_mut_ptr(),
            len: value.len(),
            capacity: value.capacity(),
        };
        std::mem::forget(value);
        buffer
    }
}

/// Result returned by all fallible plugin callbacks.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RuvieCallResult {
    pub status: u32,
    pub buffer: RuvieBuffer,
}

impl RuvieCallResult {
    pub fn ok_json<T: Serialize>(value: &T) -> Self {
        match serde_json::to_vec(value) {
            Ok(bytes) => Self {
                status: STATUS_OK,
                buffer: RuvieBuffer::from_vec(bytes),
            },
            Err(error) => Self::error(STATUS_PLUGIN_ERROR, error.to_string()),
        }
    }

    pub fn error(status: u32, message: impl Into<String>) -> Self {
        Self {
            status,
            buffer: RuvieBuffer::from_vec(message.into().into_bytes()),
        }
    }
}

/// Reclaims a buffer previously returned by [`RuvieBuffer::from_vec`].
///
/// # Safety
///
/// `buffer` must have been produced by `RuvieBuffer::from_vec` in the same
/// dynamic library and must not have been reclaimed before.
pub unsafe extern "C" fn free_owned_buffer(buffer: RuvieBuffer) {
    if buffer.ptr.is_null() {
        return;
    }
    // SAFETY: The function contract requires the exact pointer, length, and
    // capacity produced by `RuvieBuffer::from_vec`, reclaimed exactly once.
    let _owned = unsafe { Vec::from_raw_parts(buffer.ptr, buffer.len, buffer.capacity) };
}

/// Stable function table returned by `ruvie_plugin_entry_v1`.
///
/// Plugins must catch panics inside both callbacks. Unwinding through this C
/// boundary is undefined behaviour; the SDK exposes `STATUS_PANIC` for that
/// purpose.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RuviePluginApiV1 {
    pub abi_version: u32,
    pub struct_size: usize,
    pub context: *mut c_void,
    pub descriptor_json: Option<unsafe extern "C" fn(context: *mut c_void) -> RuvieCallResult>,
    pub invoke_json: Option<
        unsafe extern "C" fn(context: *mut c_void, request: RuvieBytesView) -> RuvieCallResult,
    >,
    pub free_buffer: Option<unsafe extern "C" fn(context: *mut c_void, buffer: RuvieBuffer)>,
    /// Optional discovery hook for versioned, typed extension tables such as
    /// future host-owned frame/resource-handle APIs. Returned tables remain
    /// owned by the plugin and valid while its library is loaded.
    pub query_extension: Option<
        unsafe extern "C" fn(context: *mut c_void, extension_name: RuvieBytesView) -> *const c_void,
    >,
}

// The context belongs to a loaded plugin and callbacks are required to be
// thread-safe. The host never dereferences it.
// SAFETY: The ABI contract requires callbacks and their opaque context to be
// safe for concurrent host calls for the lifetime of the loaded library.
unsafe impl Send for RuviePluginApiV1 {}
// SAFETY: See the `Send` implementation and the ABI callback contract above.
unsafe impl Sync for RuviePluginApiV1 {}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PluginDescriptorV1 {
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub components: Vec<ComponentDescriptorV1>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ComponentDescriptorV1 {
    pub id: String,
    pub name: String,
    pub category: String,
    #[serde(default)]
    pub group: String,
    pub version: String,
    #[serde(default)]
    pub operations: Vec<String>,
    #[serde(default)]
    pub properties: Vec<PropertyDefinitionV1>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PropertyDefinitionV1 {
    pub name: String,
    pub label: String,
    pub ui: PropertyUiV1,
    pub default: serde_json::Value,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PropertyUiV1 {
    Float {
        min: f64,
        max: f64,
        step: f64,
        #[serde(default)]
        suffix: String,
        #[serde(default)]
        min_hard_limit: bool,
        #[serde(default)]
        max_hard_limit: bool,
    },
    Integer {
        min: i64,
        max: i64,
        #[serde(default)]
        suffix: String,
        #[serde(default)]
        min_hard_limit: bool,
        #[serde(default)]
        max_hard_limit: bool,
    },
    Color,
    Text,
    MultilineText,
    Bool,
    Vec2 {
        #[serde(default)]
        suffix: String,
    },
    Vec3 {
        #[serde(default)]
        suffix: String,
    },
    Vec4 {
        #[serde(default)]
        suffix: String,
    },
    Dropdown {
        options: Vec<String>,
    },
    Font,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct InvokeRequestV1 {
    pub component_id: String,
    pub category: String,
    pub operation: String,
    pub payload: serde_json::Value,
}

pub const EFFECTOR_CATEGORY: &str = "effector";
pub const EFFECTOR_EVALUATE_V1: &str = "effector.evaluate.v1";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EffectorEvaluateRequestV1 {
    pub time: f64,
    pub properties: std::collections::BTreeMap<String, serde_json::Value>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EffectorTargetV1 {
    Block,
    Line,
    Char,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OpacityModeV1 {
    Set,
    Add,
    Multiply,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EffectorOutputV1 {
    NoOutput,
    Transform {
        translate: (f32, f32),
        rotate: f32,
        scale: (f32, f32),
        target: EffectorTargetV1,
    },
    Opacity {
        opacity: f32,
        mode: OpacityModeV1,
        target: EffectorTargetV1,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_wire_format_round_trips() {
        let descriptor = PluginDescriptorV1 {
            name: "Example".to_string(),
            vendor: "Third Party".to_string(),
            version: "1.2.3".to_string(),
            components: vec![ComponentDescriptorV1 {
                id: "example.opacity".to_string(),
                name: "Opacity".to_string(),
                category: EFFECTOR_CATEGORY.to_string(),
                group: "Example".to_string(),
                version: "1.2.3".to_string(),
                operations: vec![EFFECTOR_EVALUATE_V1.to_string()],
                properties: vec![PropertyDefinitionV1 {
                    name: "target".to_string(),
                    label: "Target".to_string(),
                    ui: PropertyUiV1::Dropdown {
                        options: vec!["Block".to_string(), "Char".to_string()],
                    },
                    default: serde_json::json!("Block"),
                }],
            }],
        };
        let bytes = serde_json::to_vec(&descriptor).expect("test descriptor serializes");
        let decoded: PluginDescriptorV1 =
            serde_json::from_slice(&bytes).expect("test descriptor parses");
        assert_eq!(decoded, descriptor);
    }

    #[test]
    fn unsupported_parts_target_is_not_part_of_abi_v1() {
        let error = serde_json::from_str::<EffectorTargetV1>(r#""parts""#)
            .expect_err("ABI v1 must reject the unimplemented Parts target");
        assert!(error.to_string().contains("unknown variant"));
    }
}
