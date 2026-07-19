use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};

use ruvie_plugin_api::{
    ComponentDescriptorV1, EFFECTOR_CATEGORY, EFFECTOR_EVALUATE_V1, EffectorEvaluateRequestV1,
    EffectorOutputV1, EffectorTargetV1, InvokeRequestV1, OpacityModeV1, PluginDescriptorV1,
    PropertyDefinitionV1, PropertyUiV1, RUVIE_PLUGIN_ABI_V1, RuvieBuffer, RuvieBytesView,
    RuvieCallResult, RuviePluginApiV1, STATUS_INVALID_REQUEST, STATUS_PANIC,
};

const COMPONENT_ID: &str = "example.third_party_opacity";
const DESCRIPTOR_CALLS_OPERATION: &str = "example.descriptor_calls.v1";
static DESCRIPTOR_CALLS: AtomicUsize = AtomicUsize::new(0);

fn descriptor() -> PluginDescriptorV1 {
    PluginDescriptorV1 {
        name: "Third-party Opacity Example".to_string(),
        vendor: "RuViE SDK Example".to_string(),
        version: "1.0.0".to_string(),
        components: vec![ComponentDescriptorV1 {
            id: COMPONENT_ID.to_string(),
            name: "Third-party Opacity".to_string(),
            category: EFFECTOR_CATEGORY.to_string(),
            group: "SDK Examples".to_string(),
            version: "1.0.0".to_string(),
            operations: vec![
                EFFECTOR_EVALUATE_V1.to_string(),
                DESCRIPTOR_CALLS_OPERATION.to_string(),
            ],
            properties: vec![
                PropertyDefinitionV1 {
                    name: "opacity".to_string(),
                    label: "Opacity".to_string(),
                    ui: PropertyUiV1::Float {
                        min: 0.0,
                        max: 100.0,
                        step: 1.0,
                        suffix: "%".to_string(),
                        min_hard_limit: true,
                        max_hard_limit: true,
                    },
                    default: serde_json::json!(37.5),
                },
                PropertyDefinitionV1 {
                    name: "mode".to_string(),
                    label: "Mode".to_string(),
                    ui: PropertyUiV1::Dropdown {
                        options: vec!["Set".to_string(), "Add".to_string(), "Multiply".to_string()],
                    },
                    default: serde_json::json!("Multiply"),
                },
                PropertyDefinitionV1 {
                    name: "target".to_string(),
                    label: "Target".to_string(),
                    ui: PropertyUiV1::Dropdown {
                        options: vec!["Block".to_string(), "Line".to_string(), "Char".to_string()],
                    },
                    default: serde_json::json!("Char"),
                },
            ],
        }],
    }
}

fn ffi_guard(action: impl FnOnce() -> RuvieCallResult) -> RuvieCallResult {
    match catch_unwind(AssertUnwindSafe(action)) {
        Ok(result) => result,
        Err(_) => RuvieCallResult::error(STATUS_PANIC, "plugin callback panicked"),
    }
}

unsafe extern "C" fn descriptor_json(_context: *mut c_void) -> RuvieCallResult {
    ffi_guard(|| {
        DESCRIPTOR_CALLS.fetch_add(1, Ordering::Relaxed);
        RuvieCallResult::ok_json(&descriptor())
    })
}

unsafe extern "C" fn invoke_json(
    _context: *mut c_void,
    request: RuvieBytesView,
) -> RuvieCallResult {
    ffi_guard(|| {
        if request.ptr.is_null() || request.len == 0 {
            return RuvieCallResult::error(STATUS_INVALID_REQUEST, "empty request");
        }
        // SAFETY: The host owns this immutable request for the callback's
        // duration and the ABI supplies its exact byte length.
        let bytes = unsafe { std::slice::from_raw_parts(request.ptr, request.len) };
        let request: InvokeRequestV1 = match serde_json::from_slice(bytes) {
            Ok(request) => request,
            Err(error) => {
                return RuvieCallResult::error(STATUS_INVALID_REQUEST, error.to_string());
            }
        };
        if request.component_id != COMPONENT_ID || request.category != EFFECTOR_CATEGORY {
            return RuvieCallResult::error(
                STATUS_INVALID_REQUEST,
                "unsupported component/category/operation",
            );
        }
        if request.operation == DESCRIPTOR_CALLS_OPERATION {
            return RuvieCallResult::ok_json(&serde_json::json!({
                "calls": DESCRIPTOR_CALLS.load(Ordering::Relaxed),
            }));
        }
        if request.operation != EFFECTOR_EVALUATE_V1 {
            return RuvieCallResult::error(
                STATUS_INVALID_REQUEST,
                "unsupported component/category/operation",
            );
        }
        let payload: EffectorEvaluateRequestV1 = match serde_json::from_value(request.payload) {
            Ok(payload) => payload,
            Err(error) => {
                return RuvieCallResult::error(STATUS_INVALID_REQUEST, error.to_string());
            }
        };
        let opacity = payload
            .properties
            .get("opacity")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or(37.5) as f32;
        let mode = match payload
            .properties
            .get("mode")
            .and_then(serde_json::Value::as_str)
        {
            Some("Add") => OpacityModeV1::Add,
            Some("Multiply") => OpacityModeV1::Multiply,
            _ => OpacityModeV1::Set,
        };
        let target = match payload
            .properties
            .get("target")
            .and_then(serde_json::Value::as_str)
        {
            Some("Line") => EffectorTargetV1::Line,
            Some("Char") => EffectorTargetV1::Char,
            _ => EffectorTargetV1::Block,
        };
        RuvieCallResult::ok_json(&EffectorOutputV1::Opacity {
            opacity,
            mode,
            target,
        })
    })
}

unsafe extern "C" fn free_buffer(_context: *mut c_void, buffer: RuvieBuffer) {
    // SAFETY: The host returns each buffer once to this same library.
    unsafe { ruvie_plugin_api::free_owned_buffer(buffer) };
}

static API: RuviePluginApiV1 = RuviePluginApiV1 {
    abi_version: RUVIE_PLUGIN_ABI_V1,
    struct_size: std::mem::size_of::<RuviePluginApiV1>(),
    context: std::ptr::null_mut(),
    descriptor_json: Some(descriptor_json),
    invoke_json: Some(invoke_json),
    free_buffer: Some(free_buffer),
    query_extension: None,
};

#[unsafe(no_mangle)]
pub extern "C" fn ruvie_plugin_entry_v1() -> *const RuviePluginApiV1 {
    &API
}
