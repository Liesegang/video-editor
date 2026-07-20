use std::time::Duration;

use anyhow::{Result, anyhow};
use library::expression::{
    ExpressionDiagnostic, ExpressionDiagnosticKind, ExpressionEngine, ExpressionEvaluationContext,
    ExpressionLimits, ExpressionOutputType, ExpressionPhase, ExpressionValue,
};

fn context() -> Result<ExpressionEvaluationContext> {
    Ok(ExpressionEvaluationContext::new(2.0, 24.0, (1920, 1080))?)
}

fn evaluate(source: &str, output: ExpressionOutputType) -> Result<ExpressionValue> {
    Ok(ExpressionEngine::default().evaluate(source, &context()?, output)?)
}

fn diagnostic<T>(
    result: std::result::Result<T, ExpressionDiagnostic>,
) -> Result<ExpressionDiagnostic> {
    result
        .err()
        .ok_or_else(|| anyhow!("Expression unexpectedly succeeded"))
}

#[test]
fn context_exposes_local_time_frame_resolution_and_fallback_value() -> Result<()> {
    let context = context()?.with_value(ExpressionValue::Number(10.0));
    assert_eq!(
        ExpressionEngine::default().evaluate(
            "value + time + frame / fps + resolution.x / width + resolution[1] / height",
            &context,
            ExpressionOutputType::Number,
        )?,
        ExpressionValue::Number(16.0)
    );
    assert_eq!(
        evaluate("frame_index", ExpressionOutputType::Number)?,
        ExpressionValue::Number(48.0)
    );
    Ok(())
}

#[test]
fn arithmetic_uses_python_division_floor_modulo_and_power_meaning() -> Result<()> {
    let cases = [
        ("5 / 2", 2.5),
        ("-5 // 3", -2.0),
        ("5 // -3", -2.0),
        ("-5 % 3", 1.0),
        ("5 % -3", -1.0),
        ("math.fmod(-5, 3)", -2.0),
        ("2 ** 8", 256.0),
        ("-2 ** 2", -4.0),
    ];
    for (source, expected) in cases {
        assert_eq!(
            evaluate(source, ExpressionOutputType::Number)?,
            ExpressionValue::Number(expected),
            "{source}"
        );
    }

    let error = diagnostic(ExpressionEngine::default().evaluate(
        "(-2) ** 0.5",
        &context()?,
        ExpressionOutputType::Number,
    ))?;
    assert_eq!(error.kind, ExpressionDiagnosticKind::TypeMismatch);
    Ok(())
}

#[test]
fn integer_output_is_strict_while_exact_python_ints_can_widen_to_number() -> Result<()> {
    assert_eq!(
        evaluate("7 // 2", ExpressionOutputType::Integer)?,
        ExpressionValue::Integer(3)
    );
    assert_eq!(
        evaluate("2 ** 8", ExpressionOutputType::Integer)?,
        ExpressionValue::Integer(256)
    );
    assert_eq!(
        evaluate("7", ExpressionOutputType::Number)?,
        ExpressionValue::Number(7.0)
    );

    let engine = ExpressionEngine::default();
    for source in ["7.0", "True"] {
        let error =
            diagnostic(engine.evaluate(source, &context()?, ExpressionOutputType::Integer))?;
        assert_eq!(
            error.kind,
            ExpressionDiagnosticKind::TypeMismatch,
            "{source}"
        );
    }

    let integer_context = context()?.with_value(ExpressionValue::Integer(4));
    assert_eq!(
        engine.evaluate(
            "value + frame_index",
            &integer_context,
            ExpressionOutputType::Integer,
        )?,
        ExpressionValue::Integer(52)
    );
    Ok(())
}

#[test]
fn bool_compare_conditional_and_index_follow_the_supported_python_subset() -> Result<()> {
    assert_eq!(
        evaluate("(0 or 4) + (5 and 2)", ExpressionOutputType::Number)?,
        ExpressionValue::Number(6.0)
    );
    assert_eq!(
        evaluate("0 and (1 / 0)", ExpressionOutputType::Number)?,
        ExpressionValue::Number(0.0)
    );
    assert_eq!(
        evaluate("1 if 2 < 3 < 4 else 9", ExpressionOutputType::Number)?,
        ExpressionValue::Number(1.0)
    );
    assert_eq!(
        evaluate("True == 1 and not False", ExpressionOutputType::Bool)?,
        ExpressionValue::Bool(true)
    );
    assert_eq!(
        evaluate("[10, 20, 30][-1]", ExpressionOutputType::Number)?,
        ExpressionValue::Number(30.0)
    );
    assert_eq!(
        evaluate("'abc'[-1]", ExpressionOutputType::String)?,
        ExpressionValue::String("c".to_string())
    );
    assert_eq!(
        evaluate(
            "9007199254740993 == 9007199254740992",
            ExpressionOutputType::Bool,
        )?,
        ExpressionValue::Bool(false)
    );
    assert_eq!(
        evaluate(
            "9007199254740993 > 9007199254740992.0",
            ExpressionOutputType::Bool,
        )?,
        ExpressionValue::Bool(true)
    );
    assert_eq!(
        evaluate(
            "-9007199254740993 < -9007199254740992.0",
            ExpressionOutputType::Bool,
        )?,
        ExpressionValue::Bool(true)
    );
    assert_eq!(
        evaluate(
            "9223372036854775807 < 9223372036854775808.0",
            ExpressionOutputType::Bool,
        )?,
        ExpressionValue::Bool(true)
    );
    Ok(())
}

#[test]
fn math_vector_color_and_interpolation_helpers_are_typed() -> Result<()> {
    assert_eq!(
        evaluate(
            "vec2(math.sin(pi / 2), clamp(4, 0, 3)) + vec2(1, 2)",
            ExpressionOutputType::Vec2,
        )?,
        ExpressionValue::Vec2([2.0, 5.0])
    );
    assert_eq!(
        evaluate(
            "lerp(vec3(0, 2, 4), vec3(2, 4, 6), 0.5)",
            ExpressionOutputType::Vec3,
        )?,
        ExpressionValue::Vec3([1.0, 3.0, 5.0])
    );
    assert_eq!(
        evaluate("vec4(1, 2, 3, 4)", ExpressionOutputType::Vec4)?,
        ExpressionValue::Vec4([1.0, 2.0, 3.0, 4.0])
    );
    assert_eq!(
        evaluate("rgba(0.1, 0.2, 0.3, 0.4)", ExpressionOutputType::Color)?,
        ExpressionValue::Color([0.1, 0.2, 0.3, 0.4])
    );
    assert_eq!(
        evaluate(
            "dot(normalize(vec2(3, 4)), vec2(3, 4))",
            ExpressionOutputType::Number,
        )?,
        ExpressionValue::Number(5.0)
    );
    assert_eq!(
        evaluate("round(2.5) * 10 + round(3.5)", ExpressionOutputType::Number)?,
        ExpressionValue::Number(24.0)
    );
    assert_eq!(
        evaluate("min([4, 2, 3]) + max(1, 5)", ExpressionOutputType::Number)?,
        ExpressionValue::Number(7.0)
    );
    Ok(())
}

#[test]
fn deterministic_random_and_noise_require_explicit_seeds() -> Result<()> {
    let first = evaluate("random(42) + noise(time, 7)", ExpressionOutputType::Number)?;
    let second = evaluate("random(42) + noise(time, 7)", ExpressionOutputType::Number)?;
    let different = evaluate("random(43) + noise(time, 8)", ExpressionOutputType::Number)?;
    assert_eq!(first, second);
    assert_ne!(first, different);
    Ok(())
}

#[test]
fn result_conversion_is_strict_and_rejects_non_finite_values() -> Result<()> {
    let engine = ExpressionEngine::default();
    let wrong_vector =
        diagnostic(engine.evaluate("[1, 2]", &context()?, ExpressionOutputType::Vec2))?;
    assert_eq!(wrong_vector.kind, ExpressionDiagnosticKind::TypeMismatch);

    let wrong_bool =
        diagnostic(engine.evaluate("True", &context()?, ExpressionOutputType::Number))?;
    assert_eq!(wrong_bool.kind, ExpressionDiagnosticKind::TypeMismatch);

    let non_finite =
        diagnostic(engine.evaluate("1e308 * 1e308", &context()?, ExpressionOutputType::Number))?;
    assert_eq!(non_finite.kind, ExpressionDiagnosticKind::NonFinite);

    for (source, output) in [
        ("vec2(1e308, 1e308) * 1e308", ExpressionOutputType::Vec2),
        ("normalize(vec2(1e308, 1e308))", ExpressionOutputType::Vec2),
        (
            "[vec2(1e308, 1e308) * 1e308][0]",
            ExpressionOutputType::Vec2,
        ),
    ] {
        let error = diagnostic(engine.evaluate(source, &context()?, output))?;
        assert_eq!(error.kind, ExpressionDiagnosticKind::NonFinite, "{source}");
    }
    Ok(())
}

#[test]
fn unavailable_python_capabilities_fail_with_structured_compile_diagnostics() -> Result<()> {
    let engine = ExpressionEngine::default();
    let cases = [
        ("__import__('os')", ExpressionDiagnosticKind::UnknownName),
        ("open('/tmp/file')", ExpressionDiagnosticKind::UnknownName),
        (
            "value.__class__",
            ExpressionDiagnosticKind::UnsupportedSyntax,
        ),
        (
            "[x for x in [1, 2]]",
            ExpressionDiagnosticKind::UnsupportedSyntax,
        ),
        ("lambda: 1", ExpressionDiagnosticKind::UnsupportedSyntax),
        ("[1, 2][0:1]", ExpressionDiagnosticKind::UnsupportedSyntax),
    ];
    for (source, kind) in cases {
        let error = diagnostic(engine.compile(source))?;
        assert_eq!(error.phase, ExpressionPhase::Compile, "{source}");
        assert_eq!(error.kind, kind, "{source}");
        assert!(error.span.is_some(), "{source}");
    }
    Ok(())
}

#[test]
fn deterministic_limits_cover_source_ast_operations_collections_and_strings() -> Result<()> {
    let limits = ExpressionLimits {
        max_source_bytes: 32,
        max_ast_nodes: 32,
        max_depth: 2,
        max_operations: 2,
        max_calls: 1,
        max_collection_items: 2,
        max_string_bytes: 4,
        max_exponent_abs: 4,
        max_wall_time: Duration::from_secs(1),
    };
    let engine = ExpressionEngine::new(limits, 4);
    let compile_cases = [
        "123456789012345678901234567890123",
        "1 + (2 + 3)",
        "[1, 2, 3]",
        "'12345'",
    ];
    for source in compile_cases {
        let error = diagnostic(engine.compile(source))?;
        assert_eq!(
            error.kind,
            ExpressionDiagnosticKind::ResourceLimit,
            "{source}"
        );
    }

    let operation_error =
        diagnostic(engine.evaluate("1 + 2", &context()?, ExpressionOutputType::Number))?;
    assert_eq!(
        operation_error.kind,
        ExpressionDiagnosticKind::ResourceLimit
    );

    let exponent_engine = ExpressionEngine::new(
        ExpressionLimits {
            max_depth: 32,
            max_operations: 100,
            ..engine.limits().clone()
        },
        4,
    );
    let exponent_error =
        diagnostic(exponent_engine.evaluate("2 ** 5", &context()?, ExpressionOutputType::Number))?;
    assert_eq!(exponent_error.kind, ExpressionDiagnosticKind::ResourceLimit);

    let call_engine = ExpressionEngine::new(
        ExpressionLimits {
            max_depth: 32,
            max_operations: 100,
            ..engine.limits().clone()
        },
        4,
    );
    let call_error =
        diagnostic(call_engine.evaluate("abs(abs(1))", &context()?, ExpressionOutputType::Number))?;
    assert_eq!(call_error.kind, ExpressionDiagnosticKind::ResourceLimit);
    Ok(())
}

#[test]
fn compiled_ast_and_compile_diagnostics_are_cached_by_contract_hash() -> Result<()> {
    let engine = ExpressionEngine::default();
    let first = engine.compile("time * 2")?;
    let second = engine.compile("time * 2")?;
    assert_eq!(first.source_hash(), second.source_hash());
    assert_eq!(first.source(), "time * 2");
    assert!(first.ast_node_count() > 0);
    assert_eq!(engine.cache_stats().compilations, 1);
    assert_eq!(engine.cache_stats().hits, 1);

    drop(diagnostic(engine.compile("unknown_name"))?);
    drop(diagnostic(engine.compile("unknown_name"))?);
    assert_eq!(engine.cache_stats().compilations, 2);
    assert_eq!(engine.cache_stats().hits, 2);

    let third = engine.compile("time * 3")?;
    assert_ne!(first.source_hash(), third.source_hash());
    Ok(())
}

#[test]
fn evaluation_is_thread_safe_and_does_not_require_a_python_installation() -> Result<()> {
    let engine = ExpressionEngine::default();
    drop(engine.compile("math.cos(time) + random(12)")?);
    let mut workers = Vec::new();
    for index in 0..8 {
        let engine = engine.clone();
        workers.push(std::thread::spawn(move || -> Result<ExpressionValue> {
            let context = ExpressionEvaluationContext::new(index as f64, 30.0, (1280, 720))?;
            Ok(engine.evaluate(
                "math.cos(time) + random(12)",
                &context,
                ExpressionOutputType::Number,
            )?)
        }));
    }
    for worker in workers {
        let value = worker
            .join()
            .map_err(|_| anyhow!("Expression worker thread panicked"))??;
        assert!(matches!(value, ExpressionValue::Number(number) if number.is_finite()));
    }
    assert!(engine.cache_stats().hits >= 1);
    Ok(())
}

#[test]
fn invalid_context_is_rejected_before_evaluation() -> Result<()> {
    let invalid_time = ExpressionEvaluationContext::new(f64::NAN, 24.0, (1920, 1080));
    let error = diagnostic(invalid_time)?;
    assert_eq!(error.kind, ExpressionDiagnosticKind::InvalidContext);

    let invalid_resolution = ExpressionEvaluationContext::new(0.0, 24.0, (0, 1080));
    let error = diagnostic(invalid_resolution)?;
    assert_eq!(error.kind, ExpressionDiagnosticKind::InvalidContext);

    let mismatched_value = ExpressionEvaluationContext::new(0.0, 24.0, (1920, 1080))?
        .with_value(ExpressionValue::Vec2([0.0, 0.0]));
    let error = diagnostic(ExpressionEngine::default().evaluate(
        "1",
        &mismatched_value,
        ExpressionOutputType::Number,
    ))?;
    assert_eq!(error.kind, ExpressionDiagnosticKind::InvalidContext);
    Ok(())
}
