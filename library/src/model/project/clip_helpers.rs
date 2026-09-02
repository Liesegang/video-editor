use crate::model::property::PropertyValue;
use crate::model::{Clip, GeneratorContent, Node, NodeContent};

impl Clip {
    /// Returns this timeline container's duration in seconds.
    pub fn duration_seconds(&self) -> f64 {
        self.duration.into_inner()
    }

    /// Reads a static Clip property, falling back when it is absent or animated.
    pub fn get_property_float_or(&self, key: &str, default: f32) -> f32 {
        self.properties.get_f32(key).unwrap_or(default)
    }

    /// Reads a static Clip Vec2 property, falling back when it is absent or animated.
    pub fn get_property_vec2_or(&self, key: &str, default: [f32; 2]) -> [f32; 2] {
        static_vec2_or(&self.properties, key, default)
    }
}

impl Node {
    /// Returns the display color for the leaf Node's content kind.
    pub fn display_color(&self) -> (u8, u8, u8) {
        match self.content() {
            NodeContent::Media(_) => (100, 150, 255),
            NodeContent::Generator(generator) => match generator {
                GeneratorContent::Text => (255, 200, 100),
                GeneratorContent::Shape => (128, 128, 128),
                GeneratorContent::Solid => self
                    .properties()
                    .get("color")
                    .and_then(|property| property.get_static_value())
                    .and_then(|value| match value {
                        PropertyValue::ColorValue(color) => color
                            .try_to_renderer_srgba8()
                            .ok()
                            .map(|color| (color.r, color.g, color.b)),
                        PropertyValue::Color(color) => Some((color.r, color.g, color.b)),
                        _ => None,
                    })
                    .unwrap_or((128, 128, 128)),
                GeneratorContent::SkSL => (100, 200, 200),
            },
            NodeContent::CompositionInstance(_) => (255, 150, 255),
            NodeContent::PluginOperation(_) => (180, 140, 220),
            NodeContent::Value(_) => (125, 190, 210),
            NodeContent::Color(_) => (205, 145, 105),
            NodeContent::Data(_) => (170, 130, 205),
            NodeContent::List(_) => (110, 180, 155),
            NodeContent::Path(_) => (170, 130, 205),
            NodeContent::NativeOperation(_) => (210, 145, 90),
            NodeContent::Merge => (150, 180, 190),
            NodeContent::SoundMerge => (170, 135, 205),
            NodeContent::SoundAnalysis(_) => (120, 190, 205),
        }
    }

    /// Reads a static leaf-Node property, falling back when absent or animated.
    pub fn get_property_float_or(&self, key: &str, default: f32) -> f32 {
        self.properties().get_f32(key).unwrap_or(default)
    }

    /// Reads a static leaf-Node Vec2 property, falling back when absent or animated.
    pub fn get_property_vec2_or(&self, key: &str, default: [f32; 2]) -> [f32; 2] {
        static_vec2_or(self.properties(), key, default)
    }
}

fn static_vec2_or(
    properties: &crate::model::property::PropertyMap,
    key: &str,
    default: [f32; 2],
) -> [f32; 2] {
    let Some(property) = properties.get(key) else {
        return default;
    };
    let Some(PropertyValue::Vec2(value)) = property.get_static_value() else {
        return default;
    };
    [value.x.into_inner() as f32, value.y.into_inner() as f32]
}
