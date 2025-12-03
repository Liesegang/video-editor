use crate::model::entity::Entity;
use crate::model::property::PropertyValue;
use log::{debug, warn};

use super::{TrackEntity, TransformProperty};

impl From<&TrackEntity> for Entity {
    fn from(track_entity: &TrackEntity) -> Self {
        match track_entity {
            TrackEntity::Video {
                file_path,
                zero,
                time_range,
                transform,
            } => {
                let mut entity = Entity::new("video");
                entity.start_time = time_range.start;
                entity.end_time = time_range.end;
                entity.set_constant_property("file_path", PropertyValue::String(file_path.clone()));
                entity.set_constant_property("frame", PropertyValue::Number(*zero));
                apply_transform(&mut entity, transform);
                entity
            }
            TrackEntity::Image {
                file_path,
                time_range,
                transform,
            } => {
                let mut entity = Entity::new("image");
                entity.start_time = time_range.start;
                entity.end_time = time_range.end;
                entity.set_constant_property("file_path", PropertyValue::String(file_path.clone()));
                apply_transform(&mut entity, transform);
                entity
            }
            TrackEntity::Text {
                text,
                font,
                size,
                color,
                time_range,
                transform,
            } => {
                let mut entity = Entity::new("text");
                entity.start_time = time_range.start;
                entity.end_time = time_range.end;
                entity.set_constant_property("text", PropertyValue::String(text.clone()));
                entity.set_constant_property("font", PropertyValue::String(font.clone()));
                // size is Property<f64>, need to convert
                if let super::Property::Constant { value } = size {
                    entity.set_constant_property("size", PropertyValue::Number(*value));
                }
                entity.set_constant_property("color", PropertyValue::Color { r: color.r, g: color.g, b: color.b, a: color.a });
                apply_transform(&mut entity, transform);
                entity
            }
            TrackEntity::Shape {
                path,
                styles,
                path_effects,
                time_range,
                transform,
            } => {
                let mut entity = Entity::new("shape");
                entity.start_time = time_range.start;
                entity.end_time = time_range.end;
                entity.set_constant_property("path", PropertyValue::String(path.clone()));
                
                debug!("Converting Shape entity. Styles count: {}, Path effects count: {}", styles.len(), path_effects.len());

                // Convert styles to PropertyValue
                let styles_value: Vec<PropertyValue> = styles.iter().map(|s| {
                     match serde_json::to_value(s) {
                         Ok(v) => v.into(),
                         Err(e) => {
                             warn!("Failed to serialize style: {}", e);
                             PropertyValue::String("error".to_string())
                         }
                     }
                }).collect();
                entity.set_constant_property("styles", PropertyValue::Array(styles_value));

                // Convert path_effects to PropertyValue
                let effects_value: Vec<PropertyValue> = path_effects.iter().map(|e| {
                     match serde_json::to_value(e) {
                         Ok(v) => v.into(),
                         Err(e) => {
                             warn!("Failed to serialize path effect: {}", e);
                             PropertyValue::String("error".to_string())
                         }
                     }
                }).collect();
                entity.set_constant_property("path_effects", PropertyValue::Array(effects_value));

                apply_transform(&mut entity, transform);
                entity
            }
        }
    }
}

fn apply_transform(entity: &mut Entity, transform: &TransformProperty) {
    if let super::Property::Constant { value: x } = &transform.position.x {
         if let super::Property::Constant { value: y } = &transform.position.y {
             entity.set_constant_property("position", PropertyValue::Vec2 { x: *x, y: *y });
         }
    }

    if let super::Property::Constant { value: x } = &transform.scale.x {
         if let super::Property::Constant { value: y } = &transform.scale.y {
             entity.set_constant_property("scale", PropertyValue::Vec2 { x: *x, y: *y });
         }
    }

    if let super::Property::Constant { value: x } = &transform.anchor.x {
         if let super::Property::Constant { value: y } = &transform.anchor.y {
             entity.set_constant_property("anchor", PropertyValue::Vec2 { x: *x, y: *y });
         }
    }

    if let super::Property::Constant { value } = &transform.rotation {
        entity.set_constant_property("rotation", PropertyValue::Number(*value));
    }
}
