use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::animation::EasingFunction;

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum PropertyValue {
  Number(f64),
  Integer(i64),
  String(String),
  Boolean(bool),
  Vec2 { x: f64, y: f64 },
  Vec3 { x: f64, y: f64, z: f64 },
  Color { r: u8, g: u8, b: u8, a: u8 },
  Array(Vec<PropertyValue>),
  Map(HashMap<String, PropertyValue>),
}

impl From<serde_json::Value> for PropertyValue {
  fn from(value: serde_json::Value) -> Self {
    match value {
      serde_json::Value::Null => PropertyValue::String("null".to_string()),
      serde_json::Value::Bool(b) => PropertyValue::Boolean(b),
      serde_json::Value::Number(n) => {
        if let Some(i) = n.as_i64() {
          PropertyValue::Integer(i)
        } else if let Some(u) = n.as_u64() {
          PropertyValue::Integer(u as i64)
        } else if let Some(f) = n.as_f64() {
          PropertyValue::Number(f)
        } else {
          PropertyValue::Number(0.0)
        }
      }
      serde_json::Value::String(s) => PropertyValue::String(s),
      serde_json::Value::Array(a) => {
        PropertyValue::Array(a.into_iter().map(|v| v.into()).collect())
      }
      serde_json::Value::Object(o) => {
        // Try to infer specific types
        if o.len() == 2 && o.contains_key("x") && o.contains_key("y") {
          if let (Some(x_val), Some(y_val)) = (
            o.get("x").and_then(|v| v.as_f64()),
            o.get("y").and_then(|v| v.as_f64()),
          ) {
            return PropertyValue::Vec2 { x: x_val, y: y_val };
          }
        }

        if o.len() == 3 && o.contains_key("x") && o.contains_key("y") && o.contains_key("z") {
          if let (Some(x_val), Some(y_val), Some(z_val)) = (
            o.get("x").and_then(|v| v.as_f64()),
            o.get("y").and_then(|v| v.as_f64()),
            o.get("z").and_then(|v| v.as_f64()),
          ) {
            return PropertyValue::Vec3 {
              x: x_val,
              y: y_val,
              z: z_val,
            };
          }
        }

        if o.len() == 4
          && o.contains_key("r")
          && o.contains_key("g")
          && o.contains_key("b")
          && o.contains_key("a")
        {
          if let (Some(r), Some(g), Some(b), Some(a)) = (
            o.get("r").and_then(|v| v.as_u64()),
            o.get("g").and_then(|v| v.as_u64()),
            o.get("b").and_then(|v| v.as_u64()),
            o.get("a").and_then(|v| v.as_u64()),
          ) {
            return PropertyValue::Color {
              r: r as u8,
              g: g as u8,
              b: b as u8,
              a: a as u8,
            };
          }
        }

        PropertyValue::Map(o.into_iter().map(|(k, v)| (k, v.into())).collect())
      }
    }
  }
}

impl From<&PropertyValue> for serde_json::Value {
  fn from(value: &PropertyValue) -> Self {
    match value {
      PropertyValue::Number(n) => serde_json::Value::Number(
        serde_json::Number::from_f64(*n)
          .unwrap_or_else(|| serde_json::Number::from_f64(0.0).unwrap()),
      ),
      PropertyValue::Integer(i) => serde_json::Value::Number(serde_json::Number::from(*i)),
      PropertyValue::String(s) => serde_json::Value::String(s.clone()),
      PropertyValue::Boolean(b) => serde_json::Value::Bool(*b),
      PropertyValue::Vec2 { x, y } => serde_json::json!({ "x": x, "y": y }),
      PropertyValue::Vec3 { x, y, z } => serde_json::json!({ "x": x, "y": y, "z": z }),
      PropertyValue::Color { r, g, b, a } => serde_json::json!({ "r": r, "g": g, "b": b, "a": a }),
      PropertyValue::Array(arr) => serde_json::Value::Array(arr.iter().map(|v| v.into()).collect()),
      PropertyValue::Map(map) => {
        serde_json::Value::Object(map.iter().map(|(k, v)| (k.clone(), v.into())).collect())
      }
    }
  }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum Property {
  Constant { value: PropertyValue },
  Keyframe { keyframes: Vec<Keyframe> },
  Expression { expression: String },
  Dynamic { handler: PropertyHandler },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PropertyHandler {
  pub plugin_id: String,
  pub handler_id: String,
  pub config: HashMap<String, PropertyValue>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Keyframe {
  pub time: f64,
  pub value: PropertyValue,
  #[serde(default)]
  pub easing: EasingFunction,
}

impl Property {
  pub fn get_value(&self, time: f64) -> PropertyValue {
    match self {
      Property::Constant { value } => value.clone(),
      Property::Keyframe { keyframes } => {
        if keyframes.is_empty() {
          return PropertyValue::Number(0.0);
        } else if time <= keyframes[0].time {
          return keyframes[0].value.clone();
        } else if time >= keyframes.last().unwrap().time {
          return keyframes.last().unwrap().value.clone();
        }

        let keyframe = keyframes.iter().rev().find(|k| k.time <= time).unwrap();
        let next_keyframe = keyframes.iter().find(|k| k.time > time).unwrap();

        interpolate_values(
          &keyframe.value,
          &next_keyframe.value,
          (time - keyframe.time) / (next_keyframe.time - keyframe.time),
          &keyframe.easing,
        )
      }
      Property::Expression { expression: _ } => {
        todo!()
      }
      Property::Dynamic { handler: _ } => {
        todo!()
      }
    }
  }
}

fn interpolate_values(
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
        .map(|(s, e)| interpolate_values(s, e, t, easing))
        .collect(),
    ),
    (PropertyValue::Map(s), PropertyValue::Map(e)) => PropertyValue::Map(
      s.iter()
        .zip(e.iter())
        .map(|((k, sv), (_, ev))| (k.clone(), interpolate_values(sv, ev, t, easing)))
        .collect(),
    ),
    _ => start.clone(),
  }
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(transparent)]
pub struct PropertyMap {
  properties: HashMap<String, Property>,
}

impl PropertyMap {
  pub fn new() -> Self {
    Self {
      properties: HashMap::new(),
    }
  }

  pub fn get(&self, key: &str) -> Option<&Property> {
    self.properties.get(key)
  }

  pub fn set(&mut self, key: String, property: Property) {
    self.properties.insert(key, property);
  }

  pub fn get_value(&self, key: &str, time: f64) -> Option<PropertyValue> {
    self.properties.get(key).map(|prop| prop.get_value(time))
  }

  pub fn get_number(&self, key: &str, time: f64, default: f64) -> f64 {
    match self.get_value(key, time) {
      Some(PropertyValue::Number(val)) => val,
      Some(PropertyValue::Integer(val)) => val as f64,
      _ => default,
    }
  }

  pub fn get_vec2(&self, key: &str, time: f64, default_x: f64, default_y: f64) -> (f64, f64) {
    match self.get_value(key, time) {
      Some(PropertyValue::Vec2 { x, y }) => (x, y),
      _ => (default_x, default_y),
    }
  }

  pub fn get_color(
    &self,
    key: &str,
    time: f64,
    default_r: u8,
    default_g: u8,
    default_b: u8,
    default_a: u8,
  ) -> (u8, u8, u8, u8) {
    match self.get_value(key, time) {
      Some(PropertyValue::Color { r, g, b, a }) => (r, g, b, a),
      _ => (default_r, default_g, default_b, default_a),
    }
  }

  pub fn get_array(&self, key: &str, time: f64, default: Vec<PropertyValue>) -> Vec<PropertyValue> {
    match self.get_value(key, time) {
      Some(PropertyValue::Array(val)) => val,
      _ => default,
    }
  }

  pub fn get_map(
    &self,
    key: &str,
    time: f64,
    default: HashMap<String, PropertyValue>,
  ) -> HashMap<String, PropertyValue> {
    match self.get_value(key, time) {
      Some(PropertyValue::Map(val)) => val,
      _ => default,
    }
  }
}
