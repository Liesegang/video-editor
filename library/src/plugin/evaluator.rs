//! Property evaluation infrastructure.

use std::collections::HashMap;
use std::sync::Arc;

use log::warn;
use ordered_float::OrderedFloat;

use crate::model::project::property::{Property, PropertyMap, PropertyValue};

/// Registry for property evaluators.
pub struct PropertyEvaluatorRegistry {
    evaluators: HashMap<&'static str, Arc<dyn PropertyEvaluator>>,
}

impl Clone for PropertyEvaluatorRegistry {
    fn clone(&self) -> Self {
        Self {
            evaluators: self.evaluators.clone(),
        }
    }
}

impl PropertyEvaluatorRegistry {
    pub fn new() -> Self {
        Self {
            evaluators: HashMap::new(),
        }
    }

    pub fn register(&mut self, key: &'static str, evaluator: Arc<dyn PropertyEvaluator>) {
        self.evaluators.insert(key, evaluator);
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
