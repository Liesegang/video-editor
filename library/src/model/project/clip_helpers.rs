use crate::model::project::TrackClip;
use crate::model::project::TrackClipKind;

impl TrackClip {
    // --- UI / Display Helpers ---

    /// Returns the display color of the clip based on its kind.
    pub fn display_color(&self) -> (u8, u8, u8) {
        match self.kind {
            TrackClipKind::Video => (100, 150, 255), // Blue
            TrackClipKind::Audio => (100, 255, 150), // Green
            TrackClipKind::Image => (255, 100, 150), // Pink
            TrackClipKind::Composition => (255, 150, 255), // Magenta
            TrackClipKind::Text => (255, 200, 100),  // Orange/Yellow
            _ => (128, 128, 128), // Gray
        }
    }

    /// Returns the timeline duration in frames.
    pub fn timeline_duration_frames(&self) -> u64 {
        self.out_frame.saturating_sub(self.in_frame)
    }

    /// Helper to get a float property (usually current static value if constant, or generic fallback).
    /// Note: This does NOT evaluate keyframes. It just grabs a 'base' value if available.
    /// For accurate values at a specific time, use the Evaluator.
    /// This is strictly for simple UI display where exact animation state might not be needed or available cheaply.
    pub fn get_property_float_or(&self, key: &str, default: f32) -> f32 {
        self.properties.get_f32(key).unwrap_or(default)
    }

    pub fn get_property_vec2_or(&self, key: &str, default: [f32; 2]) -> [f32; 2] {
        if let Some(prop) = self.properties.get(key) {
             if let Some(val) = prop.get_static_value() {
                 if let crate::model::project::property::PropertyValue::Vec2(v) = val {
                     return [v.x.into_inner() as f32, v.y.into_inner() as f32];
                 }
             }
        }
        default
    }
}
