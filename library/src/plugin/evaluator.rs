//! Property evaluation infrastructure.

use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use crate::model::property::{Property, PropertyMap, PropertyValue};

/// Fail-closed property evaluation error. Render callers map this to
/// `EvalOutput::NoOutput`; authored Expression errors with a valid typed
/// fallback remain successful values and do not use this path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertyEvaluationError {
    evaluator: String,
    message: String,
}

impl PropertyEvaluationError {
    pub fn new(evaluator: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            evaluator: evaluator.into(),
            message: message.into(),
        }
    }

    pub fn evaluator(&self) -> &str {
        &self.evaluator
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for PropertyEvaluationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "property evaluator '{}' failed: {}",
            self.evaluator, self.message
        )
    }
}

impl std::error::Error for PropertyEvaluationError {}

/// Registry for property evaluators.
#[derive(Clone, Default)]
pub struct PropertyEvaluatorRegistry {
    evaluators: HashMap<String, Arc<dyn PropertyEvaluator>>,
}

impl PropertyEvaluatorRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        key: &str,
        evaluator: Arc<dyn PropertyEvaluator>,
    ) -> Option<Arc<dyn PropertyEvaluator>> {
        self.evaluators.insert(key.to_string(), evaluator)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.evaluators.contains_key(key)
    }

    pub fn evaluate(
        &self,
        property: &Property,
        time: f64,
        ctx: &EvaluationContext,
    ) -> Result<PropertyValue, PropertyEvaluationError> {
        let key = property.evaluator.as_str();
        match self.evaluators.get(key) {
            Some(evaluator) => evaluator.evaluate(property, time, ctx),
            None => Err(PropertyEvaluationError::new(
                key,
                "evaluator is not registered",
            )),
        }
    }
}

/// Trait for property evaluators.
pub trait PropertyEvaluator: Send + Sync {
    fn evaluate(
        &self,
        property: &Property,
        time: f64,
        ctx: &EvaluationContext,
    ) -> Result<PropertyValue, PropertyEvaluationError>;
}

/// Context passed to property evaluators.
pub struct EvaluationContext<'a> {
    pub property_map: &'a PropertyMap,
    pub fps: f64,
    pub resolution: (u64, u64),
}

impl<'a> EvaluationContext<'a> {
    pub const fn new(property_map: &'a PropertyMap, fps: f64, resolution: (u64, u64)) -> Self {
        Self {
            property_map,
            fps,
            resolution,
        }
    }
}
