//! CPU RGBA8 solid-tint effect extension.

use std::ffi::c_void;

use ruvie_plugin_api::{
    ComponentDescriptorV1, PropertyDefinitionV1, PropertyUiV1, RuvieBuffer, RuvieBytesView,
    RuvieEffectCpuRgba8ApiV1, RuvieExtensionResultV1, RuvieOwnedRgba8FrameV1,
    RuviePropertyMapViewV1, RuvieRgba8FrameViewV1, ALPHA_MODE_STRAIGHT_V1, COLOR_PROFILE_SRGB_V1,
    EFFECT_CATEGORY, EFFECT_PROCESS_CPU_RGBA8_V1, MAX_CPU_RGBA8_DIMENSION_V1,
    MAX_CPU_RGBA8_FRAME_BYTES_V1, PROPERTY_VALUE_INTEGER_V1, RUVIE_PLUGIN_ABI_V1,
};

use crate::abi::{bytes_from_view, extension_guard, free_frame, invalid_extension, utf8_from_view};

pub(super) const COMPONENT_ID: &str = "runtime_solid_tint_effect";

pub(super) fn descriptor() -> ComponentDescriptorV1 {
    ComponentDescriptorV1 {
        id: COMPONENT_ID.to_string(),
        name: "Runtime Solid Tint".to_string(),
        category: EFFECT_CATEGORY.to_string(),
        group: "Effect/Runtime Fixture".to_string(),
        version: "0.1.0".to_string(),
        operations: vec![EFFECT_PROCESS_CPU_RGBA8_V1.to_string()],
        properties: vec![PropertyDefinitionV1 {
            name: "red".to_string(),
            label: "Red".to_string(),
            ui: PropertyUiV1::Integer {
                min: 0,
                max: 255,
                suffix: String::new(),
                min_hard_limit: true,
                max_hard_limit: true,
            },
            default: serde_json::json!(220),
        }],
        output_default: None,
    }
}

unsafe extern "C" fn create_instance(
    _context: *mut c_void,
    component_id: RuvieBytesView,
    properties: RuviePropertyMapViewV1,
    out_instance: *mut u64,
) -> RuvieExtensionResultV1 {
    extension_guard(|| {
        if out_instance.is_null() {
            return invalid_extension("Effect out_instance is null");
        }
        // SAFETY: Component IDs are callback-scoped host byte views.
        let component_id = match unsafe { utf8_from_view(component_id) } {
            Ok(value) => value,
            Err(error) => return invalid_extension(error),
        };
        if component_id != COMPONENT_ID {
            return RuvieExtensionResultV1::unsupported();
        }
        if properties.len != 1 || properties.ptr.is_null() {
            return invalid_extension("Effect requires exactly one typed property");
        }
        // SAFETY: The host provides `len` initialized property views for this
        // callback and does not mutate them concurrently.
        let properties = unsafe { std::slice::from_raw_parts(properties.ptr, properties.len) };
        let property = &properties[0];
        // SAFETY: Property names are callback-scoped host byte views.
        let name = match unsafe { utf8_from_view(property.name) } {
            Ok(value) => value,
            Err(error) => return invalid_extension(error),
        };
        if name != "red"
            || property.value_type != PROPERTY_VALUE_INTEGER_V1
            || !(0..=255).contains(&property.integer)
        {
            return invalid_extension("Effect red must be an integer in 0..=255");
        }
        // Zero is reserved by the host, so encode red as one through 256.
        // SAFETY: The pointer was checked above and is uniquely writable for
        // this callback.
        unsafe { *out_instance = u64::try_from(property.integer).unwrap_or_default() + 1 };
        RuvieExtensionResultV1::ok()
    })
}

unsafe extern "C" fn process(
    _context: *mut c_void,
    instance: u64,
    time_seconds: f64,
    input: *const RuvieRgba8FrameViewV1,
    output: *mut RuvieOwnedRgba8FrameV1,
) -> RuvieExtensionResultV1 {
    extension_guard(|| {
        if input.is_null() || output.is_null() || !(1..=256).contains(&instance) {
            return invalid_extension("Effect frame pointer or instance is invalid");
        }
        if !time_seconds.is_finite() {
            return invalid_extension("Effect time must be finite");
        }
        // SAFETY: Both pointers are host-owned and valid for the callback.
        let input = unsafe { &*input };
        if input.struct_size < std::mem::size_of::<RuvieRgba8FrameViewV1>()
            || input.alpha_mode != ALPHA_MODE_STRAIGHT_V1
            || input.color_profile != COLOR_PROFILE_SRGB_V1
            || input.width == 0
            || input.height == 0
            || input.width > MAX_CPU_RGBA8_DIMENSION_V1
            || input.height > MAX_CPU_RGBA8_DIMENSION_V1
        {
            return invalid_extension("Effect input frame metadata is invalid");
        }
        let row_bytes = match usize::try_from(input.width)
            .ok()
            .and_then(|width| width.checked_mul(4))
        {
            Some(value) => value,
            None => return invalid_extension("Effect input row-byte overflow"),
        };
        let expected = match input
            .stride_bytes
            .checked_mul(usize::try_from(input.height).unwrap_or(usize::MAX))
        {
            Some(value) => value,
            None => return invalid_extension("Effect input frame-byte overflow"),
        };
        if input.stride_bytes < row_bytes
            || expected != input.pixels.len
            || expected > MAX_CPU_RGBA8_FRAME_BYTES_V1
        {
            return invalid_extension("Effect input stride or length is invalid");
        }
        // SAFETY: The complete input layout and non-null pointer contract are
        // checked by `bytes_from_view` before iteration.
        let pixels = match unsafe { bytes_from_view(input.pixels) } {
            Ok(value) => value,
            Err(error) => return invalid_extension(error),
        };
        let red = u8::try_from(instance - 1).unwrap_or_default();
        let mut tinted = Vec::with_capacity(pixels.len());
        for row in pixels.chunks_exact(input.stride_bytes) {
            for pixel in row[..row_bytes].chunks_exact(4) {
                if pixel[3] == 0 {
                    tinted.extend_from_slice(&[0, 0, 0, 0]);
                } else {
                    tinted.extend_from_slice(&[red, 0, 0, pixel[3]]);
                }
            }
            tinted.resize(tinted.len() + (input.stride_bytes - row_bytes), 0);
        }
        let frame = RuvieOwnedRgba8FrameV1 {
            struct_size: std::mem::size_of::<RuvieOwnedRgba8FrameV1>(),
            width: input.width,
            height: input.height,
            stride_bytes: input.stride_bytes,
            alpha_mode: ALPHA_MODE_STRAIGHT_V1,
            color_profile: COLOR_PROFILE_SRGB_V1,
            pixels: RuvieBuffer::from_vec(tinted),
        };
        // SAFETY: The output pointer is uniquely writable for this callback.
        unsafe { *output = frame };
        RuvieExtensionResultV1::ok()
    })
}

unsafe extern "C" fn release_instance(_context: *mut c_void, _instance: u64) {}

pub(super) static API: RuvieEffectCpuRgba8ApiV1 = RuvieEffectCpuRgba8ApiV1 {
    abi_version: RUVIE_PLUGIN_ABI_V1,
    struct_size: std::mem::size_of::<RuvieEffectCpuRgba8ApiV1>(),
    context: std::ptr::null_mut(),
    create_instance: Some(create_instance),
    process: Some(process),
    release_instance: Some(release_instance),
    free_frame: Some(free_frame),
};
