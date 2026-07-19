//! Property evaluation infrastructure.

use std::collections::HashMap;
use std::sync::Arc;

use log::warn;
use ordered_float::OrderedFloat;

use crate::model::property::{Property, PropertyMap, PropertyValue};

/// Registry for property evaluators.
#[derive(Clone, Default)]
pub struct PropertyEvaluatorRegistry {
    evaluators: HashMap<String, Arc<dyn PropertyEvaluator>>,
}

impl PropertyEvaluatorRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, key: &str, evaluator: Arc<dyn PropertyEvaluator>) {
        self.evaluators.insert(key.to_string(), evaluator);
    }

    pub fn evaluate(
        &self,
        property: &Property,
        time: f64,
        ctx: &EvaluationContext,
    ) -> PropertyValue {
        let key = property.evaluator.as_str();
        match self.evaluators.get(key) {
            Some(evaluator) => evaluator.evaluate(property, time, ctx),
            None => {
                warn!("Unknown property evaluator '{}'", key);
                PropertyValue::Number(OrderedFloat(0.0))
            }
        }
    }
}

/// Trait for property evaluators.
pub trait PropertyEvaluator: Send + Sync {
    fn evaluate(&self, property: &Property, time: f64, ctx: &EvaluationContext) -> PropertyValue;
}

/// Context passed to property evaluators.
pub struct EvaluationContext<'a> {
    pub property_map: &'a PropertyMap,
    pub fps: f64,
}
