use std::collections::HashMap;
use std::sync::Arc;
use log::warn;
use serde_json;

use crate::model::frame::{
    color::Color,
    draw_type::{DrawStyle, PathEffect},
    effect::ImageEffect,
    entity::{FrameEntity, FrameObject, ImageSurface},
    transform::{Position, Scale, Transform},
};
use crate::model::project::entity::{EffectConfig, Entity};
use crate::model::project::project::Composition;
use crate::model::project::property::{PropertyMap, PropertyValue};
use super::property::{EvaluationContext, PropertyEvaluatorRegistry};


/// Trait for converting an Entity into a FrameObject.
pub trait EntityConverter: Send + Sync {
    fn convert_entity(
        &self,
        evaluator: &FrameEvaluationContext, // Pass context instead of individual fields
        entity: &Entity,
        time: f64,
    ) -> Option<FrameObject>;
}

/// Context passed to EntityConverters, encapsulating common FrameEvaluator methods
pub struct FrameEvaluationContext<'a> {
    pub composition: &'a Composition,
    pub property_evaluators: &'a Arc<PropertyEvaluatorRegistry>,
    // Add other common methods or fields from FrameEvaluator if needed
}

impl<'a> FrameEvaluationContext<'a> {
    // Re-implement helper methods previously in FrameEvaluator
    fn build_image_effects(&self, configs: &[EffectConfig], time: f64) -> Vec<ImageEffect> {
        configs
            .iter()
            .filter_map(|config| self.evaluate_image_effect(config, time))
            .collect()
    }

    fn evaluate_image_effect(&self, config: &EffectConfig, time: f64) -> Option<ImageEffect> {
        let mut evaluated = HashMap::new();
        for (key, property) in config.properties.iter() {
            let ctx = EvaluationContext {
                property_map: &config.properties,
            };
            let value = self.property_evaluators.evaluate(property, time, &ctx);
            evaluated.insert(key.clone(), value);
        }
        Some(ImageEffect {
            effect_type: config.effect_type.clone(),
            properties: evaluated,
        })
    }

    fn build_transform(&self, props: &PropertyMap, time: f64) -> Transform {
        let (pos_x, pos_y) = self.evaluate_vec2(props, "position", time, 0.0, 0.0);
        let (scale_x, scale_y) = self.evaluate_vec2(props, "scale", time, 1.0, 1.0);
        let (anchor_x, anchor_y) = self.evaluate_vec2(props, "anchor", time, 0.0, 0.0);
        let rotation = self.evaluate_number(props, "rotation", time, 0.0);

        Transform {
            position: Position { x: pos_x, y: pos_y },
            scale: Scale {
                x: scale_x,
                y: scale_y,
            },
            anchor: Position {
                x: anchor_x,
                y: anchor_y,
            },
            rotation,
        }
    }

    fn evaluate_property_value(
        &self,
        properties: &PropertyMap,
        key: &str,
        time: f64,
    ) -> Option<PropertyValue> {
        let property = properties.get(key)?;
        let ctx = EvaluationContext {
            property_map: properties,
        };
        Some(self.property_evaluators.evaluate(property, time, &ctx))
    }

    fn require_string(
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

    fn optional_string(&self, properties: &PropertyMap, key: &str, time: f64) -> Option<String> {
        match self.evaluate_property_value(properties, key, time) {
            Some(PropertyValue::String(value)) => Some(value),
            _ => None,
        }
    }

    fn evaluate_number(&self, properties: &PropertyMap, key: &str, time: f64, default: f64) -> f64 {
        match self.evaluate_property_value(properties, key, time) {
            Some(PropertyValue::Number(value)) => value,
            Some(PropertyValue::Integer(value)) => value as f64,
            other => {
                warn!(
                    "Property '{}' evaluated to {:?} at time {}. Falling back to default {}.",
                    key, other, time, default
                );
                default
            }
        }
    }

    fn evaluate_vec2(
        &self,
        properties: &PropertyMap,
        key: &str,
        time: f64,
        default_x: f64,
        default_y: f64,
    ) -> (f64, f64) {
        match self.evaluate_property_value(properties, key, time) {
            Some(PropertyValue::Vec2(v)) => (v.x, v.y),
            _ => (default_x, default_y),
        }
    }

    fn evaluate_color(
        &self,
        properties: &PropertyMap,
        key: &str,
        time: f64,
        default: Color,
    ) -> Color {
        match self.evaluate_property_value(properties, key, time) {
            Some(PropertyValue::Color(c)) => c,
            _ => default,
        }
    }

    fn parse_draw_styles(&self, value: PropertyValue) -> Vec<DrawStyle> {
        match value {
            PropertyValue::Array(arr) => arr
                .into_iter()
                .filter_map(|item| {
                    let json_val: serde_json::Value = (&item).into();
                    match serde_json::from_value(json_val) {
                        Ok(style) => Some(style),
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

    fn parse_path_effects(&self, value: PropertyValue) -> Vec<PathEffect> {
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
}


// Concrete EntityConverter implementations

pub struct VideoEntityConverter;

impl EntityConverter for VideoEntityConverter {
    fn convert_entity(
        &self,
        evaluator: &FrameEvaluationContext,
        entity: &Entity,
        time: f64,
    ) -> Option<FrameObject> {
        let props = &entity.properties;
        let file_path = evaluator.require_string(props, "file_path", time, "video")?;
        let frame_number = evaluator.evaluate_number(props, "frame", time, 0.0).max(0.0) as u64;
        let transform = evaluator.build_transform(props, time);
        let effects = evaluator.build_image_effects(&entity.effects, time);
        let surface = ImageSurface {
            file_path,
            effects,
            transform,
        };

        Some(FrameObject {
            entity: FrameEntity::Video {
                surface,
                frame_number,
            },
            properties: props.clone(),
        })
    }
}

pub struct ImageEntityConverter;

impl EntityConverter for ImageEntityConverter {
    fn convert_entity(
        &self,
        evaluator: &FrameEvaluationContext,
        entity: &Entity,
        time: f64,
    ) -> Option<FrameObject> {
        let props = &entity.properties;
        let file_path = evaluator.require_string(props, "file_path", time, "image")?;
        let transform = evaluator.build_transform(props, time);
        let effects = evaluator.build_image_effects(&entity.effects, time);
        let surface = ImageSurface {
            file_path,
            effects,
            transform,
        };

        Some(FrameObject {
            entity: FrameEntity::Image { surface },
            properties: props.clone(),
        })
    }
}

pub struct TextEntityConverter;

impl EntityConverter for TextEntityConverter {
    fn convert_entity(
        &self,
        evaluator: &FrameEvaluationContext,
        entity: &Entity,
        time: f64,
    ) -> Option<FrameObject> {
        let props = &entity.properties;
        let text = evaluator.require_string(props, "text", time, "text")?;
        let font = evaluator
            .optional_string(props, "font", time)
            .unwrap_or_else(|| "Arial".to_string());
        let size = evaluator.evaluate_number(props, "size", time, 12.0);
        let color = evaluator.evaluate_color(
            props,
            "color",
            time,
            Color {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
        );
        let transform = evaluator.build_transform(props, time);
        let effects = evaluator.build_image_effects(&entity.effects, time);

        Some(FrameObject {
            entity: FrameEntity::Text {
                text,
                font,
                size,
                color,
                effects,
                transform,
            },
            properties: props.clone(),
        })
    }
}

pub struct ShapeEntityConverter;

impl EntityConverter for ShapeEntityConverter {
    fn convert_entity(
        &self,
        evaluator: &FrameEvaluationContext,
        entity: &Entity,
        time: f64,
    ) -> Option<FrameObject> {
        let props = &entity.properties;
        let path = evaluator.require_string(props, "path", time, "shape")?;
        let transform = evaluator.build_transform(props, time);

        let styles_value = evaluator
            .evaluate_property_value(props, "styles", time)
            .unwrap_or(PropertyValue::Array(vec![]));
        let styles = evaluator.parse_draw_styles(styles_value);

        let effects_value = evaluator
            .evaluate_property_value(props, "path_effects", time)
            .unwrap_or(PropertyValue::Array(vec![]));
        let path_effects = evaluator.parse_path_effects(effects_value);
        let effects = evaluator.build_image_effects(&entity.effects, time);

        Some(FrameObject {
            entity: FrameEntity::Shape {
                path,
                transform,
                styles,
                path_effects,
                effects,
            },
            properties: props.clone(),
        })
    }
}

/// Registry for EntityConverter implementations.
pub struct EntityConverterRegistry {
    converters: HashMap<String, Box<dyn EntityConverter>>,
}

impl EntityConverterRegistry {
    pub fn new() -> Self {
        Self {
            converters: HashMap::new(),
        }
    }

    pub fn register(&mut self, entity_type: String, converter: Box<dyn EntityConverter>) {
        self.converters.insert(entity_type, converter);
    }

    pub fn convert_entity(
        &self,
        evaluator: &FrameEvaluationContext,
        entity: &Entity,
        time: f64,
    ) -> Option<FrameObject> {
        match self.converters.get(&entity.entity_type) {
            Some(converter) => converter.convert_entity(evaluator, entity, time),
            None => {
                warn!("No converter registered for entity type '{}'", entity.entity_type);
                None
            }
        }
    }
}

// Function to register built-in entity converters
pub fn register_builtin_entity_converters(registry: &mut EntityConverterRegistry) {
    registry.register("video".to_string(), Box::new(VideoEntityConverter));
    registry.register("image".to_string(), Box::new(ImageEntityConverter));
    registry.register("text".to_string(), Box::new(TextEntityConverter));
    registry.register("shape".to_string(), Box::new(ShapeEntityConverter));
}