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
/// The component deliberately declines a typed request. Hosts may try the
/// next registered implementation without replacing the real failure cause.
pub const STATUS_UNSUPPORTED: u32 = 4;

/// Typed extension queried through [`RuviePluginApiV1::query_extension`].
pub const EFFECT_CPU_RGBA8_EXTENSION_V1: &str = "ruvie.effect.cpu-rgba8.v1";
/// Descriptor operation implemented by [`RuvieEffectCpuRgba8ApiV1`].
pub const EFFECT_PROCESS_CPU_RGBA8_V1: &str = "effect.process.cpu-rgba8.v1";
pub const EFFECT_CATEGORY: &str = "effect";
/// Typed extension queried through [`RuviePluginApiV1::query_extension`].
pub const LOADER_CPU_RGBA8_EXTENSION_V1: &str = "ruvie.loader.cpu-rgba8.v1";
pub const LOADER_OPEN_V1: &str = "loader.open.v1";
pub const LOADER_LOAD_CPU_RGBA8_V1: &str = "loader.load.cpu-rgba8.v1";
pub const LOADER_CATEGORY: &str = "loader";

/// Hard limits shared by ABI-v1 hosts and plugins before multiplying sizes.
pub const MAX_CPU_RGBA8_DIMENSION_V1: u32 = 32_768;
pub const MAX_CPU_RGBA8_FRAME_BYTES_V1: usize = 512 * 1024 * 1024;
pub const MAX_LOADER_STREAMS_V1: usize = 64;

pub const ALPHA_MODE_STRAIGHT_V1: u32 = 1;
pub const COLOR_PROFILE_SRGB_V1: u32 = 1;

pub const PROPERTY_VALUE_NUMBER_V1: u32 = 1;
pub const PROPERTY_VALUE_INTEGER_V1: u32 = 2;
pub const PROPERTY_VALUE_STRING_V1: u32 = 3;
pub const PROPERTY_VALUE_BOOLEAN_V1: u32 = 4;
pub const PROPERTY_VALUE_VEC2_V1: u32 = 5;
pub const PROPERTY_VALUE_VEC3_V1: u32 = 6;
pub const PROPERTY_VALUE_VEC4_V1: u32 = 7;
pub const PROPERTY_VALUE_COLOR_V1: u32 = 8;

pub const LOAD_REQUEST_IMAGE_V1: u32 = 1;
pub const LOAD_REQUEST_VIDEO_FRAME_V1: u32 = 2;
pub const ASSET_KIND_IMAGE_V1: u32 = 1;
pub const ASSET_KIND_VIDEO_V1: u32 = 2;
pub const ASSET_METADATA_DURATION_V1: u32 = 1 << 0;
pub const ASSET_METADATA_FPS_V1: u32 = 1 << 1;
pub const ASSET_METADATA_DIMENSIONS_V1: u32 = 1 << 2;
pub const ASSET_METADATA_STREAM_INDEX_V1: u32 = 1 << 3;
pub const ASSET_METADATA_FRAME_COUNT_V1: u32 = 1 << 4;
pub const ASSET_METADATA_TIME_BASE_V1: u32 = 1 << 5;

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

/// Result for typed extension callbacks. `message` is plugin-owned and is
/// returned exactly once through the base table's `free_buffer` callback.
/// Successful and unsupported calls normally use an empty message.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RuvieExtensionResultV1 {
    pub status: u32,
    pub message: RuvieBuffer,
}

impl RuvieExtensionResultV1 {
    pub const fn ok() -> Self {
        Self {
            status: STATUS_OK,
            message: RuvieBuffer::empty(),
        }
    }

    pub const fn unsupported() -> Self {
        Self {
            status: STATUS_UNSUPPORTED,
            message: RuvieBuffer::empty(),
        }
    }

    pub fn error(status: u32, message: impl Into<String>) -> Self {
        Self {
            status,
            message: RuvieBuffer::from_vec(message.into().into_bytes()),
        }
    }
}

/// One borrowed, explicitly tagged operation property. All pointed-to memory
/// is host-owned and valid only until the callback returns. Unused scalar,
/// vector, color, and byte fields are zero/empty.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RuviePropertyValueViewV1 {
    pub name: RuvieBytesView,
    pub value_type: u32,
    pub number: f64,
    pub integer: i64,
    pub bytes: RuvieBytesView,
    pub vector: [f64; 4],
    pub color: [u8; 4],
}

/// Borrowed array of resolved operation properties.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RuviePropertyMapViewV1 {
    pub ptr: *const RuviePropertyValueViewV1,
    pub len: usize,
}

/// Host-owned, borrowed straight-alpha sRGB RGBA8 pixels.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RuvieRgba8FrameViewV1 {
    pub struct_size: usize,
    pub width: u32,
    pub height: u32,
    pub stride_bytes: usize,
    pub alpha_mode: u32,
    pub color_profile: u32,
    pub pixels: RuvieBytesView,
}

/// Plugin-owned RGBA8 pixels. After any structurally reclaimable callback
/// output, the host calls the extension table's `free_frame` exactly once,
/// including when semantic validation rejects the frame.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RuvieOwnedRgba8FrameV1 {
    pub struct_size: usize,
    pub width: u32,
    pub height: u32,
    pub stride_bytes: usize,
    pub alpha_mode: u32,
    pub color_profile: u32,
    pub pixels: RuvieBuffer,
}

impl RuvieOwnedRgba8FrameV1 {
    pub const fn empty() -> Self {
        Self {
            struct_size: std::mem::size_of::<Self>(),
            width: 0,
            height: 0,
            stride_bytes: 0,
            alpha_mode: 0,
            color_profile: 0,
            pixels: RuvieBuffer::empty(),
        }
    }
}

/// Effect hot-path extension. Configuration is typed and borrowed during
/// `create_instance`; no JSON or Project object crosses the per-frame ABI.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RuvieEffectCpuRgba8ApiV1 {
    pub abi_version: u32,
    pub struct_size: usize,
    pub context: *mut c_void,
    pub create_instance: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            component_id: RuvieBytesView,
            properties: RuviePropertyMapViewV1,
            out_instance: *mut u64,
        ) -> RuvieExtensionResultV1,
    >,
    pub process: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            instance: u64,
            time_seconds: f64,
            input: *const RuvieRgba8FrameViewV1,
            output: *mut RuvieOwnedRgba8FrameV1,
        ) -> RuvieExtensionResultV1,
    >,
    pub release_instance: Option<unsafe extern "C" fn(context: *mut c_void, instance: u64)>,
    pub free_frame:
        Option<unsafe extern "C" fn(context: *mut c_void, frame: RuvieOwnedRgba8FrameV1)>,
}

// SAFETY: The extension contract requires its context and callbacks to be
// thread-safe for as long as the owning base API/library remains loaded.
unsafe impl Send for RuvieEffectCpuRgba8ApiV1 {}
// SAFETY: See the `Send` implementation above.
unsafe impl Sync for RuvieEffectCpuRgba8ApiV1 {}

/// Fixed-size metadata record filled into host-owned memory by Loader `open`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct RuvieAssetMetadataV1 {
    pub kind: u32,
    pub present_fields: u32,
    pub duration_seconds: f64,
    pub fps: f64,
    pub width: u32,
    pub height: u32,
    pub stream_index: u32,
    pub frame_count: u64,
    pub time_base_numerator: i32,
    pub time_base_denominator: i32,
}

/// One typed Loader request. Paths and color-space names are borrowed
/// host-owned UTF-8 bytes valid only for the callback.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct RuvieLoaderRequestV1 {
    pub struct_size: usize,
    pub request_kind: u32,
    pub path: RuvieBytesView,
    pub source_time: f64,
    pub has_stream_index: u32,
    pub stream_index: u32,
    pub input_color_space: RuvieBytesView,
    pub output_color_space: RuvieBytesView,
}

/// Loader hot-path extension. Metadata is written into a bounded host-owned
/// array; decoded pixels are plugin-owned until `free_frame` is called.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct RuvieLoaderCpuRgba8ApiV1 {
    pub abi_version: u32,
    pub struct_size: usize,
    pub context: *mut c_void,
    pub open: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            component_id: RuvieBytesView,
            path: RuvieBytesView,
            metadata: *mut RuvieAssetMetadataV1,
            metadata_capacity: usize,
            out_metadata_len: *mut usize,
        ) -> RuvieExtensionResultV1,
    >,
    pub load: Option<
        unsafe extern "C" fn(
            context: *mut c_void,
            component_id: RuvieBytesView,
            request: *const RuvieLoaderRequestV1,
            output: *mut RuvieOwnedRgba8FrameV1,
        ) -> RuvieExtensionResultV1,
    >,
    pub free_frame:
        Option<unsafe extern "C" fn(context: *mut c_void, frame: RuvieOwnedRgba8FrameV1)>,
}

// SAFETY: The extension contract requires its context and callbacks to be
// thread-safe for as long as the owning base API/library remains loaded.
unsafe impl Send for RuvieLoaderCpuRgba8ApiV1 {}
// SAFETY: See the `Send` implementation above.
unsafe impl Sync for RuvieLoaderCpuRgba8ApiV1 {}

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
    /// Value returned by the host when a runtime property evaluator cannot be
    /// invoked or returns an invalid response. Required for `property`
    /// components; its variant also declares the component's ABI-v1 output
    /// type. Omitted for categories whose failure contract is explicit
    /// `no_output` (Effector, Style, and Decorator).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_default: Option<PropertyValueV1>,
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
        #[serde(default = "default_vector_min")]
        min: f64,
        #[serde(default = "default_vector_max")]
        max: f64,
        #[serde(default = "default_vector_step")]
        step: f64,
        #[serde(default)]
        suffix: String,
        #[serde(default)]
        min_hard_limit: bool,
        #[serde(default)]
        max_hard_limit: bool,
    },
    Vec3 {
        #[serde(default = "default_vector_min")]
        min: f64,
        #[serde(default = "default_vector_max")]
        max: f64,
        #[serde(default = "default_vector_step")]
        step: f64,
        #[serde(default)]
        suffix: String,
        #[serde(default)]
        min_hard_limit: bool,
        #[serde(default)]
        max_hard_limit: bool,
    },
    Vec4 {
        #[serde(default = "default_vector_min")]
        min: f64,
        #[serde(default = "default_vector_max")]
        max: f64,
        #[serde(default = "default_vector_step")]
        step: f64,
        #[serde(default)]
        suffix: String,
        #[serde(default)]
        min_hard_limit: bool,
        #[serde(default)]
        max_hard_limit: bool,
    },
    Dropdown {
        options: Vec<String>,
    },
    Font,
}

fn default_vector_min() -> f64 {
    -1_000_000.0
}

fn default_vector_max() -> f64 {
    1_000_000.0
}

fn default_vector_step() -> f64 {
    0.1
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
pub const PROPERTY_CATEGORY: &str = "property";
pub const PROPERTY_EVALUATE_V1: &str = "property.evaluate.v1";
pub const STYLE_CATEGORY: &str = "style";
pub const STYLE_EVALUATE_V1: &str = "style.evaluate.v1";
/// Maximum non-empty Stroke dash interval count accepted by Style ABI v1.
pub const MAX_STYLE_DASH_INTERVALS_V1: usize = 1024;
pub const DECORATOR_CATEGORY: &str = "decorator";
pub const DECORATOR_EVALUATE_V1: &str = "decorator.evaluate.v1";
/// Geometry-only, two-Shape Decorator contract. Components advertise support
/// through [`ComponentDescriptorV1::operations`]; hosts prefer this operation
/// over [`DECORATOR_EVALUATE_V1`] when both are present.
pub const DECORATOR_EVALUATE_V2: &str = "decorator.evaluate.v2";

/// Explicit value wire format for runtime property evaluators.
///
/// This is intentionally separate from RuViE's Project model. The tag keeps
/// integer and floating-point values distinct and lets ABI v1 reject values it
/// cannot faithfully adapt instead of guessing from untagged JSON.
/// `Color` remains a legacy encoded straight-sRGBA8 value with no project
/// color-space tag; HDR graph colors must not be implicitly quantized into it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PropertyValueV1 {
    Number { value: f64 },
    Integer { value: i64 },
    String { value: String },
    Boolean { value: bool },
    Vec2 { x: f64, y: f64 },
    Vec3 { x: f64, y: f64, z: f64 },
    Vec4 { x: f64, y: f64, z: f64, w: f64 },
    Color { r: u8, g: u8, b: u8, a: u8 },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PropertyEvaluateRequestV1 {
    pub time: f64,
    pub fps: f64,
    pub properties: std::collections::BTreeMap<String, PropertyValueV1>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PropertyEvaluateResponseV1 {
    pub value: PropertyValueV1,
}

/// Resolved, evaluator-local inputs for one runtime Style operation.
///
/// The host resolves authored properties and scalar graph wires before this
/// request crosses the ABI. No Project, frame, path, or GPU object is exposed.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StyleEvaluateRequestV1 {
    pub time: f64,
    pub fps: f64,
    pub properties: std::collections::BTreeMap<String, PropertyValueV1>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ColorV1 {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StrokeCapV1 {
    Round,
    Square,
    Butt,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StrokeJoinV1 {
    Round,
    Bevel,
    Miter,
}

/// Complete ABI-v1 Style config output.
///
/// These variants intentionally mirror every current host `DrawStyle`
/// variant. A future host style requires a new explicitly versioned contract.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum StyleOutputV1 {
    NoOutput,
    Fill {
        color: ColorV1,
        offset: f64,
    },
    Stroke {
        color: ColorV1,
        width: f64,
        offset: f64,
        cap: StrokeCapV1,
        join: StrokeJoinV1,
        miter: f64,
        dash_array: Vec<f64>,
        dash_offset: f64,
    },
}

/// Resolved, evaluator-local inputs for one runtime Decorator operation.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct DecoratorEvaluateRequestV1 {
    pub time: f64,
    pub fps: f64,
    pub properties: std::collections::BTreeMap<String, PropertyValueV1>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DecoratorTargetV1 {
    Block,
    Line,
    Char,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackplateShapeV1 {
    Rect,
    RoundedRect,
    Circle,
}

/// Backplate padding in top, right, bottom, left order.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct InsetsV1 {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

/// Complete ABI-v1 Decorator config output.
///
/// `parts` is deliberately absent from [`DecoratorTargetV1`] because the
/// current host renderer cannot execute it safely.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum DecoratorOutputV1 {
    NoOutput,
    Backplate {
        target: DecoratorTargetV1,
        shape: BackplateShapeV1,
        color: ColorV1,
        padding: InsetsV1,
        corner_radius: f32,
    },
}

/// Resolved, evaluator-local inputs for the geometry-only two-Shape
/// Decorator contract. Unlike v1 requests, v2 is closed: malformed or future
/// fields require a separately negotiated protocol revision.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct DecoratorEvaluateRequestV2 {
    pub time: f64,
    pub fps: f64,
    pub properties: std::collections::BTreeMap<String, PropertyValueV1>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DecoratorTargetV2 {
    Block,
    Line,
    Char,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackplateFitV2 {
    Stretch,
    Contain,
    Cover,
}

/// Geometry-only Backplate padding in top, right, bottom, left order.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct InsetsV2 {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

/// Geometry-only Backplate placement offset in output pixels.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct BackplateOffsetV2 {
    pub x: f32,
    pub y: f32,
}

/// Complete geometry-only two-Shape Decorator output. Appearance fields are
/// deliberately absent and remain the responsibility of downstream Style.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum DecoratorOutputV2 {
    NoOutput,
    Backplate {
        target: DecoratorTargetV2,
        padding: InsetsV2,
        offset: BackplateOffsetV2,
        fit: BackplateFitV2,
    },
}

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
                output_default: None,
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

    #[test]
    fn property_values_are_explicitly_tagged_and_round_trip() {
        let request = PropertyEvaluateRequestV1 {
            time: 1.25,
            fps: 30.0,
            properties: std::collections::BTreeMap::from([
                ("amount".to_string(), PropertyValueV1::Number { value: 2.5 }),
                ("seed".to_string(), PropertyValueV1::Integer { value: 7 }),
            ]),
        };
        let json = serde_json::to_value(&request).expect("property request serializes");
        assert_eq!(json["properties"]["amount"]["type"], "number");
        assert_eq!(json["properties"]["seed"]["type"], "integer");
        let decoded: PropertyEvaluateRequestV1 =
            serde_json::from_value(json).expect("property request parses");
        assert_eq!(decoded, request);
    }

    #[test]
    fn style_and_decorator_outputs_are_tagged_and_strict() {
        let style = StyleOutputV1::Stroke {
            color: ColorV1 {
                r: 1,
                g: 2,
                b: 3,
                a: 4,
            },
            width: 2.0,
            offset: 0.5,
            cap: StrokeCapV1::Round,
            join: StrokeJoinV1::Miter,
            miter: 4.0,
            dash_array: vec![3.0, 2.0],
            dash_offset: 1.0,
        };
        let encoded = serde_json::to_value(&style).expect("style output serializes");
        assert_eq!(encoded["type"], "stroke");
        assert_eq!(
            serde_json::from_value::<StyleOutputV1>(encoded).expect("style output parses"),
            style
        );

        let unknown_field = serde_json::json!({
            "type": "backplate",
            "target": "block",
            "shape": "rect",
            "color": {"r": 0, "g": 0, "b": 0, "a": 255},
            "padding": {"top": 1.0, "right": 1.0, "bottom": 1.0, "left": 1.0},
            "corner_radius": 0.0,
            "unsupported": true
        });
        assert!(serde_json::from_value::<DecoratorOutputV1>(unknown_field).is_err());
    }

    #[test]
    fn frozen_decorator_v1_backplate_json_remains_compatible() {
        let frozen = serde_json::json!({
            "type": "backplate",
            "target": "line",
            "shape": "rounded_rect",
            "color": {"r": 1, "g": 2, "b": 3, "a": 4},
            "padding": {"top": -1.0, "right": 2.0, "bottom": 3.0, "left": 4.0},
            "corner_radius": 5.5
        });
        let output: DecoratorOutputV1 =
            serde_json::from_value(frozen.clone()).expect("frozen v1 output must parse");
        assert_eq!(
            output,
            DecoratorOutputV1::Backplate {
                target: DecoratorTargetV1::Line,
                shape: BackplateShapeV1::RoundedRect,
                color: ColorV1 {
                    r: 1,
                    g: 2,
                    b: 3,
                    a: 4,
                },
                padding: InsetsV1 {
                    top: -1.0,
                    right: 2.0,
                    bottom: 3.0,
                    left: 4.0,
                },
                corner_radius: 5.5,
            }
        );
        assert_eq!(
            serde_json::to_value(output).expect("v1 output serializes"),
            frozen
        );
    }

    #[test]
    fn frozen_v1_descriptor_operation_remains_compatible() {
        let frozen = serde_json::json!({
            "name": "Legacy Decorator",
            "vendor": "External Builder",
            "version": "1.0.0",
            "components": [{
                "id": "legacy.backplate",
                "name": "Legacy Backplate",
                "category": "decorator",
                "group": "Legacy",
                "version": "1.0.0",
                "operations": ["decorator.evaluate.v1"],
                "properties": []
            }]
        });
        let descriptor: PluginDescriptorV1 =
            serde_json::from_value(frozen.clone()).expect("frozen v1 descriptor must parse");
        assert_eq!(descriptor.components[0].operations, [DECORATOR_EVALUATE_V1]);
        assert_eq!(
            serde_json::to_value(descriptor).expect("v1 descriptor serializes"),
            frozen
        );
    }

    #[test]
    fn decorator_v2_wire_is_geometry_only_and_strict() {
        let valid = serde_json::json!({
            "type": "backplate",
            "target": "char",
            "padding": {"top": 1.0, "right": 2.0, "bottom": 3.0, "left": 4.0},
            "offset": {"x": 5.0, "y": -6.0},
            "fit": "cover"
        });
        assert!(serde_json::from_value::<DecoratorOutputV2>(valid).is_ok());
        for malformed in [
            serde_json::json!({
                "type": "backplate",
                "target": "char",
                "padding": {"top": 1.0, "right": 2.0, "bottom": 3.0, "left": 4.0},
                "offset": {"x": 5.0, "y": -6.0},
                "fit": "cover",
                "color": {"r": 0, "g": 0, "b": 0, "a": 255}
            }),
            serde_json::json!({
                "type": "backplate",
                "target": "char",
                "padding": {"top": 1.0, "right": 2.0, "bottom": 3.0, "left": 4.0, "extra": 0.0},
                "offset": {"x": 5.0, "y": -6.0},
                "fit": "cover"
            }),
        ] {
            assert!(serde_json::from_value::<DecoratorOutputV2>(malformed).is_err());
        }

        let request_with_future_field = serde_json::json!({
            "time": 1.0,
            "fps": 30.0,
            "properties": {},
            "future_host_hint": true
        });
        assert!(
            serde_json::from_value::<DecoratorEvaluateRequestV2>(request_with_future_field)
                .is_err()
        );
    }

    #[test]
    fn decorator_parts_target_is_not_exposed_by_abi_v1() {
        assert!(serde_json::from_str::<DecoratorTargetV1>(r#""parts""#).is_err());
    }

    #[test]
    fn config_requests_ignore_future_fields_but_outputs_remain_strict() {
        let request = serde_json::json!({
            "time": 1.0,
            "fps": 30.0,
            "properties": {},
            "future_host_hint": {"version": 2}
        });
        assert!(serde_json::from_value::<StyleEvaluateRequestV1>(request.clone()).is_ok());
        assert!(serde_json::from_value::<DecoratorEvaluateRequestV1>(request).is_ok());

        let output = serde_json::json!({
            "type": "fill",
            "color": {"r": 0, "g": 0, "b": 0, "a": 255},
            "offset": 0.0,
            "future_plugin_field": true
        });
        assert!(serde_json::from_value::<StyleOutputV1>(output).is_err());
    }
}
