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

/// Successful property value plus a recoverable evaluator diagnostic.
///
/// Expression syntax/runtime errors may use an authored typed fallback while
/// still reporting why the authored source did not produce the value. Invalid
/// evaluator configuration remains an outer [`PropertyEvaluationError`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertyEvaluationOutcome {
    value: PropertyValue,
    diagnostic: Option<PropertyEvaluationDiagnostic>,
}

impl PropertyEvaluationOutcome {
    pub fn clean(value: PropertyValue) -> Self {
        Self {
            value,
            diagnostic: None,
        }
    }

    pub fn recovered(
        value: PropertyValue,
        evaluator: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            value,
            diagnostic: Some(PropertyEvaluationDiagnostic {
                evaluator: evaluator.into(),
                message: message.into(),
            }),
        }
    }

    pub fn value(&self) -> &PropertyValue {
        &self.value
    }

    pub fn into_value(self) -> PropertyValue {
        self.value
    }

    pub fn diagnostic(&self) -> Option<&PropertyEvaluationDiagnostic> {
        self.diagnostic.as_ref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertyEvaluationDiagnostic {
    evaluator: String,
    message: String,
}

impl PropertyEvaluationDiagnostic {
    pub fn evaluator(&self) -> &str {
        &self.evaluator
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

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
        self.evaluate_with_diagnostics(property, time, ctx)
            .map(PropertyEvaluationOutcome::into_value)
    }

    pub fn evaluate_with_diagnostics(
        &self,
        property: &Property,
        time: f64,
        ctx: &EvaluationContext,
    ) -> Result<PropertyEvaluationOutcome, PropertyEvaluationError> {
        let key = property.evaluator.as_str();
        match self.evaluators.get(key) {
            Some(evaluator) => evaluator.evaluate_with_diagnostics(property, time, ctx),
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

    fn evaluate_with_diagnostics(
        &self,
        property: &Property,
        time: f64,
        ctx: &EvaluationContext,
    ) -> Result<PropertyEvaluationOutcome, PropertyEvaluationError> {
        self.evaluate(property, time, ctx)
            .map(PropertyEvaluationOutcome::clean)
    }
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
