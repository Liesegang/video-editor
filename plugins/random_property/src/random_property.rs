//! Random scalar property descriptor and evaluator.

use std::sync::atomic::{AtomicUsize, Ordering};

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use ruvie_plugin_api::{
    ComponentDescriptorV1, PropertyDefinitionV1, PropertyEvaluateRequestV1,
    PropertyEvaluateResponseV1, PropertyUiV1, PropertyValueV1, RuvieCallResult, PROPERTY_CATEGORY,
    PROPERTY_EVALUATE_V1,
};

use crate::component_request::{has_exact_properties, invalid_request};

pub(super) const COMPONENT_ID: &str = "random_property";
pub(super) const DESCRIPTOR_CALLS_OPERATION: &str = "random_property.descriptor_calls.v1";

static DESCRIPTOR_CALLS: AtomicUsize = AtomicUsize::new(0);

pub(super) fn descriptor() -> ComponentDescriptorV1 {
    ComponentDescriptorV1 {
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
    }
}

pub(super) fn record_descriptor_call() {
    DESCRIPTOR_CALLS.fetch_add(1, Ordering::Relaxed);
}

pub(super) fn descriptor_calls_response() -> RuvieCallResult {
    RuvieCallResult::ok_json(&serde_json::json!({
        "calls": DESCRIPTOR_CALLS.load(Ordering::Relaxed),
    }))
}

pub(super) fn evaluate(payload: serde_json::Value) -> RuvieCallResult {
    let payload: PropertyEvaluateRequestV1 = match serde_json::from_value(payload) {
        Ok(payload) => payload,
        Err(error) => return invalid_request(error),
    };
    if !has_exact_properties(&payload.properties, &["amplitude", "seed"]) {
        return invalid_request("property request does not match its descriptor");
    }
    let amplitude = match payload.properties.get("amplitude") {
        Some(PropertyValueV1::Number { value }) if value.is_finite() => value.abs(),
        _ => return invalid_request("amplitude must be a finite number"),
    };
    let seed = match payload.properties.get("seed") {
        Some(PropertyValueV1::Integer { value }) => u64::try_from(*value).unwrap_or_default(),
        _ => return invalid_request("seed must be an integer"),
    };
    let time_bucket = (payload.time * 1000.0).round() as u64;
    let mut rng = StdRng::seed_from_u64(seed ^ time_bucket);
    let value = rng.gen_range(-amplitude..=amplitude);
    RuvieCallResult::ok_json(&PropertyEvaluateResponseV1 {
        value: PropertyValueV1::Number { value },
    })
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
