//! Shared evaluation context for entity converters.

use log::warn;
use serde_json;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::model::frame::{
    draw_type::{DrawStyle, PathEffect},
    effect::ImageEffect,
    entity::StyleConfig,
    transform::{Position, Scale, Transform},
};
use crate::model::project::EffectConfig;
use crate::model::project::project::Composition;
use crate::model::project::property::{PropertyMap, PropertyValue};
use crate::model::project::style::StyleInstance;
use crate::plugin::{EvaluationContext, PropertyEvaluatorRegistry};

/// Context passed to EntityConverters, encapsulating common FrameEvaluator methods
pub struct FrameEvaluationContext<'a> {
    pub composition: &'a Composition,
    pub property_evaluators: &'a Arc<PropertyEvaluatorRegistry>,
}

impl<'a> FrameEvaluationContext<'a> {
    pub fn build_image_effects(&self, configs: &[EffectConfig], time: f64) -> Vec<ImageEffect> {
        configs
            .iter()
            .filter_map(|config| self.evaluate_image_effect(config, time))
            .collect()
    }

    pub fn evaluate_image_effect(&self, config: &EffectConfig, time: f64) -> Option<ImageEffect> {
        let mut evaluated = HashMap::new();
        for (key, property) in config.properties.iter() {
            let ctx = EvaluationContext {
                property_map: &config.properties,
                fps: self.composition.fps,
            };
            let value = self.property_evaluators.evaluate(property, time, &ctx);
            evaluated.insert(key.clone(), value);
        }
        Some(ImageEffect {
            effect_type: config.effect_type.clone(),
            properties: evaluated,
        })
    }

    pub fn build_styles(&self, instances: &[StyleInstance], time: f64) -> Vec<StyleConfig> {
        instances
            .iter()
            .filter_map(|instance| {
                let props = &instance.properties;
                let color_val = self.evaluate_property_value(props, "color", time);
                let color = match color_val {
                    Some(PropertyValue::Color(c)) => c,
                    Some(PropertyValue::Map(m)) => Self::map_to_color(&m, 255),
                    _ => crate::model::frame::color::Color {
                        r: 0,
                        g: 0,
                        b: 0,
                        a: 255,
                    },
                };
                let offset = self.evaluate_number(props, "offset", time, 0.0);

                let style = match instance.style_type.as_str() {
                    "fill" => DrawStyle::Fill { color, offset },
                    "stroke" => {
                        let width = self.evaluate_number(props, "width", time, 1.0);
                        let miter = self.evaluate_number(props, "miter", time, 4.0);
                        let dash_offset = self.evaluate_number(props, "dash_offset", time, 0.0);
                        DrawStyle::Stroke {
                            color,
                            width,
                            offset,
                            cap: Default::default(),
                            join: Default::default(),
                            miter,
                            dash_array: Vec::new(),
                            dash_offset,
                        }
                    }
                    _ => return None,
                };

                Some(StyleConfig {
                    id: instance.id,
                    style,
                })
            })
            .collect()
    }

    pub fn build_transform(&self, props: &PropertyMap, time: f64) -> Transform {
        let (pos_x, pos_y) = self.evaluate_vec2(
            props,
            "position",
            "position_x",
            "position_y",
            time,
            0.0,
            0.0,
        );
        let (scale_x, scale_y) =
            self.evaluate_vec2(props, "scale", "scale_x", "scale_y", time, 100.0, 100.0);
        let (anchor_x, anchor_y) =
            self.evaluate_vec2(props, "anchor", "anchor_x", "anchor_y", time, 0.0, 0.0);
        let rotation = self.evaluate_number(props, "rotation", time, 0.0);
        let opacity = self.evaluate_number(props, "opacity", time, 100.0);

        Transform {
            position: Position { x: pos_x, y: pos_y },
            scale: Scale {
                x: scale_x / 100.0,
                y: scale_y / 100.0,
            },
            anchor: Position {
                x: anchor_x,
                y: anchor_y,
            },
            rotation,
            opacity: opacity / 100.0,
        }
    }

    pub fn evaluate_property_value(
        &self,
        properties: &PropertyMap,
        key: &str,
        time: f64,
    ) -> Option<PropertyValue> {
        let property = properties.get(key)?;
        let ctx = EvaluationContext {
            property_map: properties,
            fps: self.composition.fps,
        };
        let evaluated_value = self.property_evaluators.evaluate(property, time, &ctx);
        Some(evaluated_value)
    }

    fn map_to_color(
        m: &HashMap<String, PropertyValue>,
        default_alpha: u8,
    ) -> crate::model::frame::color::Color {
        let extract = |key: &str, default: u8| -> u8 {
            m.get(key)
                .and_then(|v| {
                    v.get_as::<i64>()
                        .or_else(|| v.get_as::<f64>().map(|f| f as i64))
                })
                .unwrap_or(default as i64) as u8
        };

        crate::model::frame::color::Color {
            r: extract("r", 0),
            g: extract("g", 0),
            b: extract("b", 0),
            a: extract("a", default_alpha),
        }
    }

    pub fn evaluate_color(
        &self,
        properties: &PropertyMap,
        key: &str,
        time: f64,
        default: crate::model::frame::color::Color,
    ) -> (f32, f32, f32, f32) {
        match self.evaluate_property_value(properties, key, time) {
            Some(PropertyValue::Color(c)) => (
                c.r as f32 / 255.0,
                c.g as f32 / 255.0,
                c.b as f32 / 255.0,
                c.a as f32 / 255.0,
            ),
            Some(PropertyValue::Map(m)) => {
                let c = Self::map_to_color(&m, default.a);
                (
                    c.r as f32 / 255.0,
                    c.g as f32 / 255.0,
                    c.b as f32 / 255.0,
                    c.a as f32 / 255.0,
                )
            }
            _ => (
                default.r as f32 / 255.0,
                default.g as f32 / 255.0,
                default.b as f32 / 255.0,
                default.a as f32 / 255.0,
            ),
        }
    }

    pub fn require_string(
        &self,
        properties: &PropertyMap,
        key: &str,
        time: f64,
        entity_kind: &str,
    ) -> Option<String> {
        match self.evaluate_property_value(properties, key, time) {
            Some(PropertyValue::String(value)) => Some(value),
            other => {
                warn!(
                    "Entity[{}]: invalid or missing '{}' ({:?}); skipping",
                    entity_kind, key, other
                );
                None
            }
        }
    }

    pub fn optional_string(
        &self,
        properties: &PropertyMap,
        key: &str,
        time: f64,
    ) -> Option<String> {
        match self.evaluate_property_value(properties, key, time) {
            Some(PropertyValue::String(value)) => Some(value),
            _ => None,
        }
    }

    pub fn evaluate_number(
        &self,
        properties: &PropertyMap,
        key: &str,
        time: f64,
        default: f64,
    ) -> f64 {
        match self.evaluate_property_value(properties, key, time) {
            Some(PropertyValue::Number(value)) => *value,
            Some(PropertyValue::Integer(value)) => value as f64,
            None => default,
            Some(other) => {
                warn!(
                    "Property '{}' evaluated to {:?} at time {}. Falling back to default {}.",
                    key, other, time, default
                );
                default
            }
        }
    }

    pub fn evaluate_vec2(
        &self,
        properties: &PropertyMap,
        key: &str,
        key_x: &str,
        key_y: &str,
        time: f64,
        default_x: f64,
        default_y: f64,
    ) -> (f64, f64) {
        let val = self.evaluate_property_value(properties, key, time);
        let (mut vx, mut vy) = match val {
            Some(PropertyValue::Vec2(v)) => (*v.x, *v.y),
            Some(PropertyValue::Map(m)) => {
                let x = m
                    .get("x")
                    .and_then(|v| match v {
                        PropertyValue::Number(n) => Some(n.into_inner()),
                        PropertyValue::Integer(i) => Some(*i as f64),
                        _ => None,
                    })
                    .unwrap_or(default_x);
                let y = m
                    .get("y")
                    .and_then(|v| match v {
                        PropertyValue::Number(n) => Some(n.into_inner()),
                        PropertyValue::Integer(i) => Some(*i as f64),
                        _ => None,
                    })
                    .unwrap_or(default_y);
                (x, y)
            }
            _ => (default_x, default_y),
        };

        if let Some(val) = self.evaluate_property_value(properties, key_x, time) {
            match val {
                PropertyValue::Number(n) => vx = n.0,
                PropertyValue::Integer(i) => vx = i as f64,
                _ => {}
            }
        }

        if let Some(val) = self.evaluate_property_value(properties, key_y, time) {
            match val {
                PropertyValue::Number(n) => vy = n.0,
                PropertyValue::Integer(i) => vy = i as f64,
                _ => {}
            }
        }

        (vx, vy)
    }

    #[allow(dead_code)]
    pub fn parse_draw_styles(&self, value: PropertyValue) -> Vec<StyleConfig> {
        match value {
            PropertyValue::Array(arr) => arr
                .into_iter()
                .filter_map(|item| {
                    let json_val: serde_json::Value = (&item).into();
                    if let Ok(config) = serde_json::from_value::<StyleConfig>(json_val.clone()) {
                        return Some(config);
                    }
                    match serde_json::from_value::<DrawStyle>(json_val) {
                        Ok(style) => Some(StyleConfig {
                            id: Uuid::new_v4(),
                            style,
                        }),
                        Err(err) => {
                            warn!("Failed to parse style: {}", err);
                            None
                        }
                    }
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    pub fn parse_path_effects(&self, value: PropertyValue) -> Vec<PathEffect> {
        match value {
            PropertyValue::Array(arr) => arr
                .into_iter()
                .filter_map(|item| {
                    let json_val: serde_json::Value = (&item).into();
                    match serde_json::from_value(json_val) {
                        Ok(effect) => Some(effect),
                        Err(err) => {
                            warn!("Failed to parse path effect: {}", err);
                            None
                        }
                    }
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    pub fn optional_bool(&self, properties: &PropertyMap, key: &str, time: f64) -> Option<bool> {
        match self.evaluate_property_value(properties, key, time) {
            Some(PropertyValue::Boolean(value)) => Some(value),
            _ => None,
        }
    }
}
