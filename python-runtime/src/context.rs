use crate::{Diagnostic, DiagnosticKind, PythonValue};

/// Typed locals supplied by a host application to authored Python code.
#[derive(Clone, Debug, PartialEq)]
pub struct EvaluationContext {
    time: f64,
    fps: f64,
    width: u64,
    height: u64,
    value: Option<PythonValue>,
}

impl EvaluationContext {
    pub fn new(time: f64, fps: f64, resolution: (u64, u64)) -> Result<Self, Diagnostic> {
        if !time.is_finite() {
            return Err(invalid_context("time must be finite"));
        }
        if !fps.is_finite() || fps <= 0.0 {
            return Err(invalid_context("fps must be finite and greater than zero"));
        }
        if !(time * fps).is_finite() {
            return Err(invalid_context("derived frame value must be finite"));
        }
        if resolution.0 == 0 || resolution.1 == 0 {
            return Err(invalid_context(
                "resolution width and height must be greater than zero",
            ));
        }
        Ok(Self {
            time,
            fps,
            width: resolution.0,
            height: resolution.1,
            value: None,
        })
    }

    pub fn with_value(mut self, value: PythonValue) -> Self {
        self.value = Some(value);
        self
    }

    pub const fn time(&self) -> f64 {
        self.time
    }

    pub const fn fps(&self) -> f64 {
        self.fps
    }

    pub const fn width(&self) -> u64 {
        self.width
    }

    pub const fn height(&self) -> u64 {
        self.height
    }

    pub fn frame(&self) -> f64 {
        self.time * self.fps
    }

    pub fn value(&self) -> Option<&PythonValue> {
        self.value.as_ref()
    }
}

fn invalid_context(message: &str) -> Diagnostic {
    Diagnostic::evaluate(DiagnosticKind::InvalidContext, message, None)
}
