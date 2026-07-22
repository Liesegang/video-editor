use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use ordered_float::OrderedFloat;
use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::model::frame::color::Color;
use crate::model::path::{FillRule, PathPoint, PathSegment, PathValue};

use super::{ColorValue, PropertyUiType};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Vec2 {
    pub x: OrderedFloat<f64>,
    pub y: OrderedFloat<f64>,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Vec3 {
    pub x: OrderedFloat<f64>,
    pub y: OrderedFloat<f64>,
    pub z: OrderedFloat<f64>,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Vec4 {
    pub x: OrderedFloat<f64>,
    pub y: OrderedFloat<f64>,
    pub z: OrderedFloat<f64>,
    pub w: OrderedFloat<f64>,
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

impl Hash for Vec4 {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.x.hash(state);
        self.y.hash(state);
        self.z.hash(state);
        self.w.hash(state);
    }
}

#[derive(Serialize, Clone, PartialEq, Eq, Debug)]
#[serde(untagged)]
pub enum PropertyValue {
    // Keep Integer before Number: both serialize as an untagged JSON number,
    // and serde tries untagged variants in declaration order. Number-first
    // changed an authored Integer(0) into Number(0.0) on Project round-trip.
    Integer(i64),
    Number(OrderedFloat<f64>),
    String(String),
    Boolean(bool),
    Vec2(Vec2),
    Vec3(Vec3),
    Vec4(Vec4),
    /// Color-space-tagged, floating-point graph color. The existing `Color`
    /// variant remains the explicit legacy straight-sRGBA8 boundary.
    ColorValue(ColorValue),
    Color(Color),
    // Tagged structured scalars stay distinct from arbitrary authored maps.
    Path(PathValue),
    Array(Vec<PropertyValue>),
    Map(HashMap<String, PropertyValue>),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum UntaggedPropertyValue {
    Integer(i64),
    Number(OrderedFloat<f64>),
    String(String),
    Boolean(bool),
    Vec2(Vec2),
    Vec3(Vec3),
    Vec4(Vec4),
    Color(Color),
    Array(Vec<PropertyValue>),
    Map(HashMap<String, PropertyValue>),
}

impl From<UntaggedPropertyValue> for PropertyValue {
    fn from(value: UntaggedPropertyValue) -> Self {
        match value {
            UntaggedPropertyValue::Integer(value) => Self::Integer(value),
            UntaggedPropertyValue::Number(value) => Self::Number(value),
            UntaggedPropertyValue::String(value) => Self::String(value),
            UntaggedPropertyValue::Boolean(value) => Self::Boolean(value),
            UntaggedPropertyValue::Vec2(value) => Self::Vec2(value),
            UntaggedPropertyValue::Vec3(value) => Self::Vec3(value),
            UntaggedPropertyValue::Vec4(value) => Self::Vec4(value),
            UntaggedPropertyValue::Color(value) => Self::Color(value),
            UntaggedPropertyValue::Array(value) => Self::Array(value),
            UntaggedPropertyValue::Map(value) => Self::Map(value),
        }
    }
}

impl<'de> Deserialize<'de> for PropertyValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        if super::color_value::is_tagged_color_value_json(&value) {
            return serde_json::from_value(value)
                .map(Self::ColorValue)
                .map_err(D::Error::custom);
        }
        if crate::model::path::is_tagged_path_value_json(&value) {
            return serde_json::from_value(value)
                .map(Self::Path)
                .map_err(D::Error::custom);
        }
        serde_json::from_value::<UntaggedPropertyValue>(value)
            .map(Self::from)
            .map_err(D::Error::custom)
    }
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
            PropertyValue::Vec4(v) => v.hash(state),
            PropertyValue::ColorValue(c) => c.hash(state),
            PropertyValue::Color(c) => c.hash(state),
            PropertyValue::Path(path) => path.hash(state),
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

impl PropertyValue {
    pub fn is_compatible_with(&self, ui_type: &PropertyUiType) -> bool {
        match self {
            PropertyValue::Number(_) => matches!(ui_type, PropertyUiType::Float { .. }),
            PropertyValue::Integer(_) => matches!(ui_type, PropertyUiType::Integer { .. }),
            PropertyValue::String(_) => matches!(
                ui_type,
                PropertyUiType::Text
                    | PropertyUiType::MultilineText
                    | PropertyUiType::Font
                    | PropertyUiType::Dropdown { .. }
            ),
            PropertyValue::Boolean(_) => matches!(ui_type, PropertyUiType::Bool),
            PropertyValue::ColorValue(_) => matches!(ui_type, PropertyUiType::ColorValue),
            PropertyValue::Color(_) => matches!(ui_type, PropertyUiType::Color),
            PropertyValue::Vec2(_) => matches!(ui_type, PropertyUiType::Vec2 { .. }),
            PropertyValue::Vec3(_) => matches!(ui_type, PropertyUiType::Vec3 { .. }),
            PropertyValue::Vec4(_) => matches!(ui_type, PropertyUiType::Vec4 { .. }),
            PropertyValue::Path(_) => matches!(ui_type, PropertyUiType::Path),
            PropertyValue::Array(_) | PropertyValue::Map(_) => false,
        }
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
                let object = serde_json::Value::Object(o.clone());
                if super::color_value::is_tagged_color_value_json(&object)
                    && let Ok(color) = serde_json::from_value(object.clone())
                {
                    return PropertyValue::ColorValue(color);
                }
                if crate::model::path::is_tagged_path_value_json(&object)
                    && let Ok(path) = serde_json::from_value(object)
                {
                    return PropertyValue::Path(path);
                }

                // Try to infer specific types
                if o.len() == 2
                    && o.contains_key("x")
                    && o.contains_key("y")
                    && let (Some(x_val), Some(y_val)) = (
                        o.get("x").and_then(|v| v.as_f64()),
                        o.get("y").and_then(|v| v.as_f64()),
                    )
                {
                    return PropertyValue::Vec2(Vec2 {
                        x: OrderedFloat(x_val),
                        y: OrderedFloat(y_val),
                    });
                }

                if o.len() == 3
                    && o.contains_key("x")
                    && o.contains_key("y")
                    && o.contains_key("z")
                    && let (Some(x_val), Some(y_val), Some(z_val)) = (
                        o.get("x").and_then(|v| v.as_f64()),
                        o.get("y").and_then(|v| v.as_f64()),
                        o.get("z").and_then(|v| v.as_f64()),
                    )
                {
                    return PropertyValue::Vec3(Vec3 {
                        x: OrderedFloat(x_val),
                        y: OrderedFloat(y_val),
                        z: OrderedFloat(z_val),
                    });
                }

                if o.len() == 4
                    && o.contains_key("x")
                    && o.contains_key("y")
                    && o.contains_key("z")
                    && o.contains_key("w")
                    && let (Some(x_val), Some(y_val), Some(z_val), Some(w_val)) = (
                        o.get("x").and_then(|v| v.as_f64()),
                        o.get("y").and_then(|v| v.as_f64()),
                        o.get("z").and_then(|v| v.as_f64()),
                        o.get("w").and_then(|v| v.as_f64()),
                    )
                {
                    return PropertyValue::Vec4(Vec4 {
                        x: OrderedFloat(x_val),
                        y: OrderedFloat(y_val),
                        z: OrderedFloat(z_val),
                        w: OrderedFloat(w_val),
                    });
                }

                if o.len() == 4
                    && o.contains_key("r")
                    && o.contains_key("g")
                    && o.contains_key("b")
                    && o.contains_key("a")
                    && let (Some(r), Some(g), Some(b), Some(a)) = (
                        o.get("r").and_then(|v| v.as_u64()),
                        o.get("g").and_then(|v| v.as_u64()),
                        o.get("b").and_then(|v| v.as_u64()),
                        o.get("a").and_then(|v| v.as_u64()),
                    )
                {
                    return PropertyValue::Color(Color {
                        r: r as u8,
                        g: g as u8,
                        b: b as u8,
                        a: a as u8,
                    });
                }

                PropertyValue::Map(o.into_iter().map(|(k, v)| (k, v.into())).collect())
            }
        }
    }
}

impl From<&PropertyValue> for serde_json::Value {
    fn from(value: &PropertyValue) -> Self {
        match value {
            // Preserve the authored numeric variant. Encoding Number(1.0) as
            // the integer JSON token `1` makes the untagged deserializer select
            // PropertyValue::Integer, which in turn disables numeric keyframe
            // interpolation. Integer has its own branch below.
            PropertyValue::Number(n) => serde_json::Value::Number(
                serde_json::Number::from_f64(n.into_inner())
                    .unwrap_or_else(|| serde_json::Number::from(0)),
            ),
            PropertyValue::Integer(i) => serde_json::Value::Number(serde_json::Number::from(*i)),
            PropertyValue::String(s) => serde_json::Value::String(s.clone()),
            PropertyValue::Boolean(b) => serde_json::Value::Bool(*b),
            PropertyValue::Vec2(v) => {
                serde_json::json!({ "x": v.x.into_inner(), "y": v.y.into_inner() })
            }
            PropertyValue::Vec3(v) => {
                serde_json::json!({ "x": v.x.into_inner(), "y": v.y.into_inner(), "z": v.z.into_inner() })
            }
            PropertyValue::Vec4(v) => {
                serde_json::json!({ "x": v.x.into_inner(), "y": v.y.into_inner(), "z": v.z.into_inner(), "w": v.w.into_inner() })
            }
            PropertyValue::ColorValue(color) => serde_json::json!({
                "$type": "color_value",
                "space": color.color_space().as_str(),
                "rgba": color.rgba(),
            }),
            PropertyValue::Color(c) => {
                serde_json::json!({ "r": c.r, "g": c.g, "b": c.b, "a": c.a })
            }
            PropertyValue::Path(path) => path_json_value(path),
            PropertyValue::Array(arr) => {
                serde_json::Value::Array(arr.iter().map(|v| v.into()).collect())
            }
            PropertyValue::Map(map) => {
                serde_json::Value::Object(map.iter().map(|(k, v)| (k.clone(), v.into())).collect())
            }
        }
    }
}

fn path_json_value(path: &PathValue) -> serde_json::Value {
    let fill_rule = match path.fill_rule() {
        FillRule::NonZero => "non_zero",
        FillRule::EvenOdd => "even_odd",
    };
    let contours = path
        .contours()
        .iter()
        .map(|contour| {
            let segments = contour
                .segments()
                .iter()
                .map(path_segment_json_value)
                .collect::<Vec<_>>();
            serde_json::json!({
                "start": path_point_json_value(contour.start()),
                "segments": segments,
                "closed": contour.is_closed(),
            })
        })
        .collect::<Vec<_>>();
    serde_json::json!({
        "$type": "path_value",
        "fill_rule": fill_rule,
        "contours": contours,
    })
}

fn path_segment_json_value(segment: &PathSegment) -> serde_json::Value {
    match segment {
        PathSegment::Line { to } => serde_json::json!({
            "kind": "line",
            "to": path_point_json_value(*to),
        }),
        PathSegment::Quadratic { control, to } => serde_json::json!({
            "kind": "quadratic",
            "control": path_point_json_value(*control),
            "to": path_point_json_value(*to),
        }),
        PathSegment::Conic {
            control,
            to,
            weight,
        } => serde_json::json!({
            "kind": "conic",
            "control": path_point_json_value(*control),
            "to": path_point_json_value(*to),
            "weight": weight.into_inner(),
        }),
        PathSegment::Cubic {
            control1,
            control2,
            to,
        } => serde_json::json!({
            "kind": "cubic",
            "control1": path_point_json_value(*control1),
            "control2": path_point_json_value(*control2),
            "to": path_point_json_value(*to),
        }),
    }
}

fn path_point_json_value(point: PathPoint) -> serde_json::Value {
    serde_json::json!({ "x": point.x(), "y": point.y() })
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

// Implement for Vec4
impl TryGetProperty<Vec4> for Vec4 {
    fn try_get(p: &PropertyValue) -> Option<Vec4> {
        match p {
            PropertyValue::Vec4(v) => Some(*v),
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

impl TryGetProperty<ColorValue> for ColorValue {
    fn try_get(p: &PropertyValue) -> Option<ColorValue> {
        match p {
            PropertyValue::ColorValue(value) => Some(value.clone()),
            _ => None,
        }
    }
}

impl TryGetProperty<PathValue> for PathValue {
    fn try_get(p: &PropertyValue) -> Option<PathValue> {
        match p {
            PropertyValue::Path(value) => Some(value.clone()),
            _ => None,
        }
    }
}

impl PropertyValue {
    pub fn get_as<T: TryGetProperty<T>>(&self) -> Option<T> {
        T::try_get(self)
    }
}

impl PropertyValue {
    pub fn interpolate(a: &PropertyValue, b: &PropertyValue, t: f64) -> PropertyValue {
        use crate::model::property::{Color, Vec2, Vec3, Vec4};

        match (a, b) {
            (PropertyValue::Number(n1), PropertyValue::Number(n2)) => {
                let v = n1.into_inner() + (n2.into_inner() - n1.into_inner()) * t;
                PropertyValue::Number(OrderedFloat(v))
            }
            (PropertyValue::Vec2(v1), PropertyValue::Vec2(v2)) => {
                let x = v1.x.into_inner() + (v2.x.into_inner() - v1.x.into_inner()) * t;
                let y = v1.y.into_inner() + (v2.y.into_inner() - v1.y.into_inner()) * t;
                PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(x),
                    y: OrderedFloat(y),
                })
            }
            (PropertyValue::Vec3(v1), PropertyValue::Vec3(v2)) => {
                let x = v1.x.into_inner() + (v2.x.into_inner() - v1.x.into_inner()) * t;
                let y = v1.y.into_inner() + (v2.y.into_inner() - v1.y.into_inner()) * t;
                let z = v1.z.into_inner() + (v2.z.into_inner() - v1.z.into_inner()) * t;
                PropertyValue::Vec3(Vec3 {
                    x: OrderedFloat(x),
                    y: OrderedFloat(y),
                    z: OrderedFloat(z),
                })
            }
            (PropertyValue::Vec4(v1), PropertyValue::Vec4(v2)) => {
                let x = v1.x.into_inner() + (v2.x.into_inner() - v1.x.into_inner()) * t;
                let y = v1.y.into_inner() + (v2.y.into_inner() - v1.y.into_inner()) * t;
                let z = v1.z.into_inner() + (v2.z.into_inner() - v1.z.into_inner()) * t;
                let w = v1.w.into_inner() + (v2.w.into_inner() - v1.w.into_inner()) * t;
                PropertyValue::Vec4(Vec4 {
                    x: OrderedFloat(x),
                    y: OrderedFloat(y),
                    z: OrderedFloat(z),
                    w: OrderedFloat(w),
                })
            }
            (PropertyValue::Color(c1), PropertyValue::Color(c2)) => {
                // Simple linear RGB blend
                let r = (c1.r as f64 + (c2.r as f64 - c1.r as f64) * t).round() as u8;
                let g = (c1.g as f64 + (c2.g as f64 - c1.g as f64) * t).round() as u8;
                let b = (c1.b as f64 + (c2.b as f64 - c1.b as f64) * t).round() as u8;
                let a = (c1.a as f64 + (c2.a as f64 - c1.a as f64) * t).round() as u8;
                PropertyValue::Color(Color { r, g, b, a })
            }
            // Fallback for non-interpolatable types (Boolean, String, Integer, Heterogeneous) -> Step
            _ => {
                if t < 1.0 {
                    a.clone()
                } else {
                    b.clone()
                }
            }
        }
    }
}
