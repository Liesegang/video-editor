use crate::animation::EasingFunction;
use crate::model::frame::{
  color::Color,
  draw_type::{DrawStyle, PathEffect},
  entity::{FrameEntity, FrameObject},
  frame::FrameInfo,
  transform::{Position, Scale, Transform},
};
use crate::model::project::entity::Entity;
use crate::model::project::project::{Composition, Project};
use crate::model::project::property::{Property, PropertyMap, PropertyValue};
use crate::util::timing::ScopedTimer;
use log::{debug, warn};
use serde_json;
use std::collections::HashMap;
use std::sync::Arc;

pub struct PropertyEvaluatorRegistry {
  evaluators: HashMap<&'static str, Box<dyn PropertyEvaluator>>,
}

impl PropertyEvaluatorRegistry {
  pub fn default() -> Self {
    let mut registry = Self {
      evaluators: HashMap::new(),
    };
    registry.register("constant", Box::new(ConstantEvaluator));
    registry.register("keyframe", Box::new(KeyframeEvaluator));
    registry.register("expression", Box::new(ExpressionEvaluator));
    registry
  }

  pub fn register(&mut self, key: &'static str, evaluator: Box<dyn PropertyEvaluator>) {
    self.evaluators.insert(key, evaluator);
  }

  pub fn evaluate(&self, property: &Property, time: f64, ctx: &EvaluationContext) -> PropertyValue {
    let key = property.evaluator.as_str();
    match self.evaluators.get(key) {
      Some(evaluator) => evaluator.evaluate(property, time, ctx),
      None => {
        warn!("Unknown property evaluator '{}'", key);
        PropertyValue::Number(0.0)
      }
    }
  }
}

pub trait PropertyEvaluator: Send + Sync {
  fn evaluate(&self, property: &Property, time: f64, ctx: &EvaluationContext) -> PropertyValue;
}

struct ConstantEvaluator;
struct KeyframeEvaluator;
struct ExpressionEvaluator;
impl PropertyEvaluator for ConstantEvaluator {
  fn evaluate(&self, property: &Property, _time: f64, _ctx: &EvaluationContext) -> PropertyValue {
    property.value().cloned().unwrap_or_else(|| {
      warn!("Constant evaluator missing 'value'; using 0");
      PropertyValue::Number(0.0)
    })
  }
}

impl PropertyEvaluator for KeyframeEvaluator {
  fn evaluate(&self, property: &Property, time: f64, _ctx: &EvaluationContext) -> PropertyValue {
    evaluate_keyframes(property, time)
  }
}

impl PropertyEvaluator for ExpressionEvaluator {
  fn evaluate(&self, property: &Property, time: f64, _ctx: &EvaluationContext) -> PropertyValue {
    warn!(
      "Expression evaluator not implemented for property '{}' at time {}",
      property.evaluator, time
    );
    PropertyValue::Number(0.0)
  }
}

pub struct EvaluationContext<'a> {
  pub property_map: &'a PropertyMap,
}

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
    self
      .composition
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

    Some(FrameObject {
      entity: FrameEntity::Video {
        file_path,
        frame_number,
        transform,
      },
      properties: props.clone(),
    })
  }

  fn build_image(&self, entity: &Entity, time: f64) -> Option<FrameObject> {
    let props = &entity.properties;
    let file_path = self.require_string(props, "file_path", time, "image")?;
    let transform = self.build_transform(props, time);

    Some(FrameObject {
      entity: FrameEntity::Image {
        file_path,
        transform,
      },
      properties: props.clone(),
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

    Some(FrameObject {
      entity: FrameEntity::Text {
        text,
        font,
        size,
        color,
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

    Some(FrameObject {
      entity: FrameEntity::Shape {
        path,
        transform,
        styles,
        path_effects,
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
      Some(PropertyValue::Vec2 { x, y }) => (x, y),
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
      Some(PropertyValue::Color { r, g, b, a }) => Color { r, g, b, a },
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

fn evaluate_keyframes(property: &Property, time: f64) -> PropertyValue {
  let keyframes = property.keyframes();
  if keyframes.is_empty() {
    return PropertyValue::Number(0.0);
  }
  if time <= keyframes[0].time {
    return keyframes[0].value.clone();
  }
  if time >= keyframes.last().unwrap().time {
    return keyframes.last().unwrap().value.clone();
  }

  let current = keyframes.iter().rev().find(|k| k.time <= time).unwrap();
  let next = keyframes.iter().find(|k| k.time > time).unwrap();
  let t = (time - current.time) / (next.time - current.time);
  interpolate_property_values(&current.value, &next.value, t, &current.easing)
}

fn interpolate_property_values(
  start: &PropertyValue,
  end: &PropertyValue,
  t: f64,
  easing: &EasingFunction,
) -> PropertyValue {
  let t = easing.apply(t);

  match (start, end) {
    (PropertyValue::Number(s), PropertyValue::Number(e)) => PropertyValue::Number(s + (e - s) * t),
    (PropertyValue::Integer(s), PropertyValue::Integer(e)) => {
      PropertyValue::Number(*s as f64 + (*e as f64 - *s as f64) * t)
    }
    (PropertyValue::Vec2 { x: sx, y: sy }, PropertyValue::Vec2 { x: ex, y: ey }) => {
      PropertyValue::Vec2 {
        x: sx + (ex - sx) * t,
        y: sy + (ey - sy) * t,
      }
    }
    (
      PropertyValue::Vec3 {
        x: sx,
        y: sy,
        z: sz,
      },
      PropertyValue::Vec3 {
        x: ex,
        y: ey,
        z: ez,
      },
    ) => PropertyValue::Vec3 {
      x: sx + (ex - sx) * t,
      y: sy + (ey - sy) * t,
      z: sz + (ez - sz) * t,
    },
    (
      PropertyValue::Color {
        r: sr,
        g: sg,
        b: sb,
        a: sa,
      },
      PropertyValue::Color {
        r: er,
        g: eg,
        b: eb,
        a: ea,
      },
    ) => PropertyValue::Color {
      r: (sr + ((er - sr) as f64 * t) as u8),
      g: (sg + ((eg - sg) as f64 * t) as u8),
      b: (sb + ((eb - sb) as f64 * t) as u8),
      a: (sa + ((ea - sa) as f64 * t) as u8),
    },
    (PropertyValue::Array(s), PropertyValue::Array(e)) => PropertyValue::Array(
      s.iter()
        .zip(e.iter())
        .map(|(start, end)| interpolate_property_values(start, end, t, easing))
        .collect(),
    ),
    (PropertyValue::Map(s), PropertyValue::Map(e)) => PropertyValue::Map(
      s.iter()
        .zip(e.iter())
        .map(|((k, sv), (_, ev))| (k.clone(), interpolate_property_values(sv, ev, t, easing)))
        .collect(),
    ),
    _ => start.clone(),
  }
}
