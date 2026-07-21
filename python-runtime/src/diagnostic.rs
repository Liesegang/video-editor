use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Phase {
    Compile,
    Evaluate,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiagnosticKind {
    Parse,
    TypeMismatch,
    NonFinite,
    InvalidContext,
    Runtime,
}

/// UTF-8 byte offsets in authored source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SourceSpan {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub phase: Phase,
    pub kind: DiagnosticKind,
    pub message: String,
    pub traceback: Option<String>,
    pub span: Option<SourceSpan>,
}

impl Diagnostic {
    pub fn compile(
        kind: DiagnosticKind,
        message: impl Into<String>,
        traceback: Option<String>,
        span: Option<SourceSpan>,
    ) -> Self {
        Self {
            phase: Phase::Compile,
            kind,
            message: message.into(),
            traceback,
            span,
        }
    }

    pub fn evaluate(
        kind: DiagnosticKind,
        message: impl Into<String>,
        span: Option<SourceSpan>,
    ) -> Self {
        Self {
            phase: Phase::Evaluate,
            kind,
            message: message.into(),
            traceback: None,
            span,
        }
    }

    pub fn runtime(
        message: impl Into<String>,
        traceback: Option<String>,
        span: Option<SourceSpan>,
    ) -> Self {
        Self {
            phase: Phase::Evaluate,
            kind: DiagnosticKind::Runtime,
            message: message.into(),
            traceback,
            span,
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(span) = self.span {
            write!(
                formatter,
                "{:?} Python error at bytes {}..{}: {}",
                self.phase, span.start, span.end, self.message
            )?;
        } else {
            write!(formatter, "{:?} Python error: {}", self.phase, self.message)?;
        }
        if let Some(traceback) = &self.traceback {
            write!(formatter, "\n{traceback}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Diagnostic {}
