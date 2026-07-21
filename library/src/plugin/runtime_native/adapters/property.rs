use std::collections::BTreeMap;

use ruvie_plugin_api::{
    PROPERTY_EVALUATE_V1, PropertyEvaluateRequestV1, PropertyEvaluateResponseV1,
};

use super::super::abi::RuntimeComponent;
use super::super::property_wire::{property_value_from_wire, property_value_to_wire};
use crate::model::property::{Property, PropertyDefinition, PropertyValue};
use crate::plugin::evaluator::{EvaluationContext, PropertyEvaluationError, PropertyEvaluator};
pub(in crate::plugin::runtime_native) struct RuntimePropertyEvaluator {
    pub(in crate::plugin::runtime_native) component: RuntimeComponent,
    pub(in crate::plugin::runtime_native) definitions: Vec<PropertyDefinition>,
    pub(in crate::plugin::runtime_native) output_default: PropertyValue,
}

impl RuntimePropertyEvaluator {
    pub(super) fn fallback(&self, detail: impl std::fmt::Display) -> PropertyValue {
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
    ) -> Result<PropertyValue, PropertyEvaluationError> {
        let mut properties = BTreeMap::new();
        for definition in &self.definitions {
            let value = property
                .properties
                .get(definition.name())
                .unwrap_or_else(|| definition.default_value());
            if let Err(error) = definition.validate_value(value) {
                return Ok(self.fallback(format!(
                    "property '{}' is invalid: {error}",
                    definition.name()
                )));
            }
            let value = match property_value_to_wire(value) {
                Ok(value) => value,
                Err(error) => {
                    return Ok(self.fallback(format!(
                        "property '{}' cannot cross ABI v1: {error}",
                        definition.name()
                    )));
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
            Err(error) => {
                return Ok(self.fallback(format!("request encoding failed: {error}")));
            }
        };
        let response = match self.component.invoke(PROPERTY_EVALUATE_V1, payload) {
            Ok(response) => response,
            Err(error) => return Ok(self.fallback(error)),
        };
        let response: PropertyEvaluateResponseV1 = match serde_json::from_value(response) {
            Ok(response) => response,
            Err(error) => return Ok(self.fallback(format!("invalid response: {error}"))),
        };
        let value = match property_value_from_wire(&response.value) {
            Ok(value) => value,
            Err(error) => {
                return Ok(self.fallback(format!("invalid response value: {error}")));
            }
        };
        if std::mem::discriminant(&value) != std::mem::discriminant(&self.output_default) {
            return Ok(self.fallback("response type differs from output_default"));
        }
        Ok(value)
    }
}
