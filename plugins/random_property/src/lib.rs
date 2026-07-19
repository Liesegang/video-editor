use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicUsize, Ordering};

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use ruvie_plugin_api::{
    ComponentDescriptorV1, InvokeRequestV1, PluginDescriptorV1, PropertyDefinitionV1,
    PropertyEvaluateRequestV1, PropertyEvaluateResponseV1, PropertyUiV1, PropertyValueV1,
    RuvieBuffer, RuvieBytesView, RuvieCallResult, RuviePluginApiV1, PROPERTY_CATEGORY,
    PROPERTY_EVALUATE_V1, RUVIE_PLUGIN_ABI_V1, STATUS_INVALID_REQUEST, STATUS_PANIC,
};

const COMPONENT_ID: &str = "random_property";
const DESCRIPTOR_CALLS_OPERATION: &str = "random_property.descriptor_calls.v1";
static DESCRIPTOR_CALLS: AtomicUsize = AtomicUsize::new(0);

fn descriptor() -> PluginDescriptorV1 {
    PluginDescriptorV1 {
        name: "Random Property".to_string(),
        vendor: "RuViE".to_string(),
        version: "0.1.0".to_string(),
        components: vec![ComponentDescriptorV1 {
            id: COMPONENT_ID.to_string(),
            name: "Random Property".to_string(),
            category: PROPERTY_CATEGORY.to_string(),
            group: "Property".to_string(),
            version: "0.1.0".to_string(),
            operations: vec![
                PROPERTY_EVALUATE_V1.to_string(),
                DESCRIPTOR_CALLS_OPERATION.to_string(),
            ],
            properties: vec![
                PropertyDefinitionV1 {
                    name: "amplitude".to_string(),
                    label: "Amplitude".to_string(),
                    ui: PropertyUiV1::Float {
                        min: 0.0,
                        max: 1_000.0,
                        step: 0.01,
                        suffix: String::new(),
                        min_hard_limit: false,
                        max_hard_limit: false,
                    },
                    default: serde_json::json!(1.0),
                },
                PropertyDefinitionV1 {
                    name: "seed".to_string(),
                    label: "Seed".to_string(),
                    ui: PropertyUiV1::Integer {
                        min: 0,
                        max: i64::MAX,
                        suffix: String::new(),
                        min_hard_limit: true,
                        max_hard_limit: true,
                    },
                    default: serde_json::json!(0),
                },
            ],
            output_default: Some(PropertyValueV1::Number { value: 0.0 }),
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
        // SAFETY: The ABI guarantees that the host-owned request remains valid
        // and immutable for exactly this callback invocation.
        let bytes = unsafe { std::slice::from_raw_parts(request.ptr, request.len) };
        let request: InvokeRequestV1 = match serde_json::from_slice(bytes) {
            Ok(request) => request,
            Err(error) => {
                return RuvieCallResult::error(STATUS_INVALID_REQUEST, error.to_string());
            }
        };
        if request.component_id != COMPONENT_ID || request.category != PROPERTY_CATEGORY {
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
        if request.operation != PROPERTY_EVALUATE_V1 {
            return RuvieCallResult::error(
                STATUS_INVALID_REQUEST,
                "unsupported component/category/operation",
            );
        }
        let payload: PropertyEvaluateRequestV1 = match serde_json::from_value(request.payload) {
            Ok(payload) => payload,
            Err(error) => {
                return RuvieCallResult::error(STATUS_INVALID_REQUEST, error.to_string());
            }
        };
        let amplitude = match payload.properties.get("amplitude") {
            Some(PropertyValueV1::Number { value }) if value.is_finite() => value.abs(),
            _ => 1.0,
        };
        let seed = match payload.properties.get("seed") {
            Some(PropertyValueV1::Integer { value }) => u64::try_from(*value).unwrap_or_default(),
            _ => 0,
        };
        let time_bucket = (payload.time * 1000.0).round() as u64;
        let mut rng = StdRng::seed_from_u64(seed ^ time_bucket);
        let value = rng.gen_range(-amplitude..=amplitude);
        RuvieCallResult::ok_json(&PropertyEvaluateResponseV1 {
            value: PropertyValueV1::Number { value },
        })
    })
}

unsafe extern "C" fn free_buffer(_context: *mut c_void, buffer: RuvieBuffer) {
    // SAFETY: The host returns every plugin-owned buffer exactly once to the
    // same dynamic library that allocated it.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evaluator_is_deterministic_and_respects_amplitude() {
        let payload = PropertyEvaluateRequestV1 {
            time: 1.25,
            fps: 30.0,
            properties: std::collections::BTreeMap::from([
                (
                    "amplitude".to_string(),
                    PropertyValueV1::Number { value: 3.0 },
                ),
                ("seed".to_string(), PropertyValueV1::Integer { value: 42 }),
            ]),
        };
        let evaluate = || {
            let amplitude = match payload.properties.get("amplitude") {
                Some(PropertyValueV1::Number { value }) => value.abs(),
                _ => 1.0,
            };
            let seed = match payload.properties.get("seed") {
                Some(PropertyValueV1::Integer { value }) => {
                    u64::try_from(*value).unwrap_or_default()
                }
                _ => 0,
            };
            let bucket = (payload.time * 1000.0).round() as u64;
            StdRng::seed_from_u64(seed ^ bucket).gen_range(-amplitude..=amplitude)
        };
        let first = evaluate();
        assert_eq!(first, evaluate());
        assert!((-3.0..=3.0).contains(&first));
    }
}
