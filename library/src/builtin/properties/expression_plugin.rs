use crate::plugin::{EvaluationContext, PropertyEvaluator};
use crate::plugin::{Plugin, PropertyPlugin};
use crate::project::property::{Property, PropertyValue}; // Added Property and PropertyValue
use log::warn;
use ordered_float::OrderedFloat;
use std::sync::Arc;

pub struct ExpressionPropertyPlugin;

impl ExpressionPropertyPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Plugin for ExpressionPropertyPlugin {
    fn id(&self) -> &'static str {
        "expression"
    }

    fn name(&self) -> String {
        "Expression Property".to_string()
    }

    fn category(&self) -> String {
        "Property".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl PropertyPlugin for ExpressionPropertyPlugin {
    fn get_evaluator_instance(&self) -> Arc<dyn PropertyEvaluator> {
        Arc::new(ExpressionEvaluator)
    }
}

pub struct ExpressionEvaluator;

impl PropertyEvaluator for ExpressionEvaluator {
    fn evaluate(&self, property: &Property, time: f64, _ctx: &EvaluationContext) -> PropertyValue {
        warn!(
            "Expression evaluator not implemented for property '{}' at time {}",
            property.evaluator, time
        );
        PropertyValue::Number(OrderedFloat(0.0))
    }
}
