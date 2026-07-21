use std::fs;
use std::sync::Arc;

use ruvie_python_runtime::{
    Diagnostic, DiagnosticKind, EvaluationContext, OutputType, Phase, PythonHost, PythonHostConfig,
    PythonValue,
};

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

fn context() -> TestResult<EvaluationContext> {
    Ok(EvaluationContext::new(2.0, 24.0, (1920, 1080))?)
}

fn expected_error<T>(result: Result<T, Diagnostic>) -> TestResult<Diagnostic> {
    result
        .err()
        .ok_or_else(|| "Python operation unexpectedly succeeded".into())
}

#[test]
fn runs_real_cpython_with_math_and_timeline_locals() -> TestResult {
    let host = PythonHost::new(PythonHostConfig::default())?;
    assert_eq!(
        host.evaluate(
            "__import__('sys').implementation.name == 'cpython'",
            &context()?,
            OutputType::Bool,
        )?,
        PythonValue::Bool(true)
    );
    assert_eq!(
        host.evaluate(
            "math.sqrt(time) + frame / fps + resolution.x / width",
            &context()?,
            OutputType::Number,
        )?,
        PythonValue::Number(2.0_f64.sqrt() + 3.0)
    );
    Ok(())
}

#[test]
fn converts_all_property_boundary_types() -> TestResult {
    let host = PythonHost::new(PythonHostConfig::default())?;
    let cases = [
        ("2.5", OutputType::Number, PythonValue::Number(2.5)),
        ("7", OutputType::Integer, PythonValue::Integer(7)),
        (
            "vec2(1, 2)",
            OutputType::Vec2,
            PythonValue::Vec2([1.0, 2.0]),
        ),
        (
            "[1, 2, 3]",
            OutputType::Vec3,
            PythonValue::Vec3([1.0, 2.0, 3.0]),
        ),
        (
            "(1, 2, 3, 4)",
            OutputType::Vec4,
            PythonValue::Vec4([1.0, 2.0, 3.0, 4.0]),
        ),
        (
            "rgba(0.1, 0.2, 0.3, 0.4)",
            OutputType::Color,
            PythonValue::Color([0.1, 0.2, 0.3, 0.4]),
        ),
        ("time > 1", OutputType::Bool, PythonValue::Bool(true)),
        (
            "f'{width}x{height}'",
            OutputType::String,
            PythonValue::String("1920x1080".to_string()),
        ),
    ];
    for (source, output, expected) in cases {
        assert_eq!(host.evaluate(source, &context()?, output)?, expected);
    }
    Ok(())
}

#[test]
fn reports_cpython_syntax_and_runtime_tracebacks_with_source_spans() -> TestResult {
    let host = PythonHost::new(PythonHostConfig::default())?;
    let syntax = expected_error(host.compile_expression("time +\n"))?;
    assert_eq!(syntax.phase, Phase::Compile);
    assert_eq!(syntax.kind, DiagnosticKind::Parse);
    assert!(syntax.span.is_some());
    assert!(syntax.message.contains("SyntaxError"));

    let runtime = expected_error(host.evaluate("10 + (1 / 0)", &context()?, OutputType::Number))?;
    assert_eq!(runtime.phase, Phase::Evaluate);
    assert_eq!(runtime.kind, DiagnosticKind::DivisionByZero);
    let traceback = runtime.traceback.as_deref().unwrap_or_default();
    assert!(traceback.contains("<ruvie-expression>"));
    assert!(traceback.contains("ZeroDivisionError"));
    let span = runtime.span.ok_or("runtime diagnostic has no span")?;
    assert!(span.end > span.start);
    assert!(span.end <= "10 + (1 / 0)".len());
    Ok(())
}

#[test]
fn maps_cpython_unicode_columns_to_utf8_byte_spans() -> TestResult {
    let host = PythonHost::new(PythonHostConfig::default())?;
    let syntax_source = "'é' + )";
    let syntax = expected_error(host.compile_expression(syntax_source))?;
    let syntax_span = syntax.span.ok_or("syntax diagnostic has no span")?;
    assert!(syntax_span.start <= syntax_source.len());
    assert!(syntax_span.end <= syntax_source.len());
    assert!(syntax_source.is_char_boundary(syntax_span.start));
    assert!(syntax_source.is_char_boundary(syntax_span.end));

    let runtime_source = "len('é') + (1 / 0)";
    let runtime = expected_error(host.evaluate(runtime_source, &context()?, OutputType::Number))?;
    let runtime_span = runtime.span.ok_or("runtime diagnostic has no span")?;
    assert!(runtime_source.is_char_boundary(runtime_span.start));
    assert!(runtime_source.is_char_boundary(runtime_span.end));
    let highlighted = runtime_source
        .get(runtime_span.start..runtime_span.end)
        .ok_or("runtime span is not valid UTF-8")?;
    assert!(
        highlighted.contains("1 / 0"),
        "highlighted: {highlighted:?}"
    );
    Ok(())
}

#[test]
fn preserves_authored_expression_helper_contract() -> TestResult {
    let host = PythonHost::new(PythonHostConfig::default())?;
    assert_eq!(
        host.evaluate(
            "math.fmod(-5, 3) + fmod(5, 3) + smoothstep(0, 1, 0.5)",
            &context()?,
            OutputType::Number,
        )?,
        PythonValue::Number(0.5)
    );
    assert_eq!(
        host.evaluate("rgb(0.1, 0.2, 0.3)", &context()?, OutputType::Color)?,
        PythonValue::Color([0.1, 0.2, 0.3, 1.0])
    );
    let value_context = context()?.with_value(PythonValue::Vec2([2.0, 3.0]));
    assert_eq!(
        host.evaluate("value * 2", &value_context, OutputType::Vec2)?,
        PythonValue::Vec2([4.0, 6.0])
    );
    Ok(())
}

#[test]
fn uses_python_only_semantics_without_a_rust_ast_or_evaluator() -> TestResult {
    let host = PythonHost::new(PythonHostConfig::default())?;
    assert_eq!(
        host.evaluate(
            "sum(x * x for x in range(6))",
            &context()?,
            OutputType::Integer,
        )?,
        PythonValue::Integer(55)
    );
    assert_eq!(
        host.evaluate(
            "type('Curve', (), {'sample': lambda self, x: x ** 3})().sample(time)",
            &context()?,
            OutputType::Number,
        )?,
        PythonValue::Number(8.0)
    );
    Ok(())
}

#[test]
fn caches_compiled_code_and_supports_multithreaded_callers() -> TestResult {
    let host = Arc::new(PythonHost::new(PythonHostConfig::default())?);
    drop(host.compile_expression("math.cos(time) + 1")?);
    drop(host.compile_expression("math.cos(time) + 1")?);
    assert_eq!(host.cache_stats().compilations, 1);
    assert_eq!(host.cache_stats().hits, 1);

    let workers = (0..8)
        .map(|index| {
            let host = Arc::clone(&host);
            std::thread::spawn(move || -> TestResult<PythonValue> {
                let context = EvaluationContext::new(index as f64, 30.0, (1280, 720))?;
                Ok(host.evaluate("math.cos(time) + 1", &context, OutputType::Number)?)
            })
        })
        .collect::<Vec<_>>();
    for worker in workers {
        let value = worker.join().map_err(|_| "Python worker panicked")??;
        assert!(matches!(value, PythonValue::Number(value) if value.is_finite()));
    }
    assert_eq!(host.cache_stats().compilations, 1);
    assert!(host.cache_stats().hits >= 9);
    Ok(())
}

#[test]
fn imports_pure_python_from_an_explicit_temporary_site_packages_path() -> TestResult {
    let temporary = tempfile::tempdir()?;
    let site_packages = temporary.path().join("site-packages");
    let package = site_packages.join("ruvie_fixture_plugin");
    fs::create_dir_all(&package)?;
    let _copied = fs::copy(
        "tests/fixtures/ruvie_fixture_plugin/ruvie_fixture_plugin/__init__.py",
        package.join("__init__.py"),
    )?;

    let host = PythonHost::new(PythonHostConfig {
        extra_site_package_paths: vec![site_packages],
        ..PythonHostConfig::default()
    })?;
    assert_eq!(
        host.evaluate(
            "__import__('ruvie_fixture_plugin').curve(time)",
            &context()?,
            OutputType::Number,
        )?,
        PythonValue::Number(5.0)
    );
    Ok(())
}
