use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::HashMap;

use ordered_float::OrderedFloat;
use std::hash::{Hash, Hasher};

use crate::animation::EasingFunction;
use crate::model::frame::color::Color;

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Vec2 {
    pub x: OrderedFloat<f64>,
    pub y: OrderedFloat<f64>,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub struct Vec3 {
    pub x: OrderedFloat<f64>,
    pub y: OrderedFloat<f64>,
    pub z: OrderedFloat<f64>,
}

impl Hash for Vec2 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.x.hash(state);
        self.y.hash(state);
    }
}

impl Hash for Vec3 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.x.hash(state);
        self.y.hash(state);
        self.z.hash(state);
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(untagged)]
pub enum PropertyValue {
    Number(OrderedFloat<f64>),
    Integer(i64),
    String(String),
    Boolean(bool),
    Vec2(Vec2),
    Vec3(Vec3),
    Color(Color),
    Array(Vec<PropertyValue>),
    Map(HashMap<String, PropertyValue>),
}

impl Hash for PropertyValue {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            PropertyValue::Number(n) => n.hash(state),
            PropertyValue::Integer(i) => i.hash(state),
            PropertyValue::String(s) => s.hash(state),
            PropertyValue::Boolean(b) => b.hash(state),
            PropertyValue::Vec2(v) => v.hash(state),
            PropertyValue::Vec3(v) => v.hash(state),
            PropertyValue::Color(c) => c.hash(state),
            PropertyValue::Array(arr) => arr.hash(state),
            PropertyValue::Map(map) => {
                let mut entries: Vec<_> = map.iter().collect();
                entries.sort_by_key(|(k, _)| k.as_str()); // Deterministic order
                for (k, v) in entries {
                    k.hash(state);
                    v.hash(state);
                }
            }
        }
    }
}

impl From<f64> for PropertyValue {
    fn from(value: f64) -> Self {
        PropertyValue::Number(OrderedFloat(value))
    }
}

impl From<f32> for PropertyValue {
    fn from(value: f32) -> Self {
        PropertyValue::Number(OrderedFloat(value as f64))
    }
}

impl From<i64> for PropertyValue {
    fn from(value: i64) -> Self {
        PropertyValue::Integer(value)
    }
}

impl From<String> for PropertyValue {
    fn from(value: String) -> Self {
        PropertyValue::String(value)
    }
}

impl From<bool> for PropertyValue {
    fn from(value: bool) -> Self {
        PropertyValue::Boolean(value)
    }
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
                    PropertyValue::Number(OrderedFloat(f))
                } else {
                    PropertyValue::Number(OrderedFloat(0.0))
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
                        return PropertyValue::Vec2(Vec2 {
                            x: OrderedFloat(x_val),
                            y: OrderedFloat(y_val),
                        });
                    }
                }

                if o.len() == 3 && o.contains_key("x") && o.contains_key("y") && o.contains_key("z")
                {
                    if let (Some(x_val), Some(y_val), Some(z_val)) = (
                        o.get("x").and_then(|v| v.as_f64()),
                        o.get("y").and_then(|v| v.as_f64()),
                        o.get("z").and_then(|v| v.as_f64()),
                    ) {
                        return PropertyValue::Vec3(Vec3 {
                            x: OrderedFloat(x_val),
                            y: OrderedFloat(y_val),
                            z: OrderedFloat(z_val),
                        });
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
                        return PropertyValue::Color(Color {
                            r: r as u8,
                            g: g as u8,
                            b: b as u8,
                            a: a as u8,
                        });
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
                    serde_json::Value::Number(serde_json::Number::from(n.into_inner() as i64))
                } else {
                    serde_json::Value::Number(
                        serde_json::Number::from_f64(n.into_inner())
                            .unwrap_or_else(|| serde_json::Number::from_f64(0.0).unwrap()),
                    )
                }
            }
            PropertyValue::Integer(i) => serde_json::Value::Number(serde_json::Number::from(*i)),
            PropertyValue::String(s) => serde_json::Value::String(s.clone()),
            PropertyValue::Boolean(b) => serde_json::Value::Bool(*b),
            PropertyValue::Vec2(v) => {
                serde_json::json!({ "x": v.x.into_inner(), "y": v.y.into_inner() })
            }
            PropertyValue::Vec3(v) => {
                serde_json::json!({ "x": v.x.into_inner(), "y": v.y.into_inner(), "z": v.z.into_inner() })
            }
            PropertyValue::Color(c) => {
                serde_json::json!({ "r": c.r, "g": c.g, "b": c.b, "a": c.a })
            }
            PropertyValue::Array(arr) => {
                serde_json::Value::Array(arr.iter().map(|v| v.into()).collect())
            }
            PropertyValue::Map(map) => {
                serde_json::Value::Object(map.iter().map(|(k, v)| (k.clone(), v.into())).collect())
            }
        }
    }
}

// Define a trait for type-safe extraction from PropertyValue
pub trait TryGetProperty<T> {
    fn try_get(p: &PropertyValue) -> Option<T>;
}

// Implement for f64
impl TryGetProperty<f64> for f64 {
    fn try_get(p: &PropertyValue) -> Option<f64> {
        match p {
            PropertyValue::Number(v) => Some(v.into_inner()),
            PropertyValue::Integer(v) => Some(*v as f64),
            _ => None,
        }
    }
}

// Implement for f32
impl TryGetProperty<f32> for f32 {
    fn try_get(p: &PropertyValue) -> Option<f32> {
        match p {
            PropertyValue::Number(v) => Some(v.into_inner() as f32),
            PropertyValue::Integer(v) => Some(*v as f32),
            _ => None,
        }
    }
}

// Implement for i64
impl TryGetProperty<i64> for i64 {
    fn try_get(p: &PropertyValue) -> Option<i64> {
        match p {
            PropertyValue::Integer(v) => Some(*v),
            PropertyValue::Number(v) => {
                // Only convert if it's a whole number and fits in i64
                if v.fract().abs() < f64::EPSILON
                    && *v >= OrderedFloat(i64::MIN as f64)
                    && *v <= OrderedFloat(i64::MAX as f64)
                {
                    Some(v.into_inner() as i64)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

// Implement for String
impl TryGetProperty<String> for String {
    fn try_get(p: &PropertyValue) -> Option<String> {
        match p {
            PropertyValue::String(v) => Some(v.clone()),
            _ => None,
        }
    }
}

// Implement for bool
impl TryGetProperty<bool> for bool {
    fn try_get(p: &PropertyValue) -> Option<bool> {
        match p {
            PropertyValue::Boolean(v) => Some(*v),
            _ => None,
        }
    }
}

// Implement for Vec<PropertyValue>
impl TryGetProperty<Vec<PropertyValue>> for Vec<PropertyValue> {
    fn try_get(p: &PropertyValue) -> Option<Vec<PropertyValue>> {
        match p {
            PropertyValue::Array(v) => Some(v.clone()),
            _ => None,
        }
    }
}

// Implement for HashMap<String, PropertyValue>
impl TryGetProperty<HashMap<String, PropertyValue>> for HashMap<String, PropertyValue> {
    fn try_get(p: &PropertyValue) -> Option<HashMap<String, PropertyValue>> {
        match p {
            PropertyValue::Map(v) => Some(v.clone()),
            _ => None,
        }
    }
}

// Implement for Vec2
impl TryGetProperty<Vec2> for Vec2 {
    fn try_get(p: &PropertyValue) -> Option<Vec2> {
        match p {
            PropertyValue::Vec2(v) => Some(*v),
            _ => None,
        }
    }
}

// Implement for Vec3
impl TryGetProperty<Vec3> for Vec3 {
    fn try_get(p: &PropertyValue) -> Option<Vec3> {
        match p {
            PropertyValue::Vec3(v) => Some(*v),
            _ => None,
        }
    }
}

// Implement for Color
impl TryGetProperty<Color> for Color {
    fn try_get(p: &PropertyValue) -> Option<Color> {
        match p {
            PropertyValue::Color(v) => Some(v.clone()),
            _ => None,
        }
    }
}

impl PropertyValue {
    pub fn get_as<T: TryGetProperty<T>>(&self) -> Option<T> {
        T::try_get(self)
    }
}

#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Eq, Debug)]
pub struct Property {
    #[serde(default = "default_constant_evaluator", rename = "type")]
    pub evaluator: String,
    #[serde(default)]
    pub properties: HashMap<String, PropertyValue>,
}

impl Hash for Property {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.evaluator.hash(state);
        let mut entries: Vec<_> = self.properties.iter().collect();
        entries.sort_by_key(|(k, _)| k.as_str());
        for (k, v) in entries {
            k.hash(state);
            v.hash(state);
        }
    }
}

fn default_constant_evaluator() -> String {
    "constant".to_string()
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
pub struct Keyframe {
    pub time: OrderedFloat<f64>,
    pub value: PropertyValue,
    #[serde(default)]
    pub easing: EasingFunction, // Assuming EasingFunction implements Hash/Eq, check later
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
            properties: HashMap::from([(
                "expression".to_string(),
                PropertyValue::String(expression),
            )]),
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
        self.properties
            .get("expression")
            .and_then(|value| match value {
                PropertyValue::String(expr) => Some(expr.as_str()),
                _ => None,
            })
    }
}

#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Eq, Debug)] // Added Debug
#[serde(transparent)]
pub struct PropertyMap {
    properties: HashMap<String, Property>,
}

impl Hash for PropertyMap {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let mut entries: Vec<_> = self.properties.iter().collect();
        entries.sort_by_key(|(k, _)| k.as_str());
        for (k, v) in entries {
            k.hash(state);
            v.hash(state);
        }
    }
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

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Property)> {
        self.properties.iter()
    }

    pub fn get_constant_value(&self, key: &str) -> Option<&PropertyValue> {
        self.get(key)
            .and_then(|property| match property.evaluator.as_str() {
                "constant" => property.value(),
                _ => None,
            })
    }

    pub fn get_f64(&self, key: &str) -> Option<f64> {
        self.get_constant_value(key)
            .and_then(|pv| pv.get_as::<f64>())
    }

    pub fn get_f32(&self, key: &str) -> Option<f32> {
        self.get_constant_value(key)
            .and_then(|pv| pv.get_as::<f32>())
    }

    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.get_constant_value(key)
            .and_then(|pv| pv.get_as::<i64>())
    }

    pub fn get_string(&self, key: &str) -> Option<String> {
        self.get_constant_value(key)
            .and_then(|pv| pv.get_as::<String>())
    }
}
