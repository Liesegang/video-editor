//! PinValue — the typed output of node evaluation.

use super::audio::AudioChunk;
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

    /// Audio sample data.
    Audio(AudioChunk),

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

    /// Extract as AudioChunk.
    pub fn into_audio(self) -> Option<AudioChunk> {
        match self {
            PinValue::Audio(a) => Some(a),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::color::Color;
    use crate::runtime::draw_type::DrawStyle;
    use uuid::Uuid;

    #[test]
    fn as_scalar_returns_value_for_scalar() {
        let pv = PinValue::Scalar(42.0);
        assert_eq!(pv.as_scalar(0.0), 42.0);
    }

    #[test]
    fn as_scalar_converts_integer() {
        let pv = PinValue::Integer(7);
        assert_eq!(pv.as_scalar(0.0), 7.0);
    }

    #[test]
    fn as_scalar_returns_default_for_wrong_type() {
        let pv = PinValue::String("hello".into());
        assert_eq!(pv.as_scalar(-1.0), -1.0);
    }

    #[test]
    fn as_vec2_returns_value() {
        let pv = PinValue::Vec2(1.0, 2.0);
        assert_eq!(pv.as_vec2((0.0, 0.0)), (1.0, 2.0));
    }

    #[test]
    fn as_vec2_returns_default_for_wrong_type() {
        let pv = PinValue::Scalar(5.0);
        assert_eq!(pv.as_vec2((9.0, 9.0)), (9.0, 9.0));
    }

    #[test]
    fn as_color_returns_value() {
        let c = Color {
            r: 10,
            g: 20,
            b: 30,
            a: 255,
        };
        let pv = PinValue::Color(c.clone());
        assert_eq!(pv.as_color(Color::black()), c);
    }

    #[test]
    fn as_color_returns_default_for_wrong_type() {
        let pv = PinValue::None;
        let def = Color::white();
        assert_eq!(pv.as_color(def.clone()), def);
    }

    #[test]
    fn as_string_returns_some_for_string() {
        let pv = PinValue::String("hello".into());
        assert_eq!(pv.as_string(), Some("hello"));
    }

    #[test]
    fn as_string_returns_none_for_wrong_type() {
        let pv = PinValue::Scalar(1.0);
        assert_eq!(pv.as_string(), None);
    }

    #[test]
    fn into_image_returns_none_for_non_image() {
        let pv = PinValue::Scalar(1.0);
        assert!(pv.into_image().is_none());
    }

    #[test]
    fn into_style_chain_from_chain() {
        let s1 = StyleConfig {
            id: Uuid::new_v4(),
            style: DrawStyle::default(),
        };
        let s2 = StyleConfig {
            id: Uuid::new_v4(),
            style: DrawStyle::default(),
        };
        let pv = PinValue::StyleChain(vec![s1.clone(), s2.clone()]);
        let chain = pv.into_style_chain();
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[0].id, s1.id);
        assert_eq!(chain[1].id, s2.id);
    }

    #[test]
    fn into_style_chain_from_single_style() {
        let s = StyleConfig {
            id: Uuid::new_v4(),
            style: DrawStyle::default(),
        };
        let pv = PinValue::Style(s.clone());
        let chain = pv.into_style_chain();
        assert_eq!(chain.len(), 1);
        assert_eq!(chain[0].id, s.id);
    }

    #[test]
    fn into_style_chain_returns_empty_for_wrong_type() {
        let pv = PinValue::None;
        assert!(pv.into_style_chain().is_empty());
    }

    #[test]
    fn into_shape_returns_none_for_non_shape() {
        let pv = PinValue::Scalar(1.0);
        assert!(pv.into_shape().is_none());
    }

    #[test]
    fn into_audio_returns_some_for_audio() {
        let chunk = AudioChunk {
            samples: vec![0.1, -0.1, 0.2, -0.2],
            sample_rate: 44100,
            channels: 2,
        };
        let pv = PinValue::Audio(chunk);
        let result = pv.into_audio().unwrap();
        assert_eq!(result.samples.len(), 4);
        assert_eq!(result.sample_rate, 44100);
        assert_eq!(result.channels, 2);
    }

    #[test]
    fn into_audio_returns_none_for_non_audio() {
        let pv = PinValue::None;
        assert!(pv.into_audio().is_none());
    }

    #[test]
    fn none_variant_defaults() {
        let pv = PinValue::None;
        assert_eq!(pv.as_scalar(5.0), 5.0);
        assert_eq!(pv.as_vec2((1.0, 2.0)), (1.0, 2.0));
        assert!(pv.as_string().is_none());
    }

    #[test]
    fn boolean_and_integer_variants() {
        let b = PinValue::Boolean(true);
        assert_eq!(b.as_scalar(0.0), 0.0); // Boolean doesn't convert to scalar
        assert!(b.as_string().is_none());

        let i = PinValue::Integer(42);
        assert_eq!(i.as_scalar(0.0), 42.0); // Integer converts to scalar
    }

    #[test]
    fn path_variant() {
        let pv = PinValue::Path("M 0 0 L 10 10".into());
        assert!(pv.as_string().is_none()); // Path is not String
    }

    #[test]
    fn vec3_variant() {
        let pv = PinValue::Vec3(1.0, 2.0, 3.0);
        // Vec3 doesn't extract as Vec2
        assert_eq!(pv.as_vec2((0.0, 0.0)), (0.0, 0.0));
    }
}
