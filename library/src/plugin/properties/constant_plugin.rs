use super::super::{Plugin, PluginCategory, PropertyPlugin};
use crate::model::project::property::{Property, PropertyValue};
use crate::plugin::{EvaluationContext, PropertyEvaluator};
use log::warn;
use ordered_float::OrderedFloat;
use std::sync::Arc;

pub struct ConstantPropertyPlugin;

impl ConstantPropertyPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl Plugin for ConstantPropertyPlugin {
    fn id(&self) -> &'static str {
        "constant"
    }

    fn category(&self) -> String {
        "Property".to_string()
    }

    fn name(&self) -> String {
        "Constant Property".to_string()
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl PropertyPlugin for ConstantPropertyPlugin {
    fn get_evaluator_instance(&self) -> Arc<dyn PropertyEvaluator> {
        Arc::new(ConstantEvaluator)
    }
}

pub struct ConstantEvaluator;

impl PropertyEvaluator for ConstantEvaluator {
    fn evaluate(&self, property: &Property, _time: f64, _ctx: &EvaluationContext) -> PropertyValue {
        property.value().cloned().unwrap_or_else(|| {
            warn!("Constant evaluator missing 'value'; using 0");
            PropertyValue::Number(OrderedFloat(0.0))
        })
    }
}
