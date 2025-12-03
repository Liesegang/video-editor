use serde::{Deserialize, Serialize};
use serde_json;
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
      PropertyValue::Number(n) => {
        if n.fract().abs() < f64::EPSILON && n.abs() <= (i64::MAX as f64) {
          serde_json::Value::Number(serde_json::Number::from(*n as i64))
        } else {
          serde_json::Value::Number(
            serde_json::Number::from_f64(*n)
              .unwrap_or_else(|| serde_json::Number::from_f64(0.0).unwrap()),
          )
        }
      }
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

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Property {
  #[serde(default = "default_constant_evaluator", rename = "type")]
  pub evaluator: String,
  #[serde(default)]
  pub properties: HashMap<String, PropertyValue>,
}

fn default_constant_evaluator() -> String {
  "constant".to_string()
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Keyframe {
  pub time: f64,
  pub value: PropertyValue,
  #[serde(default)]
  pub easing: EasingFunction,
}

impl Property {
  pub fn constant(value: PropertyValue) -> Self {
    Self {
      evaluator: "constant".to_string(),
      properties: HashMap::from([("value".to_string(), value)]),
      ..Default::default()
    }
  }

  pub fn keyframe(keyframes: Vec<Keyframe>) -> Self {
    let list = keyframes
      .into_iter()
      .filter_map(|kf| serde_json::to_value(kf).ok())
      .map(PropertyValue::from)
      .collect();
    Self {
      evaluator: "keyframe".to_string(),
      properties: HashMap::from([("keyframes".to_string(), PropertyValue::Array(list))]),
      ..Default::default()
    }
  }

  pub fn expression(expression: String) -> Self {
    Self {
      evaluator: "expression".to_string(),
      properties: HashMap::from([("expression".to_string(), PropertyValue::String(expression))]),
      ..Default::default()
    }
  }

  pub fn keyframes(&self) -> Vec<Keyframe> {
    match self.properties.get("keyframes") {
      Some(PropertyValue::Array(items)) => items
        .iter()
        .filter_map(|item| serde_json::from_value(serde_json::Value::from(item)).ok())
        .collect(),
      _ => Vec::new(),
    }
  }

  pub fn value(&self) -> Option<&PropertyValue> {
    self.properties.get("value")
  }

  pub fn expression_text(&self) -> Option<&str> {
    self
      .properties
      .get("expression")
      .and_then(|value| match value {
        PropertyValue::String(expr) => Some(expr.as_str()),
        _ => None,
      })
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

  pub fn get_constant_value(&self, key: &str) -> Option<&PropertyValue> {
    self
      .get(key)
      .and_then(|property| match property.evaluator.as_str() {
        "constant" => property.value(),
        _ => None,
      })
  }

  pub fn get_constant_number(&self, key: &str, default: f64) -> f64 {
    match self.get_constant_value(key) {
      Some(PropertyValue::Number(val)) => *val,
      Some(PropertyValue::Integer(val)) => *val as f64,
      _ => default,
    }
  }
}
