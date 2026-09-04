//! Runtime plugin fixture exposing property, style, decorator, effect, and loader components.

use std::ffi::c_void;

use ruvie_plugin_api::{
    InvokeRequestV1, PluginDescriptorV1, RuvieBytesView, RuvieCallResult, RuviePluginApiV1,
    DECORATOR_CATEGORY, DECORATOR_EVALUATE_V1, DECORATOR_EVALUATE_V2,
    EFFECT_CPU_RGBA8_EXTENSION_V1, LOADER_CPU_RGBA8_EXTENSION_V1, PROPERTY_CATEGORY,
    PROPERTY_EVALUATE_V1, RUVIE_PLUGIN_ABI_V1, STATUS_INVALID_REQUEST, STYLE_CATEGORY,
    STYLE_EVALUATE_V1,
};

mod abi;
mod backplate;
mod component_request;
mod cpu_effect;
mod descriptors;
mod random_property;
mod rgba_fixture_loader;
mod style;

fn descriptor() -> PluginDescriptorV1 {
    PluginDescriptorV1 {
        name: "Random Property".to_string(),
        vendor: "RuViE".to_string(),
        version: "0.1.0".to_string(),
        components: vec![
            random_property::descriptor(),
            style::fill_descriptor(),
            style::stroke_descriptor(),
            backplate::descriptor(),
            cpu_effect::descriptor(),
            rgba_fixture_loader::descriptor(),
        ],
    }
}

unsafe extern "C" fn descriptor_json(_context: *mut c_void) -> RuvieCallResult {
    abi::call_guard(|| {
        random_property::record_descriptor_call();
        RuvieCallResult::ok_json(&descriptor())
    })
}

unsafe extern "C" fn invoke_json(
    _context: *mut c_void,
    request: RuvieBytesView,
) -> RuvieCallResult {
    abi::call_guard(|| {
        if request.ptr.is_null() || request.len == 0 {
            return RuvieCallResult::error(STATUS_INVALID_REQUEST, "empty request");
        }
        // SAFETY: The ABI guarantees that the host-owned request remains valid
        // and immutable for exactly this callback invocation.
        let bytes = unsafe { std::slice::from_raw_parts(request.ptr, request.len) };
        let request: InvokeRequestV1 = match serde_json::from_slice(bytes) {
            Ok(request) => request,
            Err(error) => {
                return RuvieCallResult::error(STATUS_INVALID_REQUEST, error.to_string());
            }
        };
        let InvokeRequestV1 {
            component_id,
            category,
            operation,
            payload,
        } = request;
        match (category.as_str(), component_id.as_str(), operation.as_str()) {
            (
                PROPERTY_CATEGORY,
                random_property::COMPONENT_ID,
                random_property::DESCRIPTOR_CALLS_OPERATION,
            ) => random_property::descriptor_calls_response(),
            (PROPERTY_CATEGORY, random_property::COMPONENT_ID, PROPERTY_EVALUATE_V1) => {
                random_property::evaluate(payload)
            }
            (STYLE_CATEGORY, style::FILL_COMPONENT_ID, STYLE_EVALUATE_V1) => {
                style::evaluate_fill(payload)
            }
            (STYLE_CATEGORY, style::STROKE_COMPONENT_ID, STYLE_EVALUATE_V1) => {
                style::evaluate_stroke(payload)
            }
            (DECORATOR_CATEGORY, backplate::COMPONENT_ID, DECORATOR_EVALUATE_V1) => {
                backplate::evaluate_v1(payload)
            }
            (DECORATOR_CATEGORY, backplate::COMPONENT_ID, DECORATOR_EVALUATE_V2) => {
                backplate::evaluate_v2(payload)
            }
            _ => RuvieCallResult::error(
                STATUS_INVALID_REQUEST,
                "unsupported component/category/operation",
            ),
        }
    })
}

unsafe extern "C" fn query_extension(
    _context: *mut c_void,
    extension_name: RuvieBytesView,
) -> *const c_void {
    // This callback cannot report an error, so invalid views simply decline.
    // SAFETY: `extension_name` is borrowed only for this callback invocation;
    // `bytes_from_view` validates the pointer/length pair before exposing it.
    let Ok(name) = (unsafe { abi::bytes_from_view(extension_name) }) else {
        return std::ptr::null();
    };
    if name == EFFECT_CPU_RGBA8_EXTENSION_V1.as_bytes() {
        std::ptr::from_ref(&cpu_effect::API).cast()
    } else if name == LOADER_CPU_RGBA8_EXTENSION_V1.as_bytes() {
        std::ptr::from_ref(&rgba_fixture_loader::API).cast()
    } else {
        std::ptr::null()
    }
}

static API: RuviePluginApiV1 = RuviePluginApiV1 {
    abi_version: RUVIE_PLUGIN_ABI_V1,
    struct_size: std::mem::size_of::<RuviePluginApiV1>(),
    context: std::ptr::null_mut(),
    descriptor_json: Some(descriptor_json),
    invoke_json: Some(invoke_json),
    free_buffer: Some(abi::free_buffer),
    query_extension: Some(query_extension),
};

#[unsafe(no_mangle)]
pub extern "C" fn ruvie_plugin_entry_v1() -> *const RuviePluginApiV1 {
    &API
}
