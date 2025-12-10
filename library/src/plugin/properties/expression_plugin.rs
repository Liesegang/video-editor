use super::super::{Plugin, PluginCategory, PropertyPlugin};
use crate::model::project::property::{Property, PropertyValue}; // Added Property and PropertyValue
use crate::plugin::{EvaluationContext, PropertyEvaluator};
use log::warn;
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

    fn category(&self) -> PluginCategory {
        PluginCategory::Property
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
        PropertyValue::Number(0.0)
    }
}
