use std::collections::HashMap;

use anyhow::{Result, anyhow};
use library::expression::{ExpressionDiagnostic, ExpressionDiagnosticKind};
use library::model::frame::color::Color;
use library::model::property::{Property, PropertyMap, PropertyUiType, PropertyValue, Vec2};
use library::plugin::properties::ExpressionEvaluator;
use library::plugin::{EvaluationContext, PluginManager, PropertyEvaluator};
use ordered_float::OrderedFloat;

fn diagnostic<T>(
    result: std::result::Result<T, ExpressionDiagnostic>,
) -> Result<ExpressionDiagnostic> {
    result
        .err()
        .ok_or_else(|| anyhow!("Expression unexpectedly succeeded"))
}

fn context<'a>(properties: &'a PropertyMap) -> EvaluationContext<'a> {
    EvaluationContext::new(properties, 24.0, (100, 50))
}

#[test]
fn expression_property_persists_source_and_typed_fallback_without_cache_state() -> Result<()> {
    let fallback = PropertyValue::Vec2(Vec2 {
        x: OrderedFloat(12.0),
        y: OrderedFloat(34.0),
    });
    let property = Property::expression("value + vec2(time, 0)".to_string(), fallback.clone());
    assert_eq!(property.evaluator, "expression");
    assert_eq!(property.expression_text(), Some("value + vec2(time, 0)"));
    assert_eq!(property.value(), Some(&fallback));
    assert!(fallback.is_compatible_with(&PropertyUiType::Vec2 {
        suffix: "px".to_string(),
    }));

    let encoded = serde_json::to_string(&property)?;
    assert!(!encoded.contains("cache"));
    let decoded: Property = serde_json::from_str(&encoded)?;
    assert_eq!(decoded, property);
    Ok(())
}

#[test]
fn registered_expression_evaluator_uses_local_context_and_resolution() -> Result<()> {
    let property = Property::expression(
        "value + time + frame / fps + width + height".to_string(),
        PropertyValue::from(1.0),
    );
    let properties = PropertyMap::new();
    let output = PluginManager::default().get_property_evaluators().evaluate(
        &property,
        2.0,
        &context(&properties),
    );
    assert_eq!(output?, PropertyValue::from(155.0));

    let vector = Property::expression(
        "vec2(width / 2, value.y + frame)".to_string(),
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(1.0),
            y: OrderedFloat(3.0),
        }),
    );
    assert_eq!(
        ExpressionEvaluator.evaluate_detailed(&vector, 2.0, &context(&properties))?,
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(50.0),
            y: OrderedFloat(51.0),
        })
    );
    Ok(())
}

#[test]
fn every_supported_property_type_converts_strictly() -> Result<()> {
    let properties = PropertyMap::new();
    let cases = [
        (
            Property::expression("value + 2".to_string(), PropertyValue::Integer(3)),
            PropertyValue::Integer(5),
        ),
        (
            Property::expression("time > 0".to_string(), PropertyValue::Boolean(false)),
            PropertyValue::Boolean(true),
        ),
        (
            Property::expression(
                "value + '-ok'".to_string(),
                PropertyValue::String("text".to_string()),
            ),
            PropertyValue::String("text-ok".to_string()),
        ),
        (
            Property::expression(
                "rgba(0.0, 0.5, 1.0, 1.0)".to_string(),
                PropertyValue::Color(Color::black()),
            ),
            PropertyValue::Color(Color {
                r: 0,
                g: 128,
                b: 255,
                a: 255,
            }),
        ),
    ];
    for (property, expected) in cases {
        assert_eq!(
            ExpressionEvaluator.evaluate_detailed(&property, 2.0, &context(&properties))?,
            expected
        );
    }
    Ok(())
}

#[test]
fn parse_runtime_and_type_failures_return_each_authored_typed_fallback() -> Result<()> {
    let properties = PropertyMap::new();
    let cases = [
        Property::expression("1 +".to_string(), PropertyValue::from(7.0)),
        Property::expression(
            "1 / 0".to_string(),
            PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(4.0),
                y: OrderedFloat(5.0),
            }),
        ),
        Property::expression(
            "rgba(2, 0, 0, 1)".to_string(),
            PropertyValue::Color(Color {
                r: 10,
                g: 20,
                b: 30,
                a: 40,
            }),
        ),
        Property::expression("1".to_string(), PropertyValue::Boolean(true)),
        Property::expression(
            "unknown_name".to_string(),
            PropertyValue::String("fallback".to_string()),
        ),
    ];
    for property in cases {
        let fallback = property
            .value()
            .cloned()
            .ok_or_else(|| anyhow!("test Expression fallback is missing"))?;
        assert_eq!(
            ExpressionEvaluator.evaluate(&property, 2.0, &context(&properties)),
            Ok(fallback)
        );
    }
    Ok(())
}

#[test]
fn detailed_api_preserves_diagnostic_for_inspector_and_node_callers() -> Result<()> {
    let properties = PropertyMap::new();
    let property = Property::expression("1 / 0".to_string(), PropertyValue::from(9.0));
    let error =
        diagnostic(ExpressionEvaluator.evaluate_detailed(&property, 0.0, &context(&properties)))?;
    assert_eq!(error.kind, ExpressionDiagnosticKind::DivisionByZero);

    let unsupported_type = Property {
        evaluator: "expression".to_string(),
        properties: HashMap::from([
            (
                "expression".to_string(),
                PropertyValue::String("1".to_string()),
            ),
            (
                "value".to_string(),
                PropertyValue::Array(vec![PropertyValue::Integer(3)]),
            ),
        ]),
    };
    let error = diagnostic(ExpressionEvaluator.evaluate_detailed(
        &unsupported_type,
        0.0,
        &context(&properties),
    ))?;
    assert_eq!(error.kind, ExpressionDiagnosticKind::TypeMismatch);
    assert!(
        ExpressionEvaluator
            .evaluate(&unsupported_type, 0.0, &context(&properties))
            .is_err(),
        "unsupported authored input types are malformed, not recoverable script errors"
    );
    Ok(())
}

#[test]
fn registered_evaluator_preserves_recoverable_diagnostics_with_typed_value() -> Result<()> {
    let properties = PropertyMap::new();
    let property = Property::expression("1 / 0".to_string(), PropertyValue::from(9.0));
    let registry = PluginManager::default().get_property_evaluators();
    let outcome = registry.evaluate_with_diagnostics(&property, 0.0, &context(&properties))?;
    assert_eq!(outcome.value(), &PropertyValue::from(9.0));
    let diagnostic = outcome
        .diagnostic()
        .ok_or_else(|| anyhow!("recoverable script error lost its diagnostic"))?;
    assert_eq!(diagnostic.evaluator(), "expression");
    assert!(diagnostic.message().contains("division by zero"));
    Ok(())
}

#[test]
fn missing_fallback_and_unknown_evaluator_fail_closed() {
    let properties = PropertyMap::new();
    let context = context(&properties);
    let malformed = Property {
        evaluator: "expression".to_string(),
        properties: HashMap::from([(
            "expression".to_string(),
            PropertyValue::String("1 +".to_string()),
        )]),
    };
    let expression_error = ExpressionEvaluator
        .evaluate(&malformed, 0.0, &context)
        .expect_err("an Expression without a typed fallback must not invent a value");
    assert_eq!(expression_error.evaluator(), "expression");

    let unknown = Property {
        evaluator: "not-installed".to_string(),
        properties: HashMap::new(),
    };
    let registry = PluginManager::default().get_property_evaluators();
    let unknown_error = registry
        .evaluate(&unknown, 0.0, &context)
        .expect_err("an unknown evaluator must not return a legacy Number(0)");
    assert_eq!(unknown_error.evaluator(), "not-installed");
}

#[test]
fn invalid_expression_context_fails_closed_instead_of_returning_input() {
    let properties = PropertyMap::new();
    let property = Property::expression("value".to_string(), PropertyValue::from(4.0));
    let registry = PluginManager::default().get_property_evaluators();
    let invalid = EvaluationContext::new(&properties, 0.0, (1920, 1080));
    assert!(registry.evaluate(&property, 0.0, &invalid).is_err());
}
