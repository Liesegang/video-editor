//! Property resolution convenience methods for EvalContext.

use super::EvalContext;
use crate::plugin::EvaluationContext;
use crate::project::property::{PropertyMap, PropertyValue};

impl<'a> EvalContext<'a> {
    /// Resolve a property value for a node.
    ///
    /// Reads from the node's own PropertyMap (with keyframe/expression evaluation).
    pub fn resolve_property_value(
        &self,
        properties: &PropertyMap,
        key: &str,
        default: PropertyValue,
    ) -> PropertyValue {
        if let Some(prop) = properties.get(key) {
            let eval_ctx = EvaluationContext {
                property_map: properties,
                fps: self.composition.fps,
            };
            self.property_evaluators
                .evaluate(prop, self.time, &eval_ctx)
        } else {
            default
        }
    }

    /// Convenience: resolve a property as f64.
    pub fn resolve_number(&self, properties: &PropertyMap, key: &str, default: f64) -> f64 {
        match self.resolve_property_value(properties, key, PropertyValue::from(default)) {
            PropertyValue::Number(n) => n.into_inner(),
            _ => default,
        }
    }

    /// Convenience: resolve a property as String.
    pub fn resolve_string(&self, properties: &PropertyMap, key: &str, default: &str) -> String {
        match self.resolve_property_value(
            properties,
            key,
            PropertyValue::String(default.to_string()),
        ) {
            PropertyValue::String(s) => s,
            _ => default.to_string(),
        }
    }

    /// Convenience: resolve a property as Color.
    pub fn resolve_color(
        &self,
        properties: &PropertyMap,
        key: &str,
        default: crate::runtime::color::Color,
    ) -> crate::runtime::color::Color {
        match self.resolve_property_value(properties, key, PropertyValue::Color(default.clone())) {
            PropertyValue::Color(c) => c,
            _ => default,
        }
    }

    /// Convenience: resolve a property as bool.
    pub fn resolve_bool(&self, properties: &PropertyMap, key: &str, default: bool) -> bool {
        match self.resolve_property_value(properties, key, PropertyValue::Boolean(default)) {
            PropertyValue::Boolean(b) => b,
            _ => default,
        }
    }

    /// Get the scaled width of the composition.
    pub fn scaled_width(&self) -> u32 {
        (self.composition.width as f64 * self.render_scale) as u32
    }

    /// Get the scaled height of the composition.
    pub fn scaled_height(&self) -> u32 {
        (self.composition.height as f64 * self.render_scale) as u32
    }

    /// Compute the clip-local evaluation time (seconds).
    ///
    /// Accounts for clip's `in_frame`, `source_begin_frame`, and `fps`.
    pub fn clip_eval_time(&self, clip: &crate::project::clip::TrackClip) -> f64 {
        let delta_frames = self.frame_number as f64 - clip.in_frame as f64;
        let time_offset = delta_frames / self.composition.fps;
        let source_start_time = clip.source_begin_frame as f64 / clip.fps;
        source_start_time + time_offset
    }
}
