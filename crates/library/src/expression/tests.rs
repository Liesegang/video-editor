use super::{
    ExpressionDiagnosticKind, ExpressionEngine, ExpressionEvaluationContext, ExpressionOutputType,
    ExpressionValue,
};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn compatibility_engine_uses_cpython_and_typed_context() -> TestResult {
    let context = ExpressionEvaluationContext::new(2.0, 24.0, (1920, 1080))?
        .with_value(ExpressionValue::Number(10.0));
    assert_eq!(
        ExpressionEngine::default().evaluate(
            "value + time + frame / fps + resolution.x / width",
            &context,
            ExpressionOutputType::Number,
        )?,
        ExpressionValue::Number(15.0)
    );
    Ok(())
}

#[test]
fn compatibility_engine_preserves_structured_python_diagnostics() -> TestResult {
    let context = ExpressionEvaluationContext::new(0.0, 24.0, (1920, 1080))?;
    let error = ExpressionEngine::default()
        .evaluate("1 / 0", &context, ExpressionOutputType::Number)
        .err()
        .ok_or("Expression unexpectedly succeeded")?;
    assert_eq!(error.kind, ExpressionDiagnosticKind::DivisionByZero);
    assert!(error.traceback.is_some());
    assert!(error.span.is_some());
    Ok(())
}
