//! Shared comparison and conditional-selection semantics for graph values.

use serde::{Deserialize, Serialize};

use super::project::PortDataType;
use super::property::{ColorValue, PropertyValue};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComparisonOperation {
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Equal,
    NotEqual,
}

impl ComparisonOperation {
    pub const ALL: [Self; 6] = [
        Self::Less,
        Self::LessEqual,
        Self::Greater,
        Self::GreaterEqual,
        Self::Equal,
        Self::NotEqual,
    ];
}

/// Compare finite scalar numeric values. Integer values use the same f64
/// promotion as the existing shared numeric kernel.
pub(crate) fn evaluate_comparison(
    operation: ComparisonOperation,
    left: &PropertyValue,
    right: &PropertyValue,
) -> Result<PropertyValue, String> {
    let left = finite_number(left, "left")?;
    let right = finite_number(right, "right")?;
    let result = match operation {
        ComparisonOperation::Less => left < right,
        ComparisonOperation::LessEqual => left <= right,
        ComparisonOperation::Greater => left > right,
        ComparisonOperation::GreaterEqual => left >= right,
        ComparisonOperation::Equal => left == right,
        ComparisonOperation::NotEqual => left != right,
    };
    Ok(PropertyValue::Boolean(result))
}

/// Eagerly validate both branches and select one concrete typed value. Number
/// uses the graph's existing Integer-to-Number promotion; other branch types
/// are exact. Eager validation matches Point-program atomic failure: an
/// invalid unselected branch is not silently hidden by the condition.
pub(crate) fn evaluate_selection(
    data_type: PortDataType,
    condition: &PropertyValue,
    when_true: &PropertyValue,
    when_false: &PropertyValue,
) -> Result<PropertyValue, String> {
    let PropertyValue::Boolean(condition) = condition else {
        return Err("Select condition requires Boolean".to_string());
    };
    let when_true = canonical_branch(data_type, when_true, "true")?;
    let when_false = canonical_branch(data_type, when_false, "false")?;
    Ok(if *condition { when_true } else { when_false })
}

fn finite_number(value: &PropertyValue, label: &str) -> Result<f64, String> {
    let value = match value {
        PropertyValue::Number(value) => value.into_inner(),
        PropertyValue::Integer(value) => *value as f64,
        _ => return Err(format!("Comparison {label} input requires Number")),
    };
    value
        .is_finite()
        .then_some(value)
        .ok_or_else(|| format!("Comparison {label} input must be finite"))
}

fn canonical_branch(
    data_type: PortDataType,
    value: &PropertyValue,
    label: &str,
) -> Result<PropertyValue, String> {
    let canonical = match (data_type, value) {
        (PortDataType::Number, PropertyValue::Number(value)) => {
            finite(value.into_inner(), label)?;
            PropertyValue::Number(*value)
        }
        // Number inputs already accept Integer sources. Selection preserves
        // that uniform graph contract while producing the canonical Number
        // payload required by downstream evaluators.
        (PortDataType::Number, PropertyValue::Integer(value)) => {
            PropertyValue::Number((*value as f64).into())
        }
        (PortDataType::Integer, PropertyValue::Integer(_))
        | (PortDataType::Color, PropertyValue::ColorValue(_))
        | (PortDataType::Boolean, PropertyValue::Boolean(_)) => value.clone(),
        (PortDataType::Vec2, PropertyValue::Vec2(value)) => {
            validate_components(label, &[value.x.into_inner(), value.y.into_inner()])?;
            PropertyValue::Vec2(*value)
        }
        (PortDataType::Vec3, PropertyValue::Vec3(value)) => {
            validate_components(
                label,
                &[
                    value.x.into_inner(),
                    value.y.into_inner(),
                    value.z.into_inner(),
                ],
            )?;
            PropertyValue::Vec3(*value)
        }
        (PortDataType::Vec4, PropertyValue::Vec4(value)) => {
            validate_components(
                label,
                &[
                    value.x.into_inner(),
                    value.y.into_inner(),
                    value.z.into_inner(),
                    value.w.into_inner(),
                ],
            )?;
            PropertyValue::Vec4(*value)
        }
        (PortDataType::Color, PropertyValue::Color(value)) => {
            PropertyValue::ColorValue(ColorValue::from_straight_srgba8(value))
        }
        _ => {
            return Err(format!(
                "Select {label} branch requires exact {data_type:?} value"
            ));
        }
    };
    Ok(canonical)
}

fn validate_components(label: &str, components: &[f64]) -> Result<(), String> {
    components
        .iter()
        .try_for_each(|component| finite(*component, label))
}

fn finite(value: f64, label: &str) -> Result<(), String> {
    value
        .is_finite()
        .then_some(())
        .ok_or_else(|| format!("Select {label} branch components must be finite"))
}

#[cfg(test)]
mod tests {
    use ordered_float::OrderedFloat;

    use super::*;
    use crate::model::frame::color::Color;
    use crate::model::property::Vec2;

    #[test]
    fn comparison_promotes_integer_and_uses_exact_finite_relations() {
        assert_eq!(
            evaluate_comparison(
                ComparisonOperation::GreaterEqual,
                &PropertyValue::Integer(3),
                &PropertyValue::Number(OrderedFloat(3.0)),
            ),
            Ok(PropertyValue::Boolean(true))
        );
        assert_eq!(
            evaluate_comparison(
                ComparisonOperation::Equal,
                &PropertyValue::Number(OrderedFloat(-0.0)),
                &PropertyValue::Number(OrderedFloat(0.0)),
            ),
            Ok(PropertyValue::Boolean(true))
        );
        assert!(
            evaluate_comparison(
                ComparisonOperation::Less,
                &PropertyValue::Number(OrderedFloat(f64::NAN)),
                &PropertyValue::Number(OrderedFloat(0.0)),
            )
            .is_err()
        );
    }

    #[test]
    fn selection_is_exact_eager_and_canonicalizes_legacy_color() {
        let left = PropertyValue::Vec2(Vec2 {
            x: 1.0.into(),
            y: 2.0.into(),
        });
        assert_eq!(
            evaluate_selection(
                PortDataType::Vec2,
                &PropertyValue::Boolean(true),
                &left,
                &PropertyValue::Vec2(Vec2 {
                    x: 3.0.into(),
                    y: 4.0.into(),
                }),
            ),
            Ok(left)
        );
        assert_eq!(
            evaluate_selection(
                PortDataType::Number,
                &PropertyValue::Boolean(false),
                &PropertyValue::Number(1.0.into()),
                &PropertyValue::Integer(7),
            ),
            Ok(PropertyValue::Number(7.0.into()))
        );
        assert!(
            evaluate_selection(
                PortDataType::Number,
                &PropertyValue::Boolean(false),
                &PropertyValue::String("invalid but unselected".into()),
                &PropertyValue::Number(0.0.into()),
            )
            .is_err()
        );
        let encoded = Color::white();
        assert_eq!(
            evaluate_selection(
                PortDataType::Color,
                &PropertyValue::Boolean(true),
                &PropertyValue::Color(encoded.clone()),
                &PropertyValue::ColorValue(ColorValue::from_straight_srgba8(&encoded)),
            ),
            Ok(PropertyValue::ColorValue(ColorValue::from_straight_srgba8(
                &encoded
            )))
        );
    }
}
