use crate::animation::EasingFunction;
use crate::model::entity::Entity;
use crate::model::property::{
  Keyframe as EntityKeyframe, Property as EntityProperty, PropertyValue,
};
use log::{debug, warn};
use ordered_float::OrderedFloat;
use serde::Serialize;
use std::collections::BTreeSet;

use super::{Property as TrackProperty, TrackEntity, TransformProperty};

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
        entity.set_property(
          "frame",
          create_video_frame_property(*zero, time_range.start, time_range.end),
        );
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
        entity.set_property("size", convert_numeric_property(size));
        entity.set_constant_property(
          "color",
          PropertyValue::Color {
            r: color.r,
            g: color.g,
            b: color.b,
            a: color.a,
          },
        );
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

        debug!(
          "Converting Shape entity. Styles count: {}, Path effects count: {}",
          styles.len(),
          path_effects.len()
        );

        // Convert styles to PropertyValue
        let styles_value: Vec<PropertyValue> = styles
          .iter()
          .map(|s| match serde_json::to_value(s) {
            Ok(v) => v.into(),
            Err(e) => {
              warn!("Failed to serialize style: {}", e);
              PropertyValue::String("error".to_string())
            }
          })
          .collect();
        entity.set_constant_property("styles", PropertyValue::Array(styles_value));

        // Convert path_effects to PropertyValue
        let effects_value: Vec<PropertyValue> = path_effects
          .iter()
          .map(|e| match serde_json::to_value(e) {
            Ok(v) => v.into(),
            Err(e) => {
              warn!("Failed to serialize path effect: {}", e);
              PropertyValue::String("error".to_string())
            }
          })
          .collect();
        entity.set_constant_property("path_effects", PropertyValue::Array(effects_value));

        apply_transform(&mut entity, transform);
        entity
      }
    }
  }
}

fn apply_transform(entity: &mut Entity, transform: &TransformProperty) {
  entity.set_property(
    "position",
    convert_vec2_property(&transform.position.x, &transform.position.y),
  );
  entity.set_property(
    "scale",
    convert_vec2_property(&transform.scale.x, &transform.scale.y),
  );
  entity.set_property(
    "anchor",
    convert_vec2_property(&transform.anchor.x, &transform.anchor.y),
  );
  entity.set_property("rotation", convert_numeric_property(&transform.rotation));
}

fn convert_numeric_property(property: &TrackProperty<f64>) -> EntityProperty {
  convert_property_serialized(property)
}

fn convert_property_serialized<T>(property: &TrackProperty<T>) -> EntityProperty
where
  T: Serialize + Clone,
{
  convert_property_with(property, |value| serialize_property_value(value))
}

fn convert_property_with<T, F>(property: &TrackProperty<T>, mut to_value: F) -> EntityProperty
where
  T: Clone,
  F: FnMut(&T) -> PropertyValue,
{
  match property {
    TrackProperty::Constant { value } => EntityProperty::Constant {
      value: to_value(value),
    },
    TrackProperty::Keyframe { keyframes } => EntityProperty::Keyframe {
      keyframes: keyframes
        .iter()
        .map(|kf| EntityKeyframe {
          time: kf.time,
          value: to_value(&kf.value),
          easing: EasingFunction::Linear,
        })
        .collect(),
    },
    TrackProperty::Expression { expression } => EntityProperty::Expression {
      expression: expression.clone(),
    },
  }
}

fn serialize_property_value<T: Serialize>(value: &T) -> PropertyValue {
  match serde_json::to_value(value) {
    Ok(v) => PropertyValue::from(v),
    Err(e) => {
      warn!("Failed to serialize property value: {}", e);
      PropertyValue::String("serialization_error".to_string())
    }
  }
}

fn convert_vec2_property(
  x_property: &TrackProperty<f64>,
  y_property: &TrackProperty<f64>,
) -> EntityProperty {
  match (x_property, y_property) {
    (TrackProperty::Constant { value: x }, TrackProperty::Constant { value: y }) => {
      EntityProperty::Constant {
        value: PropertyValue::Vec2 { x: *x, y: *y },
      }
    }
    _ => {
      let mut key_times: BTreeSet<OrderedFloat<f64>> = BTreeSet::new();
      collect_keyframe_times(x_property, &mut key_times);
      collect_keyframe_times(y_property, &mut key_times);

      if key_times.is_empty() {
        let x = evaluate_property(x_property, 0.0);
        let y = evaluate_property(y_property, 0.0);
        return EntityProperty::Constant {
          value: PropertyValue::Vec2 { x, y },
        };
      }

      let keyframes = key_times
        .into_iter()
        .map(|time| {
          let t = time.into_inner();
          EntityKeyframe {
            time: t,
            value: PropertyValue::Vec2 {
              x: evaluate_property(x_property, t),
              y: evaluate_property(y_property, t),
            },
            easing: EasingFunction::Linear,
          }
        })
        .collect();

      EntityProperty::Keyframe { keyframes }
    }
  }
}

fn collect_keyframe_times(property: &TrackProperty<f64>, set: &mut BTreeSet<OrderedFloat<f64>>) {
  if let TrackProperty::Keyframe { keyframes } = property {
    for kf in keyframes {
      set.insert(OrderedFloat(kf.time));
    }
  }
}

fn evaluate_property(property: &TrackProperty<f64>, time: f64) -> f64 {
  property.get_value(time)
}

fn create_video_frame_property(start_frame: f64, start_time: f64, end_time: f64) -> EntityProperty {
  if (end_time - start_time).abs() < f64::EPSILON {
    return EntityProperty::Constant {
      value: PropertyValue::Number(start_frame),
    };
  }

  let mut keyframes = Vec::new();
  keyframes.push(EntityKeyframe {
    time: start_time,
    value: PropertyValue::Number(start_frame),
    easing: EasingFunction::Linear,
  });
  keyframes.push(EntityKeyframe {
    time: end_time,
    value: PropertyValue::Number(start_frame + (end_time - start_time)),
    easing: EasingFunction::Linear,
  });

  EntityProperty::Keyframe { keyframes }
}
