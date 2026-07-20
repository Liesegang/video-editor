use super::{ExpressionDiagnostic, ExpressionDiagnosticKind};

/// Output contract selected by the target property or node port.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExpressionOutputType {
    Number,
    Integer,
    Vec2,
    Vec3,
    Vec4,
    Color,
    Bool,
    String,
}

/// Values that can cross the Python Expression engine boundary.
#[derive(Clone, Debug, PartialEq)]
pub enum ExpressionValue {
    Number(f64),
    Integer(i64),
    Vec2([f64; 2]),
    Vec3([f64; 3]),
    Vec4([f64; 4]),
    /// Linear normalized RGBA channels. Each channel must be in `0.0..=1.0`.
    Color([f64; 4]),
    Bool(bool),
    String(String),
}

impl ExpressionValue {
    pub const fn output_type(&self) -> ExpressionOutputType {
        match self {
            Self::Number(_) => ExpressionOutputType::Number,
            Self::Integer(_) => ExpressionOutputType::Integer,
            Self::Vec2(_) => ExpressionOutputType::Vec2,
            Self::Vec3(_) => ExpressionOutputType::Vec3,
            Self::Vec4(_) => ExpressionOutputType::Vec4,
            Self::Color(_) => ExpressionOutputType::Color,
            Self::Bool(_) => ExpressionOutputType::Bool,
            Self::String(_) => ExpressionOutputType::String,
        }
    }
}

/// Read-only locals visible to an Expression.
#[derive(Clone, Debug, PartialEq)]
pub struct ExpressionEvaluationContext {
    time: f64,
    fps: f64,
    width: u64,
    height: u64,
    value: Option<ExpressionValue>,
}

impl ExpressionEvaluationContext {
    pub fn new(time: f64, fps: f64, resolution: (u64, u64)) -> Result<Self, ExpressionDiagnostic> {
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

    pub fn with_value(mut self, value: ExpressionValue) -> Self {
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

    pub fn value(&self) -> Option<&ExpressionValue> {
        self.value.as_ref()
    }
}

fn invalid_context(message: &str) -> ExpressionDiagnostic {
    ExpressionDiagnostic::evaluate(ExpressionDiagnosticKind::InvalidContext, message, None)
}
