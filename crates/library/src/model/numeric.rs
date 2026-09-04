//! Shared numeric kernel for native graph arithmetic Nodes.
//!
//! Graph ports use a numeric union while runtime values retain their concrete
//! scalar or vector dimension. Binary operations broadcast a scalar, require
//! equal vector dimensions, and reject partial/invalid results atomically.

use ordered_float::OrderedFloat;

use crate::model::property::{PropertyValue, Vec2, Vec3, Vec4};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NumericBinaryOperation {
    Add,
    Subtract,
    Multiply,
    Divide,
    Fmod,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NumericEvaluationError {
    NonNumeric,
    NonFiniteInput,
    DimensionMismatch { left: usize, right: usize },
    ZeroDivisor,
    NonFiniteResult,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum NumericValue {
    Scalar(f64),
    Vec2([f64; 2]),
    Vec3([f64; 3]),
    Vec4([f64; 4]),
}

impl NumericValue {
    fn from_property(value: &PropertyValue) -> Result<Self, NumericEvaluationError> {
        let numeric = match value {
            PropertyValue::Integer(value) => Self::Scalar(*value as f64),
            PropertyValue::Number(value) => Self::Scalar(value.into_inner()),
            PropertyValue::Vec2(value) => Self::Vec2([value.x.into_inner(), value.y.into_inner()]),
            PropertyValue::Vec3(value) => Self::Vec3([
                value.x.into_inner(),
                value.y.into_inner(),
                value.z.into_inner(),
            ]),
            PropertyValue::Vec4(value) => Self::Vec4([
                value.x.into_inner(),
                value.y.into_inner(),
                value.z.into_inner(),
                value.w.into_inner(),
            ]),
            _ => return Err(NumericEvaluationError::NonNumeric),
        };
        if !numeric.all_finite() {
            return Err(NumericEvaluationError::NonFiniteInput);
        }
        Ok(numeric)
    }

    fn dimension(self) -> usize {
        match self {
            Self::Scalar(_) => 1,
            Self::Vec2(_) => 2,
            Self::Vec3(_) => 3,
            Self::Vec4(_) => 4,
        }
    }

    fn all_finite(self) -> bool {
        match self {
            Self::Scalar(value) => value.is_finite(),
            Self::Vec2(values) => values.into_iter().all(f64::is_finite),
            Self::Vec3(values) => values.into_iter().all(f64::is_finite),
            Self::Vec4(values) => values.into_iter().all(f64::is_finite),
        }
    }

    fn component(self, index: usize) -> f64 {
        match self {
            Self::Scalar(value) => value,
            Self::Vec2(values) => values[index],
            Self::Vec3(values) => values[index],
            Self::Vec4(values) => values[index],
        }
    }

    fn from_components(dimension: usize, values: [f64; 4]) -> Result<Self, NumericEvaluationError> {
        match dimension {
            1 => Ok(Self::Scalar(values[0])),
            2 => Ok(Self::Vec2([values[0], values[1]])),
            3 => Ok(Self::Vec3([values[0], values[1], values[2]])),
            4 => Ok(Self::Vec4(values)),
            left => Err(NumericEvaluationError::DimensionMismatch { left, right: 0 }),
        }
    }

    fn into_property(self) -> PropertyValue {
        match self {
            Self::Scalar(value) => PropertyValue::Number(OrderedFloat(value)),
            Self::Vec2([x, y]) => PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(x),
                y: OrderedFloat(y),
            }),
            Self::Vec3([x, y, z]) => PropertyValue::Vec3(Vec3 {
                x: OrderedFloat(x),
                y: OrderedFloat(y),
                z: OrderedFloat(z),
            }),
            Self::Vec4([x, y, z, w]) => PropertyValue::Vec4(Vec4 {
                x: OrderedFloat(x),
                y: OrderedFloat(y),
                z: OrderedFloat(z),
                w: OrderedFloat(w),
            }),
        }
    }
}

pub(crate) fn evaluate_numeric_binary(
    operation: NumericBinaryOperation,
    left: &PropertyValue,
    right: &PropertyValue,
) -> Result<PropertyValue, NumericEvaluationError> {
    let left = NumericValue::from_property(left)?;
    let right = NumericValue::from_property(right)?;
    let left_dimension = left.dimension();
    let right_dimension = right.dimension();
    let dimension = match (left_dimension, right_dimension) {
        (left, right) if left == right => left,
        (1, right) => right,
        (left, 1) => left,
        (left, right) => {
            return Err(NumericEvaluationError::DimensionMismatch { left, right });
        }
    };

    let mut values = [0.0; 4];
    for (index, value) in values.iter_mut().take(dimension).enumerate() {
        let left = left.component(index);
        let right = right.component(index);
        if matches!(
            operation,
            NumericBinaryOperation::Divide | NumericBinaryOperation::Fmod
        ) && right == 0.0
        {
            return Err(NumericEvaluationError::ZeroDivisor);
        }
        *value = match operation {
            NumericBinaryOperation::Add => left + right,
            NumericBinaryOperation::Subtract => left - right,
            NumericBinaryOperation::Multiply => left * right,
            NumericBinaryOperation::Divide => left / right,
            NumericBinaryOperation::Fmod => left % right,
        };
        if !value.is_finite() {
            return Err(NumericEvaluationError::NonFiniteResult);
        }
    }
    Ok(NumericValue::from_components(dimension, values)?.into_property())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vec2(x: f64, y: f64) -> PropertyValue {
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(x),
            y: OrderedFloat(y),
        })
    }

    fn vec3(x: f64, y: f64, z: f64) -> PropertyValue {
        PropertyValue::Vec3(Vec3 {
            x: OrderedFloat(x),
            y: OrderedFloat(y),
            z: OrderedFloat(z),
        })
    }

    #[test]
    fn scalar_broadcast_and_same_dimension_vectors_are_component_wise() {
        assert_eq!(
            evaluate_numeric_binary(
                NumericBinaryOperation::Fmod,
                &vec2(5.5, -5.5),
                &PropertyValue::Number(OrderedFloat(2.0)),
            ),
            Ok(vec2(1.5, -1.5))
        );
        assert_eq!(
            evaluate_numeric_binary(
                NumericBinaryOperation::Fmod,
                &PropertyValue::Integer(8),
                &vec3(3.0, 5.0, 6.0),
            ),
            Ok(vec3(2.0, 3.0, 2.0))
        );
    }

    #[test]
    fn mismatched_vectors_and_any_invalid_component_reject_the_whole_value() {
        assert_eq!(
            evaluate_numeric_binary(
                NumericBinaryOperation::Fmod,
                &vec2(1.0, 2.0),
                &vec3(1.0, 2.0, 3.0),
            ),
            Err(NumericEvaluationError::DimensionMismatch { left: 2, right: 3 })
        );
        assert_eq!(
            evaluate_numeric_binary(
                NumericBinaryOperation::Fmod,
                &vec2(3.0, 4.0),
                &vec2(2.0, 0.0),
            ),
            Err(NumericEvaluationError::ZeroDivisor)
        );
        assert_eq!(
            evaluate_numeric_binary(
                NumericBinaryOperation::Fmod,
                &vec2(3.0, f64::NAN),
                &vec2(2.0, 1.0),
            ),
            Err(NumericEvaluationError::NonFiniteInput)
        );
    }

    #[test]
    fn basic_arithmetic_uses_the_same_broadcast_and_atomic_failure_rules() {
        for (operation, expected) in [
            (NumericBinaryOperation::Add, vec2(8.0, 10.0)),
            (NumericBinaryOperation::Subtract, vec2(2.0, 2.0)),
            (NumericBinaryOperation::Multiply, vec2(15.0, 24.0)),
            (NumericBinaryOperation::Divide, vec2(5.0 / 3.0, 1.5)),
        ] {
            assert_eq!(
                evaluate_numeric_binary(operation, &vec2(5.0, 6.0), &vec2(3.0, 4.0)),
                Ok(expected)
            );
        }
        assert_eq!(
            evaluate_numeric_binary(
                NumericBinaryOperation::Divide,
                &vec2(5.0, 6.0),
                &vec2(1.0, -0.0),
            ),
            Err(NumericEvaluationError::ZeroDivisor)
        );
        assert_eq!(
            evaluate_numeric_binary(
                NumericBinaryOperation::Multiply,
                &PropertyValue::Number(OrderedFloat(f64::MAX)),
                &PropertyValue::Number(OrderedFloat(2.0)),
            ),
            Err(NumericEvaluationError::NonFiniteResult)
        );
    }
}
