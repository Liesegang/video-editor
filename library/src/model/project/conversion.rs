use crate::animation::EasingFunction;
use crate::model::project::entity::Entity;
use crate::model::project::property::{
  Keyframe as EntityKeyframe, Property as EntityProperty, PropertyValue,
};
use log::{debug, warn};
use ordered_float::OrderedFloat;
use serde::Serialize;
use std::collections::BTreeSet;

use super::{Property as TrackProperty, TrackEntity, TransformProperty};

fn normalize_fps(fps: f64) -> f64 {
  if fps > f64::EPSILON {
    fps
  } else {
    1.0
  }
}

impl From<&TrackEntity> for Entity {
  fn from(track_entity: &TrackEntity) -> Self {
    match track_entity {
      TrackEntity::Video {
        file_path,
        zero,
        time_range,
        transform,
      } => {
        let fps = normalize_fps(time_range.fps);
        let start_time = time_range.start / fps;
        let end_time = time_range.end / fps;

        let mut entity = Entity::new("video");
        entity.start_time = start_time;
        entity.end_time = end_time;
        entity.set_constant_property("file_path", PropertyValue::String(file_path.clone()));
        entity.set_property(
          "frame",
          create_video_frame_property(*zero, start_time, end_time, fps),
        );
        apply_transform(&mut entity, transform, fps);
        entity
      }
      TrackEntity::Image {
        file_path,
        time_range,
        transform,
      } => {
        let fps = normalize_fps(time_range.fps);
        let start_time = time_range.start / fps;
        let end_time = time_range.end / fps;

        let mut entity = Entity::new("image");
        entity.start_time = start_time;
        entity.end_time = end_time;
        entity.set_constant_property("file_path", PropertyValue::String(file_path.clone()));
        apply_transform(&mut entity, transform, fps);
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
        let fps = normalize_fps(time_range.fps);
        let start_time = time_range.start / fps;
        let end_time = time_range.end / fps;

        let mut entity = Entity::new("text");
        entity.start_time = start_time;
        entity.end_time = end_time;
        entity.set_constant_property("text", PropertyValue::String(text.clone()));
        entity.set_constant_property("font", PropertyValue::String(font.clone()));
        entity.set_property("size", convert_numeric_property(size, fps));
        entity.set_constant_property(
          "color",
          PropertyValue::Color {
            r: color.r,
            g: color.g,
            b: color.b,
            a: color.a,
          },
        );
        apply_transform(&mut entity, transform, fps);
        entity
      }
      TrackEntity::Shape {
        path,
        styles,
        path_effects,
        time_range,
        transform,
      } => {
        let fps = normalize_fps(time_range.fps);
        let start_time = time_range.start / fps;
        let end_time = time_range.end / fps;

        let mut entity = Entity::new("shape");
        entity.start_time = start_time;
        entity.end_time = end_time;
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

        apply_transform(&mut entity, transform, fps);
        entity
      }
    }
  }
}

fn apply_transform(entity: &mut Entity, transform: &TransformProperty, fps: f64) {
  entity.set_property(
    "position",
    convert_vec2_property(&transform.position.x, &transform.position.y, fps),
  );
  entity.set_property(
    "scale",
    convert_vec2_property(&transform.scale.x, &transform.scale.y, fps),
  );
  entity.set_property(
    "anchor",
    convert_vec2_property(&transform.anchor.x, &transform.anchor.y, fps),
  );
  entity.set_property("rotation", convert_numeric_property(&transform.rotation, fps));
}

fn convert_numeric_property(property: &TrackProperty<f64>, fps: f64) -> EntityProperty {
  convert_property_serialized(property, fps)
}

fn convert_property_serialized<T>(property: &TrackProperty<T>, fps: f64) -> EntityProperty
where
  T: Serialize + Clone,
{
  convert_property_with(property, fps, |value| serialize_property_value(value))
}

fn convert_property_with<T, F>(
  property: &TrackProperty<T>,
  fps: f64,
  mut to_value: F,
) -> EntityProperty
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
          time: kf.time / fps,
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
  fps: f64,
) -> EntityProperty {
  match (x_property, y_property) {
    (TrackProperty::Constant { value: x }, TrackProperty::Constant { value: y }) => {
      EntityProperty::Constant {
        value: PropertyValue::Vec2 { x: *x, y: *y },
      }
    }
    _ => {
      let mut key_times: BTreeSet<OrderedFloat<f64>> = BTreeSet::new();
      collect_keyframe_times(x_property, fps, &mut key_times);
      collect_keyframe_times(y_property, fps, &mut key_times);

      if key_times.is_empty() {
        let x = evaluate_property_seconds(x_property, 0.0, fps);
        let y = evaluate_property_seconds(y_property, 0.0, fps);
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
              x: evaluate_property_seconds(x_property, t, fps),
              y: evaluate_property_seconds(y_property, t, fps),
            },
            easing: EasingFunction::Linear,
          }
        })
        .collect();

      EntityProperty::Keyframe { keyframes }
    }
  }
}

fn collect_keyframe_times(
  property: &TrackProperty<f64>,
  fps: f64,
  set: &mut BTreeSet<OrderedFloat<f64>>,
) {
  if let TrackProperty::Keyframe { keyframes } = property {
    for kf in keyframes {
      set.insert(OrderedFloat(kf.time / fps));
    }
  }
}

fn evaluate_property_seconds(property: &TrackProperty<f64>, time_seconds: f64, fps: f64) -> f64 {
  property.get_value(time_seconds * fps)
}

fn create_video_frame_property(
  start_frame: f64,
  start_time: f64,
  end_time: f64,
  fps: f64,
) -> EntityProperty {
  if (end_time - start_time).abs() < f64::EPSILON {
    return EntityProperty::Constant {
      value: PropertyValue::Number(start_frame),
    };
  }

  let mut keyframes = Vec::new();
  let frame_span = (end_time - start_time) * fps.max(0.0);
  keyframes.push(EntityKeyframe {
    time: start_time,
    value: PropertyValue::Number(start_frame),
    easing: EasingFunction::Linear,
  });
  keyframes.push(EntityKeyframe {
    time: end_time,
    value: PropertyValue::Number(start_frame + frame_span),
    easing: EasingFunction::Linear,
  });

  EntityProperty::Keyframe { keyframes }
}
