use std::sync::{Arc, OnceLock};

use ordered_float::OrderedFloat;

use super::super::{Plugin, PropertyPlugin};
use crate::expression::{
    ExpressionDiagnostic, ExpressionDiagnosticKind, ExpressionEngine, ExpressionEvaluationContext,
    ExpressionValue,
};
use crate::model::frame::color::Color;
use crate::model::property::{Property, PropertyValue, Vec2, Vec3, Vec4};
use crate::plugin::{
    EvaluationContext, PropertyEvaluationError, PropertyEvaluationOutcome, PropertyEvaluator,
};

static EXPRESSION_ENGINE: OnceLock<ExpressionEngine> = OnceLock::new();

/// Shared runtime-only compiler/cache used by render evaluation and Inspector
/// validation. It is intentionally absent from the persisted Project model.
pub(crate) fn expression_engine() -> &'static ExpressionEngine {
    EXPRESSION_ENGINE.get_or_init(ExpressionEngine::default)
}

#[derive(Default)]
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

    fn name(&self) -> String {
        "Expression Property".to_string()
    }

    fn category(&self) -> String {
        "Property".to_string()
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

impl ExpressionEvaluator {
    /// Detailed API for Inspector validation and nodes. Property evaluation
    /// wraps this result with the property's authored, type-defining fallback.
    pub fn evaluate_detailed(
        &self,
        property: &Property,
        time: f64,
        context: &EvaluationContext<'_>,
    ) -> Result<PropertyValue, ExpressionDiagnostic> {
        if property.evaluator != "expression" {
            return Err(evaluation_error(
                ExpressionDiagnosticKind::InvalidContext,
                format!(
                    "ExpressionEvaluator cannot evaluate property type '{}'",
                    property.evaluator
                ),
            ));
        }
        let source = property.expression_text().ok_or_else(|| {
            evaluation_error(
                ExpressionDiagnosticKind::InvalidContext,
                "Expression property has no string source",
            )
        })?;
        let fallback = property.value().ok_or_else(|| {
            evaluation_error(
                ExpressionDiagnosticKind::InvalidContext,
                "Expression property has no typed fallback",
            )
        })?;
        let expression_fallback = expression_value_from_property(fallback)?;
        let output_type = expression_fallback.output_type();
        let evaluation_context =
            ExpressionEvaluationContext::new(time, context.fps, context.resolution)?
                .with_value(expression_fallback);
        let value = expression_engine().evaluate(source, &evaluation_context, output_type)?;
        Ok(property_value_from_expression(value))
    }
}

impl PropertyEvaluator for ExpressionEvaluator {
    fn evaluate(
        &self,
        property: &Property,
        time: f64,
        context: &EvaluationContext,
    ) -> Result<PropertyValue, PropertyEvaluationError> {
        self.evaluate_with_diagnostics(property, time, context)
            .map(PropertyEvaluationOutcome::into_value)
    }

    fn evaluate_with_diagnostics(
        &self,
        property: &Property,
        time: f64,
        context: &EvaluationContext,
    ) -> Result<PropertyEvaluationOutcome, PropertyEvaluationError> {
        if property.evaluator != "expression" {
            return Err(PropertyEvaluationError::new(
                "expression",
                format!(
                    "ExpressionEvaluator cannot evaluate property type '{}'",
                    property.evaluator
                ),
            ));
        }
        let source = property.expression_text().ok_or_else(|| {
            PropertyEvaluationError::new("expression", "property has no string source")
        })?;
        let fallback = property.value().ok_or_else(|| {
            PropertyEvaluationError::new("expression", "property has no typed input value")
        })?;
        let expression_fallback =
            expression_value_from_property(fallback).map_err(|diagnostic| {
                PropertyEvaluationError::new("expression", diagnostic.to_string())
            })?;
        let output_type = expression_fallback.output_type();
        let evaluation_context =
            ExpressionEvaluationContext::new(time, context.fps, context.resolution)
                .map_err(|diagnostic| {
                    PropertyEvaluationError::new("expression", diagnostic.to_string())
                })?
                .with_value(expression_fallback);

        match expression_engine().evaluate(source, &evaluation_context, output_type) {
            Ok(value) => Ok(PropertyEvaluationOutcome::clean(
                property_value_from_expression(value),
            )),
            Err(diagnostic) if diagnostic.kind == ExpressionDiagnosticKind::InvalidContext => Err(
                PropertyEvaluationError::new("expression", diagnostic.to_string()),
            ),
            Err(diagnostic) => Ok(PropertyEvaluationOutcome::recovered(
                fallback.clone(),
                "expression",
                diagnostic.to_string(),
            )),
        }
    }
}

fn expression_value_from_property(
    value: &PropertyValue,
) -> Result<ExpressionValue, ExpressionDiagnostic> {
    match value {
        PropertyValue::Number(value) => Ok(ExpressionValue::Number(value.into_inner())),
        PropertyValue::Integer(value) => Ok(ExpressionValue::Integer(*value)),
        PropertyValue::Vec2(value) => Ok(ExpressionValue::Vec2([
            value.x.into_inner(),
            value.y.into_inner(),
        ])),
        PropertyValue::Vec3(value) => Ok(ExpressionValue::Vec3([
            value.x.into_inner(),
            value.y.into_inner(),
            value.z.into_inner(),
        ])),
        PropertyValue::Vec4(value) => Ok(ExpressionValue::Vec4([
            value.x.into_inner(),
            value.y.into_inner(),
            value.z.into_inner(),
            value.w.into_inner(),
        ])),
        PropertyValue::Color(value) => Ok(ExpressionValue::Color([
            f64::from(value.r) / 255.0,
            f64::from(value.g) / 255.0,
            f64::from(value.b) / 255.0,
            f64::from(value.a) / 255.0,
        ])),
        PropertyValue::ColorValue(_) => Err(evaluation_error(
            ExpressionDiagnosticKind::TypeMismatch,
            "Tagged graph colors are not supported by the legacy untagged Expression color bridge",
        )),
        PropertyValue::Boolean(value) => Ok(ExpressionValue::Bool(*value)),
        PropertyValue::String(value) => Ok(ExpressionValue::String(value.clone())),
        PropertyValue::Path(_) | PropertyValue::Array(_) | PropertyValue::Map(_) => {
            Err(evaluation_error(
                ExpressionDiagnosticKind::TypeMismatch,
                "Expression fallback must be Number, Integer, Vec2, Vec3, Vec4, legacy Color, Bool, or String",
            ))
        }
    }
}

fn property_value_from_expression(value: ExpressionValue) -> PropertyValue {
    match value {
        ExpressionValue::Number(value) => PropertyValue::Number(OrderedFloat(value)),
        ExpressionValue::Integer(value) => PropertyValue::Integer(value),
        ExpressionValue::Vec2([x, y]) => PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(x),
            y: OrderedFloat(y),
        }),
        ExpressionValue::Vec3([x, y, z]) => PropertyValue::Vec3(Vec3 {
            x: OrderedFloat(x),
            y: OrderedFloat(y),
            z: OrderedFloat(z),
        }),
        ExpressionValue::Vec4([x, y, z, w]) => PropertyValue::Vec4(Vec4 {
            x: OrderedFloat(x),
            y: OrderedFloat(y),
            z: OrderedFloat(z),
            w: OrderedFloat(w),
        }),
        ExpressionValue::Color([r, g, b, a]) => PropertyValue::Color(Color {
            r: normalized_channel(r),
            g: normalized_channel(g),
            b: normalized_channel(b),
            a: normalized_channel(a),
        }),
        ExpressionValue::Bool(value) => PropertyValue::Boolean(value),
        ExpressionValue::String(value) => PropertyValue::String(value),
    }
}

fn normalized_channel(value: f64) -> u8 {
    (value * 255.0).round() as u8
}

fn evaluation_error(
    kind: ExpressionDiagnosticKind,
    message: impl Into<String>,
) -> ExpressionDiagnostic {
    ExpressionDiagnostic::evaluate(kind, message, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::path::{FillRule, PathValue};
    use crate::model::property::{ColorSpaceRef, ColorValue};

    #[test]
    fn tagged_color_is_not_silently_lowered_to_the_legacy_expression_color()
    -> Result<(), Box<dyn std::error::Error>> {
        let value = PropertyValue::ColorValue(ColorValue::new(
            ColorSpaceRef::new("scene_linear")?,
            [2.0, -0.25, 0.5, 1.0],
        )?);
        let error = match expression_value_from_property(&value) {
            Ok(_) => return Err("tagged color unexpectedly crossed the legacy bridge".into()),
            Err(error) => error,
        };
        assert_eq!(error.kind, ExpressionDiagnosticKind::TypeMismatch);
        assert!(error.message.contains("Tagged graph colors"));
        Ok(())
    }

    #[test]
    fn canonical_path_is_explicitly_rejected_as_expression_fallback() {
        let path = PropertyValue::Path(PathValue::empty(FillRule::EvenOdd));
        let diagnostic = expression_value_from_property(&path).unwrap_err();
        assert_eq!(diagnostic.kind, ExpressionDiagnosticKind::TypeMismatch);
        assert!(diagnostic.to_string().contains("Expression fallback"));
    }
}
