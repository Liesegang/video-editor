use std::collections::HashMap;
use std::sync::Arc;

use log::{debug, warn};
use serde_json;

use crate::model::frame::{
    color::Color,
    draw_type::{DrawStyle, PathEffect},
    effect::ImageEffect,
    entity::{FrameEntity, FrameObject, ImageSurface},
    frame::FrameInfo,
    transform::{Position, Scale, Transform},
};
use crate::model::project::entity::{EffectConfig, Entity};
use crate::model::project::project::{Composition, Project};
use crate::model::project::property::{PropertyMap, PropertyValue};
use crate::util::timing::ScopedTimer;

use super::property::{EvaluationContext, PropertyEvaluatorRegistry};

pub struct FrameEvaluator<'a> {
    composition: &'a Composition,
    property_evaluators: Arc<PropertyEvaluatorRegistry>,
}

impl<'a> FrameEvaluator<'a> {
    pub fn new(
        composition: &'a Composition,
        property_evaluators: Arc<PropertyEvaluatorRegistry>,
    ) -> Self {
        Self {
            composition,
            property_evaluators,
        }
    }

    pub fn evaluate(&self, time: f64) -> FrameInfo {
        let mut frame = self.initialize_frame();
        for entity in self.active_entities(time) {
            if let Some(object) = self.convert_entity(entity, time) {
                frame.objects.push(object);
            }
        }
        frame
    }

    fn initialize_frame(&self) -> FrameInfo {
        FrameInfo {
            width: self.composition.width,
            height: self.composition.height,
            background_color: self.composition.background_color.clone(),
            color_profile: self.composition.color_profile.clone(),
            objects: Vec::new(),
        }
    }

    fn active_entities(&self, time: f64) -> impl Iterator<Item = &Entity> {
        self.composition
            .cached_entities()
            .iter()
            .filter(move |entity| entity.start_time <= time && entity.end_time >= time)
    }

    fn convert_entity(&self, entity: &Entity, time: f64) -> Option<FrameObject> {
        match entity.entity_type.as_str() {
            "video" => self.build_video(entity, time),
            "image" => self.build_image(entity, time),
            "text" => self.build_text(entity, time),
            "shape" => self.build_shape(entity, time),
            other => {
                warn!("Entity type '{}' is not supported; skipping", other);
                None
            }
        }
    }

    fn build_video(&self, entity: &Entity, time: f64) -> Option<FrameObject> {
        let props = &entity.properties;
        let file_path = self.require_string(props, "file_path", time, "video")?;
        let frame_number = self.evaluate_number(props, "frame", time, 0.0).max(0.0) as u64;
        let transform = self.build_transform(props, time);
        let effects = self.build_image_effects(&entity.effects, time);
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

    fn build_image(&self, entity: &Entity, time: f64) -> Option<FrameObject> {
        let props = &entity.properties;
        let file_path = self.require_string(props, "file_path", time, "image")?;
        let transform = self.build_transform(props, time);
        let effects = self.build_image_effects(&entity.effects, time);
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

    fn build_text(&self, entity: &Entity, time: f64) -> Option<FrameObject> {
        let props = &entity.properties;
        let text = self.require_string(props, "text", time, "text")?;
        let font = self
            .optional_string(props, "font", time)
            .unwrap_or_else(|| "Arial".to_string());
        let size = self.evaluate_number(props, "size", time, 12.0);
        let color = self.evaluate_color(
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
        let transform = self.build_transform(props, time);
        let effects = self.build_image_effects(&entity.effects, time);

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

    fn build_shape(&self, entity: &Entity, time: f64) -> Option<FrameObject> {
        let props = &entity.properties;
        let path = self.require_string(props, "path", time, "shape")?;
        let transform = self.build_transform(props, time);

        let styles_value = self
            .evaluate_property_value(props, "styles", time)
            .unwrap_or(PropertyValue::Array(vec![]));
        let styles = self.parse_draw_styles(styles_value);

        let effects_value = self
            .evaluate_property_value(props, "path_effects", time)
            .unwrap_or(PropertyValue::Array(vec![]));
        let path_effects = self.parse_path_effects(effects_value);
        let effects = self.build_image_effects(&entity.effects, time);

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

pub fn evaluate_composition_frame(
    composition: &Composition,
    time: f64,
    property_evaluators: &Arc<PropertyEvaluatorRegistry>,
) -> FrameInfo {
    FrameEvaluator::new(composition, Arc::clone(property_evaluators)).evaluate(time)
}

pub fn get_frame_from_project(
    project: &Project,
    composition_index: usize,
    frame_index: f64,
    property_evaluators: &Arc<PropertyEvaluatorRegistry>,
) -> FrameInfo {
    let _timer = ScopedTimer::debug(format!(
        "Frame assembly comp={} frame={}",
        composition_index, frame_index
    ));

    let composition = &project.compositions[composition_index];
    let frame = evaluate_composition_frame(composition, frame_index, property_evaluators);

    debug!(
        "Frame {} summary: objects={}",
        frame_index,
        frame.objects.len()
    );
    frame
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framing::property::register_builtin_evaluators;
    use crate::model::project::property::{Property, PropertyValue, Vec2};
    use crate::model::frame::color::Color;
    use crate::model::project::{Track, TrackEntity};
    use std::sync::Arc;

    fn make_vec2(x: f64, y: f64) -> PropertyValue {
        PropertyValue::Vec2(Vec2 { x, y })
    }

    fn constant(value: PropertyValue) -> Property {
        Property::constant(value)
    }

    #[test]
    fn frame_evaluator_builds_text_object() {
        let mut composition = Composition::new("comp", 1920, 1080, 30.0, 10.0);

        let mut text_props = PropertyMap::new();
        text_props.set(
            "text".into(),
            constant(PropertyValue::String("Hello".into())),
        );
        text_props.set(
            "font".into(),
            constant(PropertyValue::String("Roboto".into())),
        );
        text_props.set("size".into(), constant(PropertyValue::Number(48.0)));
        text_props.set(
            "color".into(),
            constant(PropertyValue::Color(Color {
                r: 255,
                g: 255,
                b: 0,
                a: 255,
            })),
        );
        text_props.set("position".into(), constant(make_vec2(10.0, 20.0)));
        text_props.set("scale".into(), constant(make_vec2(1.0, 1.0)));
        text_props.set("anchor".into(), constant(make_vec2(0.0, 0.0)));
        text_props.set("rotation".into(), constant(PropertyValue::Number(0.0)));

        let track_entity = TrackEntity {
            entity_type: "text".into(),
            start_time: 0.0,
            end_time: 5.0,
            fps: 30.0,
            properties: text_props,
            effects: Vec::new(),
        };
        let track = Track {
            name: "track".into(),
            entities: vec![track_entity],
        };
        composition.add_track(track);

        let mut registry = PropertyEvaluatorRegistry::new();
        register_builtin_evaluators(&mut registry);
        let evaluator = FrameEvaluator::new(&composition, Arc::new(registry));
        let frame = evaluator.evaluate(1.0);

        assert_eq!(frame.objects.len(), 1);
        match &frame.objects[0].entity {
            FrameEntity::Text {
                text, font, size, ..
            } => {
                assert_eq!(text, "Hello");
                assert_eq!(font, "Roboto");
                assert!((*size - 48.0).abs() < f64::EPSILON);
            }
            other => panic!("Expected text entity, got {:?}", other),
        }
    }

    #[test]
    fn frame_evaluator_filters_inactive_entities() {
        let mut composition = Composition::new("comp", 1920, 1080, 30.0, 10.0);

        let mut props = PropertyMap::new();
        props.set(
            "file_path".into(),
            constant(PropertyValue::String("foo.png".into())),
        );
        props.set("position".into(), constant(make_vec2(0.0, 0.0)));
        props.set("scale".into(), constant(make_vec2(1.0, 1.0)));
        props.set("anchor".into(), constant(make_vec2(0.0, 0.0)));
        props.set("rotation".into(), constant(PropertyValue::Number(0.0)));

        let early = TrackEntity {
            entity_type: "image".into(),
            start_time: 0.0,
            end_time: 1.0,
            fps: 30.0,
            properties: props.clone(),
            effects: Vec::new(),
        };

        let late = TrackEntity {
            entity_type: "image".into(),
            start_time: 5.0,
            end_time: 6.0,
            fps: 30.0,
            properties: props,
            effects: Vec::new(),
        };

        let track = Track {
            name: "track".into(),
            entities: vec![early, late],
        };
        composition.add_track(track);

        let mut registry = PropertyEvaluatorRegistry::new();
        register_builtin_evaluators(&mut registry);
        let evaluator = FrameEvaluator::new(&composition, Arc::new(registry));

        let frame = evaluator.evaluate(0.5);
        assert_eq!(frame.objects.len(), 1, "Only early entity should render");

        let frame_late = evaluator.evaluate(5.5);
        assert_eq!(
            frame_late.objects.len(),
            1,
            "Only late entity should render"
        );
    }
}
