//! Shared numeric kernel for native graph arithmetic Nodes.
//!
//! Graph ports use a numeric union while runtime values retain their concrete
//! scalar or vector dimension. Binary operations broadcast a scalar, require
//! equal vector dimensions, and reject partial/invalid results atomically.

use ordered_float::OrderedFloat;

use crate::model::property::{PropertyValue, Vec2, Vec3, Vec4};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum NumericBinaryOperation {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum NumericShape {
    Scalar,
    Vec2,
    Vec3,
    Vec4,
}

impl NumericShape {
    pub(crate) const fn dimension(self) -> usize {
        match self {
            Self::Scalar => 1,
            Self::Vec2 => 2,
            Self::Vec3 => 3,
            Self::Vec4 => 4,
        }
    }

    /// The authoritative scalar-broadcast rule shared by frame-wide and
    /// per-Point numeric evaluation.
    pub(crate) fn broadcast_result(self, other: Self) -> Result<Self, NumericEvaluationError> {
        match (self, other) {
            (left, right) if left == right => Ok(left),
            (Self::Scalar, right) => Ok(right),
            (left, Self::Scalar) => Ok(left),
            (left, right) => Err(NumericEvaluationError::DimensionMismatch {
                left: left.dimension(),
                right: right.dimension(),
            }),
        }
    }
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

    fn shape(self) -> NumericShape {
        match self {
            Self::Scalar(_) => NumericShape::Scalar,
            Self::Vec2(_) => NumericShape::Vec2,
            Self::Vec3(_) => NumericShape::Vec3,
            Self::Vec4(_) => NumericShape::Vec4,
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

    fn stable_length(self) -> Result<f64, NumericEvaluationError> {
        if let Self::Scalar(value) = self {
            return Ok(value.abs());
        }
        let dimension = self.shape().dimension();
        let scale = (0..dimension)
            .map(|index| self.component(index).abs())
            .fold(0.0, f64::max);
        if scale == 0.0 {
            return Ok(0.0);
        }
        let sum = (0..dimension)
            .map(|index| {
                let normalized = self.component(index) / scale;
                normalized * normalized
            })
            .sum::<f64>();
        let result = scale * sum.sqrt();
        result
            .is_finite()
            .then_some(result)
            .ok_or(NumericEvaluationError::NonFiniteResult)
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
    let dimension = left.shape().broadcast_result(right.shape())?.dimension();

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

/// Evaluate the magnitude of a scalar or 2D/3D/4D vector without squaring
/// large components before scaling. Scalar length is absolute value.
pub(crate) fn evaluate_numeric_length(
    value: &PropertyValue,
) -> Result<PropertyValue, NumericEvaluationError> {
    let value = NumericValue::from_property(value)?;
    Ok(PropertyValue::Number(OrderedFloat(value.stable_length()?)))
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

    #[test]
    fn shape_algebra_matches_runtime_broadcasting() {
        assert_eq!(
            NumericShape::Vec3.broadcast_result(NumericShape::Scalar),
            Ok(NumericShape::Vec3)
        );
        assert_eq!(
            NumericShape::Scalar.broadcast_result(NumericShape::Vec4),
            Ok(NumericShape::Vec4)
        );
        assert_eq!(
            NumericShape::Vec2.broadcast_result(NumericShape::Vec3),
            Err(NumericEvaluationError::DimensionMismatch { left: 2, right: 3 })
        );
    }

    #[test]
    fn length_is_scalar_absolute_and_stably_scaled_vector_magnitude() {
        assert_eq!(
            evaluate_numeric_length(&PropertyValue::Number(OrderedFloat(-4.0))),
            Ok(PropertyValue::Number(OrderedFloat(4.0)))
        );
        assert_eq!(
            evaluate_numeric_length(&vec3(3.0, 4.0, 12.0)),
            Ok(PropertyValue::Number(OrderedFloat(13.0)))
        );
        let large = f64::MAX / 2.0;
        assert_eq!(
            evaluate_numeric_length(&vec2(large, 0.0)),
            Ok(PropertyValue::Number(OrderedFloat(large)))
        );
        assert_eq!(
            evaluate_numeric_length(&vec2(f64::MAX, f64::MAX)),
            Err(NumericEvaluationError::NonFiniteResult)
        );
        assert_eq!(
            evaluate_numeric_length(&PropertyValue::Boolean(true)),
            Err(NumericEvaluationError::NonNumeric)
        );
    }
}
