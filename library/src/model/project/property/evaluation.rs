use std::fmt;

use super::{Property, PropertyValue};

/// Failure from the model-only constant/keyframe sampler.
///
/// Expression and plugin-authored evaluators require an explicit
/// `EvaluationContext` and therefore must be sampled through the evaluator
/// registry. Keeping this API fallible prevents model/UI helpers from
/// accidentally treating an Expression's authored input as its output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertySampleError {
    evaluator: String,
    message: String,
}

impl PropertySampleError {
    fn new(evaluator: impl Into<String>, message: impl Into<String>) -> Self {
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

impl fmt::Display for PropertySampleError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "property evaluator '{}' cannot be sampled locally: {}",
            self.evaluator, self.message
        )
    }
}

impl std::error::Error for PropertySampleError {}

impl Property {
    /// Samples only context-free built-in properties.
    ///
    /// Expressions and unknown/plugin evaluators deliberately return an error;
    /// callers with fps/resolution must use `PropertyEvaluatorRegistry`.
    pub fn evaluate_at(&self, time: f64) -> Result<PropertyValue, PropertySampleError> {
        match self.evaluator.as_str() {
            "constant" => self.value().cloned().ok_or_else(|| {
                PropertySampleError::new("constant", "property has no authored value")
            }),
            "keyframe" => evaluate_keyframes(self, time),
            evaluator => Err(PropertySampleError::new(
                evaluator,
                "an explicit evaluator context is required",
            )),
        }
    }
}

fn evaluate_keyframes(
    property: &Property,
    time: f64,
) -> Result<PropertyValue, PropertySampleError> {
    if !time.is_finite() {
        return Err(PropertySampleError::new(
            "keyframe",
            "sample time must be finite",
        ));
    }

    let keyframes = property.keyframes();
    if keyframes.is_empty() {
        return property.value().cloned().ok_or_else(|| {
            PropertySampleError::new(
                "keyframe",
                "property has neither keyframes nor an authored value",
            )
        });
    }
    if keyframes
        .iter()
        .any(|keyframe| !keyframe.time.into_inner().is_finite())
    {
        return Err(PropertySampleError::new(
            "keyframe",
            "keyframe time must be finite",
        ));
    }

    if time <= keyframes[0].time.into_inner() {
        return Ok(keyframes[0].value.clone());
    }
    let last = &keyframes[keyframes.len() - 1];
    if time >= last.time.into_inner() {
        return Ok(last.value.clone());
    }

    for window in keyframes.windows(2) {
        let start = &window[0];
        let end = &window[1];
        let start_time = start.time.into_inner();
        let end_time = end.time.into_inner();
        if time >= start_time && time < end_time {
            let duration = end_time - start_time;
            if duration <= f64::EPSILON {
                return Ok(start.value.clone());
            }
            let normalized = (time - start_time) / duration;
            return Ok(PropertyValue::interpolate(
                &start.value,
                &end.value,
                start.easing.apply(normalized),
            ));
        }
    }

    Err(PropertySampleError::new(
        "keyframe",
        "sample time did not resolve to a keyframe segment",
    ))
}

#[cfg(test)]
mod tests {
    use ordered_float::OrderedFloat;

    use super::*;

    #[test]
    fn context_dependent_evaluators_never_return_authored_input_as_output() {
        let expression = Property::expression(
            "1 / 0".to_string(),
            PropertyValue::Number(OrderedFloat(42.0)),
        );
        assert_eq!(
            expression.evaluate_at(0.0).unwrap_err().evaluator(),
            "expression"
        );

        let unknown = Property {
            evaluator: "third-party".to_string(),
            properties: std::collections::HashMap::from([(
                "value".to_string(),
                PropertyValue::Number(OrderedFloat(7.0)),
            )]),
        };
        assert_eq!(
            unknown.evaluate_at(0.0).unwrap_err().evaluator(),
            "third-party"
        );
    }

    #[test]
    fn malformed_builtins_do_not_invent_numeric_zero() {
        let constant = Property {
            evaluator: "constant".to_string(),
            properties: std::collections::HashMap::new(),
        };
        assert!(constant.evaluate_at(0.0).is_err());

        let keyframe = Property {
            evaluator: "keyframe".to_string(),
            properties: std::collections::HashMap::new(),
        };
        assert!(keyframe.evaluate_at(0.0).is_err());
    }
}
