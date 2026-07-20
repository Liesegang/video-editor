use std::fmt;

/// Stage at which a Python Expression failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExpressionPhase {
    Compile,
    Evaluate,
}

/// Stable category for presenting or filtering Expression diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExpressionDiagnosticKind {
    Parse,
    UnsupportedSyntax,
    UnknownName,
    InvalidArguments,
    TypeMismatch,
    DivisionByZero,
    NonFinite,
    ResourceLimit,
    InvalidContext,
    Runtime,
}

/// UTF-8 byte offsets in the authored Expression source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ExpressionSourceSpan {
    pub start: usize,
    pub end: usize,
}

/// A user-facing, structured failure. Evaluation callers decide whether to
/// use a typed property fallback or produce no node output.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpressionDiagnostic {
    pub phase: ExpressionPhase,
    pub kind: ExpressionDiagnosticKind,
    pub message: String,
    pub span: Option<ExpressionSourceSpan>,
}

impl ExpressionDiagnostic {
    pub(crate) fn compile(
        kind: ExpressionDiagnosticKind,
        message: impl Into<String>,
        span: Option<ExpressionSourceSpan>,
    ) -> Self {
        Self {
            phase: ExpressionPhase::Compile,
            kind,
            message: message.into(),
            span,
        }
    }

    pub(crate) fn evaluate(
        kind: ExpressionDiagnosticKind,
        message: impl Into<String>,
        span: Option<ExpressionSourceSpan>,
    ) -> Self {
        Self {
            phase: ExpressionPhase::Evaluate,
            kind,
            message: message.into(),
            span,
        }
    }
}

impl fmt::Display for ExpressionDiagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(span) = self.span {
            write!(
                formatter,
                "{:?} Expression error at bytes {}..{}: {}",
                self.phase, span.start, span.end, self.message
            )
        } else {
            write!(
                formatter,
                "{:?} Expression error: {}",
                self.phase, self.message
            )
        }
    }
}

impl std::error::Error for ExpressionDiagnostic {}
