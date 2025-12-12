use log::{debug, warn}; // Ensure debug is imported
use serde_json;
use std::collections::HashMap;
use std::sync::Arc;

use crate::model::frame::{
    color::Color,
    draw_type::{DrawStyle, PathEffect},
    effect::ImageEffect,
    entity::{FrameContent, FrameObject, ImageSurface},
    transform::{Position, Scale, Transform},
};
use crate::model::project::EffectConfig;
use crate::model::project::TrackClip; // Add this
use crate::model::project::project::Composition;
use crate::model::project::property::{PropertyMap, PropertyValue};
use crate::plugin::Plugin;
use crate::plugin::{EvaluationContext, PropertyEvaluatorRegistry}; // Added this line

/// Trait for converting an Entity into a FrameObject.
pub trait EntityConverter: Send + Sync {
    fn convert_entity(
        &self,
        evaluator: &FrameEvaluationContext, // Pass context instead of individual fields
        track_clip: &TrackClip,             // Changed to TrackClip
        frame_number: u64,                  // Changed to u64
    ) -> Option<FrameObject>;
}

// New trait for entity converter plugins
pub trait EntityConverterPlugin: Plugin {
    fn register_converters(&self, registry: &mut EntityConverterRegistry);
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
        let (scale_x, scale_y) = self.evaluate_vec2(props, "scale", time, 100.0, 100.0);
        let (anchor_x, anchor_y) = self.evaluate_vec2(props, "anchor", time, 0.0, 0.0);
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
        let evaluated_value = self.property_evaluators.evaluate(property, time, &ctx);
        debug!(
            "Evaluated property '{}' at time {} to {:?}",
            key, time, evaluated_value
        ); // Added debug log
        Some(evaluated_value)
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

    fn evaluate_vec2(
        &self,
        properties: &PropertyMap,
        key: &str,
        time: f64,
        default_x: f64,
        default_y: f64,
    ) -> (f64, f64) {
        // Initialize with default or Vec2 value
        let (mut vx, mut vy) = if let Some(PropertyValue::Vec2(v)) =
            self.evaluate_property_value(properties, key, time)
        {
            (*v.x, *v.y)
        } else {
            (default_x, default_y)
        };

        // Override with split keys (e.g. position_x, position_y) if they exist
        let key_x = format!("{}_x", key);
        if let Some(val) = self.evaluate_property_value(properties, &key_x, time) {
            match val {
                PropertyValue::Number(n) => vx = n.0,
                PropertyValue::Integer(i) => vx = i as f64,
                _ => {}
            }
        }

        let key_y = format!("{}_y", key);
        if let Some(val) = self.evaluate_property_value(properties, &key_y, time) {
            match val {
                PropertyValue::Number(n) => vy = n.0,
                PropertyValue::Integer(i) => vy = i as f64,
                _ => {}
            }
        }

        (vx, vy)
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
        track_clip: &TrackClip,
        frame_number: u64,
    ) -> Option<FrameObject> {
        let props = &track_clip.properties;
        let fps = evaluator.composition.fps;
        let time = frame_number as f64 / fps;

        let file_path = evaluator.require_string(props, "file_path", time, "video")?;

        let source_frame_number = frame_number.saturating_sub(track_clip.source_begin_frame);

        let transform = evaluator.build_transform(props, time);
        let effects = evaluator.build_image_effects(&track_clip.effects, time);
        let surface = ImageSurface {
            file_path,
            effects,
            transform,
        };

        Some(FrameObject {
            content: FrameContent::Video {
                surface,
                frame_number: source_frame_number,
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
        track_clip: &TrackClip,
        frame_number: u64,
    ) -> Option<FrameObject> {
        let props = &track_clip.properties;
        let fps = evaluator.composition.fps;
        let time = frame_number as f64 / fps;

        let file_path = evaluator.require_string(props, "file_path", time, "image")?;
        let transform = evaluator.build_transform(props, time);
        let effects = evaluator.build_image_effects(&track_clip.effects, time);
        let surface = ImageSurface {
            file_path,
            effects,
            transform,
        };

        Some(FrameObject {
            content: FrameContent::Image { surface },
            properties: props.clone(),
        })
    }
}

pub struct TextEntityConverter;

impl EntityConverter for TextEntityConverter {
    fn convert_entity(
        &self,
        evaluator: &FrameEvaluationContext,
        track_clip: &TrackClip,
        frame_number: u64,
    ) -> Option<FrameObject> {
        let props = &track_clip.properties;
        let fps = evaluator.composition.fps;
        let time = frame_number as f64 / fps;

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
        let effects = evaluator.build_image_effects(&track_clip.effects, time);

        Some(FrameObject {
            content: FrameContent::Text {
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
        track_clip: &TrackClip,
        frame_number: u64,
    ) -> Option<FrameObject> {
        let props = &track_clip.properties;
        let fps = evaluator.composition.fps;
        let time = frame_number as f64 / fps;

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
        let effects = evaluator.build_image_effects(&track_clip.effects, time);

        Some(FrameObject {
            content: FrameContent::Shape {
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
#[derive(Clone)] // Added derive Clone
pub struct EntityConverterRegistry {
    converters: HashMap<String, Arc<dyn EntityConverter>>, // Changed to Arc
}

impl EntityConverterRegistry {
    pub fn new() -> Self {
        Self {
            converters: HashMap::new(),
        }
    }

    pub fn register(&mut self, entity_type: String, converter: Arc<dyn EntityConverter>) {
        // Changed to Arc
        self.converters.insert(entity_type, converter);
    }

    pub fn convert_entity(
        &self,
        evaluator: &FrameEvaluationContext,
        track_clip: &TrackClip, // Changed to TrackClip
        frame_number: u64,      // Changed to u64
    ) -> Option<FrameObject> {
        let kind_str = track_clip.kind.to_string();
        match self.converters.get(&kind_str) {
            // Use track_clip.kind.to_string()
            Some(converter) => converter.convert_entity(evaluator, track_clip, frame_number),
            None => {
                warn!("No converter registered for entity type '{}'", kind_str);
                None
            }
        }
    }
}

pub struct BuiltinEntityConverterPlugin;

impl BuiltinEntityConverterPlugin {
    pub fn new() -> Self {
        Self
    }
}

impl crate::plugin::Plugin for BuiltinEntityConverterPlugin {
    fn id(&self) -> &'static str {
        "builtin_entity_converters"
    }

    fn category(&self) -> crate::plugin::PluginCategory {
        crate::plugin::PluginCategory::EntityConverter
    }

    fn version(&self) -> (u32, u32, u32) {
        (0, 1, 0)
    }
}

impl EntityConverterPlugin for BuiltinEntityConverterPlugin {
    fn register_converters(&self, registry: &mut EntityConverterRegistry) {
        register_builtin_entity_converters(registry);
    }
}

pub struct SkSLEntityConverter;

impl EntityConverter for SkSLEntityConverter {
    fn convert_entity(
        &self,
        evaluator: &FrameEvaluationContext,
        track_clip: &TrackClip,
        frame_number: u64,
    ) -> Option<FrameObject> {
        let props = &track_clip.properties;
        let fps = evaluator.composition.fps;
        let time = frame_number as f64 / fps;

        let shader = evaluator.require_string(props, "shader", time, "sksl")?;
        
        let width = evaluator.evaluate_number(props, "width", time, 1920.0);
        let height = evaluator.evaluate_number(props, "height", time, 1080.0);
        
        // Use composition size as default if not specified, 
        // but for now properties "width"/"height" aren't standard on clips unless I added them?
        // Actually, SkSL clips might not have explicit width/height properties yet.
        // Let's use composition resolution if properties are missing/zero, or hardcode typical?
        // Better: Use specific properties or default to 1920x1080.
        // Wait, creating SkSL clip didn't add width/height properties.
        // It relies on the renderer filling the screen?
        // But the renderer needs `resolution`.
        // Let's use the composition's width/height from context.
        let comp_width = evaluator.composition.width as f64;
        let comp_height = evaluator.composition.height as f64;
        
        // Check if we want overrides, otherwise use comp size
        let res_x = if width > 0.0 { width } else { comp_width };
        let res_y = if height > 0.0 { height } else { comp_height };

        let transform = evaluator.build_transform(props, time);
        let effects = evaluator.build_image_effects(&track_clip.effects, time);

        Some(FrameObject {
            content: FrameContent::SkSL {
                shader,
                resolution: (res_x as f32, res_y as f32),
                effects,
                transform,
            },
            properties: props.clone(),
        })
    }
}

// Function to register built-in entity converters
pub fn register_builtin_entity_converters(registry: &mut EntityConverterRegistry) {
    registry.register("video".to_string(), Arc::new(VideoEntityConverter));
    registry.register("image".to_string(), Arc::new(ImageEntityConverter));
    registry.register("text".to_string(), Arc::new(TextEntityConverter));
    registry.register("shape".to_string(), Arc::new(ShapeEntityConverter));
    registry.register("sksl".to_string(), Arc::new(SkSLEntityConverter));
}
