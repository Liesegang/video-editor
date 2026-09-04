/// Output contract chosen by the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OutputType {
    Number,
    Integer,
    Vec2,
    Vec3,
    Vec4,
    Color,
    Bool,
    String,
}

/// Values crossing the Rust/Python boundary in the first runtime slice.
#[derive(Clone, Debug, PartialEq)]
pub enum PythonValue {
    Number(f64),
    Integer(i64),
    Vec2([f64; 2]),
    Vec3([f64; 3]),
    Vec4([f64; 4]),
    Color([f64; 4]),
    Bool(bool),
    String(String),
}

impl PythonValue {
    pub const fn output_type(&self) -> OutputType {
        match self {
            Self::Number(_) => OutputType::Number,
            Self::Integer(_) => OutputType::Integer,
            Self::Vec2(_) => OutputType::Vec2,
            Self::Vec3(_) => OutputType::Vec3,
            Self::Vec4(_) => OutputType::Vec4,
            Self::Color(_) => OutputType::Color,
            Self::Bool(_) => OutputType::Bool,
            Self::String(_) => OutputType::String,
        }
    }
}
