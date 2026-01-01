use crate::model::frame::entity::FrameObject;
use crate::model::frame::transform::{Position, Scale, Transform};
use crate::model::project::project::Composition;
use crate::model::project::property::{PropertyMap, PropertyValue, Vec2};
use crate::model::project::style::StyleInstance;
use crate::model::project::{EffectConfig, TrackClip};
use crate::plugin::{EvaluationContext, PluginManager, PropertyEvaluatorRegistry};

pub mod effector;
mod image;
mod shape;
mod sksl;
mod text;
mod video;

pub use image::ImageEntityConverterPlugin;
pub use shape::ShapeEntityConverterPlugin;
pub use sksl::SkSLEntityConverterPlugin;
pub use text::TextEntityConverterPlugin;
pub use text::measure_text_size;
pub use video::VideoEntityConverterPlugin;

pub struct FrameEvaluationContext<'a> {
    pub composition: &'a Composition,
    pub property_evaluators: &'a PropertyEvaluatorRegistry,
    pub plugin_manager: &'a PluginManager,
}

impl<'a> FrameEvaluationContext<'a> {
    pub fn evaluate_property_value(
        &self,
        property: &crate::model::project::property::Property,
        properties: &PropertyMap,
        time: f64,
    ) -> PropertyValue {
        let ctx = EvaluationContext {
            property_map: properties,
            fps: self.composition.fps,
        };
        self.property_evaluators.evaluate(property, time, &ctx)
    }

    pub fn evaluate_number(&self, props: &PropertyMap, key: &str, time: f64, default: f64) -> f64 {
        if let Some(prop) = props.get(key) {
            let val = self.evaluate_property_value(prop, props, time);
            val.get_as::<f64>().unwrap_or(default)
        } else {
            default
        }
    }

    pub fn evaluate_vec2(
        &self,
        props: &PropertyMap,
        key: &str,
        time: f64,
        default: [f64; 2],
    ) -> [f64; 2] {
        if let Some(prop) = props.get(key) {
            let val = self.evaluate_property_value(prop, props, time);
            if let Some(v) = val.get_as::<Vec2>() {
                return [v.x.into_inner(), v.y.into_inner()];
            }
        }
        default
    }

    pub fn evaluate_vec2_components(
        &self,
        props: &PropertyMap,
        key_main: &str,
        key_x: &str,
        key_y: &str,
        time: f64,
        default_x: f64,
        default_y: f64,
    ) -> (f64, f64) {
        // Try main key first (Vec2)
        if let Some(prop) = props.get(key_main) {
            let val = self.evaluate_property_value(prop, props, time);
            if let Some(v) = val.get_as::<Vec2>() {
                return (v.x.into_inner(), v.y.into_inner());
            }
        }

        // Fallback to components
        let x = self.evaluate_number(props, key_x, time, default_x);
        let y = self.evaluate_number(props, key_y, time, default_y);
        (x, y)
    }

    pub fn require_string(
        &self,
        props: &PropertyMap,
        key: &str,
        time: f64,
        context: &str,
    ) -> Option<String> {
        if let Some(prop) = props.get(key) {
            let val = self.evaluate_property_value(prop, props, time);
            if let Some(s) = val.get_as::<String>() {
                return Some(s);
            }
        }
        log::warn!(
            "Missing or invalid string property '{}' for {}",
            key,
            context
        );
        None
    }

    pub fn optional_string(&self, props: &PropertyMap, key: &str, time: f64) -> Option<String> {
        if let Some(prop) = props.get(key) {
            let val = self.evaluate_property_value(prop, props, time);
            val.get_as::<String>()
        } else {
            None
        }
    }

    pub fn optional_bool(&self, props: &PropertyMap, key: &str, time: f64) -> Option<bool> {
        if let Some(prop) = props.get(key) {
            let val = self.evaluate_property_value(prop, props, time);
            val.get_as::<bool>()
        } else {
            None
        }
    }

    pub fn build_transform(&self, props: &PropertyMap, time: f64) -> Transform {
        let position = self.evaluate_vec2(props, "position", time, [0.0, 0.0]);
        let anchor = self.evaluate_vec2(props, "anchor", time, [0.0, 0.0]);
        let scale = self.evaluate_vec2(props, "scale", time, [100.0, 100.0]);
        let rotation = self.evaluate_number(props, "rotation", time, 0.0);
        let opacity = self.evaluate_number(props, "opacity", time, 100.0);

        Transform {
            position: Position {
                x: position[0],
                y: position[1],
            },
            anchor: Position {
                x: anchor[0],
                y: anchor[1],
            },
            scale: Scale {
                x: scale[0] / 100.0,
                y: scale[1] / 100.0,
            },
            rotation,
            opacity: opacity / 100.0,
        }
    }

    pub fn build_image_effects(
        &self,
        effects: &[EffectConfig],
        time: f64,
    ) -> Vec<crate::model::frame::effect::ImageEffect> {
        use crate::model::frame::effect::ImageEffect;
        effects
            .iter()
            .map(|e| {
                let mut properties = std::collections::HashMap::new();
                for (key, prop) in e.properties.iter() {
                    let val = self.evaluate_property_value(prop, &e.properties, time);
                    properties.insert(key.clone(), val);
                }
                ImageEffect {
                    effect_type: e.effect_type.clone(),
                    properties,
                }
            })
            .collect()
    }

    pub fn build_styles(
        &self,
        styles: &[StyleInstance],
        time: f64,
    ) -> Vec<crate::model::frame::entity::StyleConfig> {
        styles
            .iter()
            .filter_map(|s| {
                if let Some(plugin) = self.plugin_manager.get_style_plugin(&s.style_type) {
                    plugin.convert(self, s, time)
                } else {
                    log::warn!("Unknown style type: {}", s.style_type);
                    None
                }
            })
            .collect()
    }

    pub fn parse_path_effects(
        &self,
        _props: &PropertyMap,
        _time: f64,
    ) -> Vec<crate::model::frame::draw_type::PathEffect> {
        Vec::new()
    }

    pub fn evaluate_color(
        &self,
        props: &PropertyMap,
        key: &str,
        time: f64,
        default: crate::model::frame::color::Color,
    ) -> crate::model::frame::color::Color {
        use crate::model::frame::color::Color;
        if let Some(prop) = props.get(key) {
            let val = self.evaluate_property_value(prop, props, time);
            if let Some(c) = val.get_as::<Color>() {
                return c;
            }
        }
        default
    }

    pub fn evaluate_cap_type(
        &self,
        props: &PropertyMap,
        key: &str,
        time: f64,
        default: crate::model::frame::draw_type::CapType,
    ) -> crate::model::frame::draw_type::CapType {
        use crate::model::frame::draw_type::CapType;
        if let Some(prop) = props.get(key) {
            let val = self.evaluate_property_value(prop, props, time);
            if let Some(s) = val.get_as::<String>() {
                return match s.to_lowercase().as_str() {
                    "round" => CapType::Round,
                    "square" => CapType::Square,
                    "butt" => CapType::Butt,
                    _ => {
                        log::warn!("Unknown CapType: {}", s);
                        default
                    }
                };
            }
        }
        default
    }

    pub fn evaluate_join_type(
        &self,
        props: &PropertyMap,
        key: &str,
        time: f64,
        default: crate::model::frame::draw_type::JoinType,
    ) -> crate::model::frame::draw_type::JoinType {
        use crate::model::frame::draw_type::JoinType;
        if let Some(prop) = props.get(key) {
            let val = self.evaluate_property_value(prop, props, time);
            if let Some(s) = val.get_as::<String>() {
                return match s.to_lowercase().as_str() {
                    "round" => JoinType::Round,
                    "bevel" => JoinType::Bevel,
                    "miter" => JoinType::Miter,
                    _ => {
                        log::warn!("Unknown JoinType: {}", s);
                        default
                    }
                };
            }
        }
        default
    }

    pub fn evaluate_number_array(&self, props: &PropertyMap, key: &str, time: f64) -> Vec<f64> {
        use crate::model::project::property::PropertyValue;
        if let Some(prop) = props.get(key) {
            let val = self.evaluate_property_value(prop, props, time);
            if let Some(arr) = val.get_as::<Vec<PropertyValue>>() {
                return arr.iter().filter_map(|v| v.get_as::<f64>()).collect();
            }
            if let Some(s) = val.get_as::<String>() {
                return s
                    .split(&[',', ' '][..])
                    .filter(|s| !s.is_empty())
                    .filter_map(|s| s.parse::<f64>().ok())
                    .collect();
            }
        }
        Vec::new()
    }
}

/// Trait for entity converter plugins.
pub trait EntityConverterPlugin: crate::plugin::Plugin + Send + Sync {
    fn supports_kind(&self, kind: &str) -> bool;

    fn convert_entity(
        &self,
        evaluator: &FrameEvaluationContext,
        track_clip: &TrackClip,
        frame_number: u64,
    ) -> Option<FrameObject>;

    fn get_bounds(
        &self,
        _evaluator: &FrameEvaluationContext,
        _track_clip: &TrackClip,
        _frame_number: u64,
    ) -> Option<(f32, f32, f32, f32)> {
        None
    }

    fn get_property_definitions(
        &self,
        _canvas_width: u64,
        _canvas_height: u64,
        _clip_width: u64,
        _clip_height: u64,
    ) -> Vec<crate::model::project::property::PropertyDefinition> {
        Vec::new()
    }

    fn plugin_type(&self) -> crate::plugin::PluginCategory {
        crate::plugin::PluginCategory::EntityConverter
    }
}
