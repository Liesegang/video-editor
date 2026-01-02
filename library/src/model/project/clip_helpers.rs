use crate::model::{GeneratorContent, Layer, LayerContent};

impl Layer {
    // --- UI / Display Helpers ---

    /// Returns the display color of the layer based on its content.
    pub fn display_color(&self) -> (u8, u8, u8) {
        match &self.content {
            LayerContent::Media(_) => (100, 150, 255), // Blue (Generic Media)
            LayerContent::Generator(generator) => match generator {
                GeneratorContent::Text { .. } => (255, 200, 100), // Orange/Yellow
                GeneratorContent::Shape { .. } => (128, 128, 128), // Gray
                GeneratorContent::Solid { color } => (color.r, color.g, color.b), // The solid color itself? Or icon color? Let's use Gray for UI icon.
                GeneratorContent::SkSL { .. } => (100, 200, 200),                 // Cyan-ish
            },
            LayerContent::Reference(_) => (255, 150, 255), // Magenta
        }
    }

    /// Returns the duration in seconds.
    pub fn duration_seconds(&self) -> f64 {
        self.duration.into_inner()
    }

    /// Helper to get a float property (usually current static value if constant, or generic fallback).
    pub fn get_property_float_or(&self, key: &str, default: f32) -> f32 {
        self.properties.get_f32(key).unwrap_or(default)
    }

    pub fn get_property_vec2_or(&self, key: &str, default: [f32; 2]) -> [f32; 2] {
        if let Some(prop) = self.properties.get(key) {
            if let Some(val) = prop.get_static_value() {
                if let crate::model::property::PropertyValue::Vec2(v) = val {
                    return [v.x.into_inner() as f32, v.y.into_inner() as f32];
                }
            }
        }
        default
    }
}
