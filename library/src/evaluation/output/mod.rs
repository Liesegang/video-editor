//! Typed output values produced by node evaluation.

mod pin_value;
pub mod shape_data;

pub use pin_value::PinValue;
pub use shape_data::{DecorationShape, FontInfo, LineInfo, ShapeData, ShapeGroup};
