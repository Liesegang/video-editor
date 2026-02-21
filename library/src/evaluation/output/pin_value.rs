//! PinValue — the typed output of node evaluation.

use super::shape_data::ShapeData;
use crate::rendering::renderer::RenderOutput;
use crate::runtime::color::Color;
use crate::runtime::entity::StyleConfig;

/// The value produced by evaluating a node's output pin.
///
/// Each variant corresponds to a `PinDataType` and carries the concrete
/// runtime value for that type.
#[derive(Clone, Debug)]
pub enum PinValue {
    /// Image data (GPU texture or CPU image).
    Image(RenderOutput),
    /// Single floating-point number.
    Scalar(f64),
    /// 2D vector.
    Vec2(f64, f64),
    /// 3D vector.
    Vec3(f64, f64, f64),
    /// RGBA color.
    Color(Color),
    /// Text string.
    String(String),
    /// Boolean.
    Boolean(bool),
    /// Integer.
    Integer(i64),
    /// SVG path data.
    Path(String),

    // --- Chain types (collected from chained nodes) ---
    /// A single style config (from one style node).
    Style(StyleConfig),
    /// Collected chain of styles (multiple style nodes chained together).
    StyleChain(Vec<StyleConfig>),

    /// Shape data (grouped glyphs, SVG path) for deferred rasterization.
    Shape(ShapeData),

    /// No value / unconnected pin.
    None,
}

impl PinValue {
    /// Extract as scalar, returning default if not a Scalar.
    pub fn as_scalar(&self, default: f64) -> f64 {
        match self {
            PinValue::Scalar(v) => *v,
            PinValue::Integer(v) => *v as f64,
            _ => default,
        }
    }

    /// Extract as Vec2, returning default if not a Vec2.
    pub fn as_vec2(&self, default: (f64, f64)) -> (f64, f64) {
        match self {
            PinValue::Vec2(x, y) => (*x, *y),
            _ => default,
        }
    }

    /// Extract as Color.
    pub fn as_color(&self, default: Color) -> Color {
        match self {
            PinValue::Color(c) => c.clone(),
            _ => default,
        }
    }

    /// Extract as String.
    pub fn as_string(&self) -> Option<&str> {
        match self {
            PinValue::String(s) => Some(s),
            _ => None,
        }
    }

    /// Extract as Image (RenderOutput).
    pub fn into_image(self) -> Option<RenderOutput> {
        match self {
            PinValue::Image(img) => Some(img),
            _ => None,
        }
    }

    /// Extract as StyleChain.
    pub fn into_style_chain(self) -> Vec<StyleConfig> {
        match self {
            PinValue::StyleChain(chain) => chain,
            PinValue::Style(s) => vec![s],
            _ => vec![],
        }
    }

    /// Extract as ShapeData.
    pub fn into_shape(self) -> Option<ShapeData> {
        match self {
            PinValue::Shape(s) => Some(s),
            _ => None,
        }
    }
}
