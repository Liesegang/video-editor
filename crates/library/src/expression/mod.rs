//! Trusted CPython expressions shared by properties and easing.
//!
//! Authored code has ordinary Python builtins and imports. It is not sandboxed;
//! opening a Project containing Python must be treated as executing code.

mod engine;

pub(crate) use engine::ExpressionEngine;
pub use ruvie_python_runtime::{
    Diagnostic as ExpressionDiagnostic, DiagnosticKind as ExpressionDiagnosticKind,
    EvaluationContext as ExpressionEvaluationContext, OutputType as ExpressionOutputType,
    Phase as ExpressionPhase, PythonValue as ExpressionValue, SourceSpan as ExpressionSourceSpan,
};

#[cfg(test)]
mod tests;
