//! UI presentation metadata for authored Project properties.

/// Defines how a property should be displayed and edited in the UI.
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyUiType {
    Float {
        min: f64,
        max: f64,
        step: f64,
        suffix: String,
        min_hard_limit: bool,
        max_hard_limit: bool,
    },
    Integer {
        min: i64,
        max: i64,
        suffix: String,
        min_hard_limit: bool,
        max_hard_limit: bool,
    },
    /// Floating-point, tagged, straight-alpha graph color. This must never be
    /// rendered through the legacy 8-bit color picker without an explicit
    /// lossless boundary check.
    ColorValue,
    /// Canonical multi-contour graph path. Editing uses an explicit canonical
    /// import boundary rather than treating SVG text as authoritative state.
    Path,
    Color,
    Text,
    MultilineText,
    Bool,
    Vec2 {
        min: f64,
        max: f64,
        step: f64,
        suffix: String,
        min_hard_limit: bool,
        max_hard_limit: bool,
    },
    Vec3 {
        min: f64,
        max: f64,
        step: f64,
        suffix: String,
        min_hard_limit: bool,
        max_hard_limit: bool,
    },
    Vec4 {
        min: f64,
        max: f64,
        step: f64,
        suffix: String,
        min_hard_limit: bool,
        max_hard_limit: bool,
    },
    Dropdown {
        options: Vec<String>,
    },
    Font,
}

impl PropertyUiType {
    const DEFAULT_VECTOR_MIN: f64 = -1_000_000.0;
    const DEFAULT_VECTOR_MAX: f64 = 1_000_000.0;
    const DEFAULT_VECTOR_STEP: f64 = 0.1;

    /// Whether the registered Python Expression evaluator can produce this
    /// property's authoritative value type today.
    pub const fn supports_expression(&self) -> bool {
        !matches!(self, Self::ColorValue | Self::Path)
    }

    pub fn vec2(suffix: impl Into<String>) -> Self {
        Self::vec2_with_range(
            Self::DEFAULT_VECTOR_MIN,
            Self::DEFAULT_VECTOR_MAX,
            Self::DEFAULT_VECTOR_STEP,
            suffix,
            false,
            false,
        )
    }

    pub fn vec2_with_range(
        min: f64,
        max: f64,
        step: f64,
        suffix: impl Into<String>,
        min_hard_limit: bool,
        max_hard_limit: bool,
    ) -> Self {
        Self::Vec2 {
            min,
            max,
            step,
            suffix: suffix.into(),
            min_hard_limit,
            max_hard_limit,
        }
    }

    pub fn vec3(suffix: impl Into<String>) -> Self {
        Self::vec3_with_range(
            Self::DEFAULT_VECTOR_MIN,
            Self::DEFAULT_VECTOR_MAX,
            Self::DEFAULT_VECTOR_STEP,
            suffix,
            false,
            false,
        )
    }

    pub fn vec3_with_range(
        min: f64,
        max: f64,
        step: f64,
        suffix: impl Into<String>,
        min_hard_limit: bool,
        max_hard_limit: bool,
    ) -> Self {
        Self::Vec3 {
            min,
            max,
            step,
            suffix: suffix.into(),
            min_hard_limit,
            max_hard_limit,
        }
    }

    pub fn vec4(suffix: impl Into<String>) -> Self {
        Self::vec4_with_range(
            Self::DEFAULT_VECTOR_MIN,
            Self::DEFAULT_VECTOR_MAX,
            Self::DEFAULT_VECTOR_STEP,
            suffix,
            false,
            false,
        )
    }

    pub fn vec4_with_range(
        min: f64,
        max: f64,
        step: f64,
        suffix: impl Into<String>,
        min_hard_limit: bool,
        max_hard_limit: bool,
    ) -> Self {
        Self::Vec4 {
            min,
            max,
            step,
            suffix: suffix.into(),
            min_hard_limit,
            max_hard_limit,
        }
    }
}
