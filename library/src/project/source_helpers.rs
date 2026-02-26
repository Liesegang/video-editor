use crate::project::source::SourceData;
use crate::project::source::SourceKind;

impl SourceData {
    // --- UI / Display Helpers ---

    /// Returns the display color of the source based on its kind.
    pub fn display_color(&self) -> (u8, u8, u8) {
        match self.kind {
            SourceKind::Video => (100, 150, 255),       // Blue
            SourceKind::Audio => (100, 255, 150),       // Green
            SourceKind::Image => (255, 100, 150),       // Pink
            SourceKind::Composition => (255, 150, 255), // Magenta
            SourceKind::Text => (255, 200, 100),        // Orange/Yellow
            _ => (128, 128, 128),                       // Gray
        }
    }

    /// Returns the timeline duration in frames.
    pub fn timeline_duration_frames(&self) -> u64 {
        self.out_frame.saturating_sub(self.in_frame)
    }

    /// Helper to get a float property (usually current static value if constant, or generic fallback).
    pub fn get_property_float_or(&self, key: &str, default: f32) -> f32 {
        self.properties.get_f32(key).unwrap_or(default)
    }

    pub fn get_property_vec2_or(&self, key: &str, default: [f32; 2]) -> [f32; 2] {
        if let Some(prop) = self.properties.get(key) {
            if let Some(val) = prop.get_static_value() {
                if let crate::project::property::PropertyValue::Vec2(v) = val {
                    return [v.x.into_inner() as f32, v.y.into_inner() as f32];
                }
            }
        }
        default
    }
}
