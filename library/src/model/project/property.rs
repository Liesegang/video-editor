use log;
use serde::{Deserialize, Serialize};
use serde_json;
use std::collections::{HashMap, HashSet};

use ordered_float::OrderedFloat;
use std::hash::{Hash, Hasher};
use uuid::Uuid;

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

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
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

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
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
            PropertyValue::Vec4(v) => v.hash(state),
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
            PropertyValue::Color(_) => matches!(ui_type, PropertyUiType::Color),
            PropertyValue::Vec2(_) => matches!(ui_type, PropertyUiType::Vec2 { .. }),
            PropertyValue::Vec3(_) => matches!(ui_type, PropertyUiType::Vec3 { .. }),
            PropertyValue::Vec4(_) => matches!(ui_type, PropertyUiType::Vec4 { .. }),
            PropertyValue::Array(_) => false,
            _ => false,
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

/// Persistent identity for one authored keyframe.
///
/// Time and sorted position are editable presentation data, so neither is a
/// safe identity while a drag crosses neighbouring keys.  This ID is stored in
/// the authoritative Project model and survives sorting and save/load.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[serde(transparent)]
pub struct KeyframeId(Uuid);

impl KeyframeId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub const fn from_uuid(id: Uuid) -> Self {
        Self(id)
    }

    pub const fn as_uuid(&self) -> Uuid {
        self.0
    }
}

impl Default for KeyframeId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for KeyframeId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Atomic changes applied to one persistently identified keyframe.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeyframeUpdate {
    pub time: Option<f64>,
    pub value: Option<PropertyValue>,
    pub easing: Option<EasingFunction>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug, Hash)]
pub struct Keyframe {
    pub id: KeyframeId,
    pub time: OrderedFloat<f64>,
    pub value: PropertyValue,
    #[serde(default)]
    pub easing: EasingFunction, // Assuming EasingFunction implements Hash/Eq, check later
}

impl Keyframe {
    pub fn new(time: f64, value: PropertyValue, easing: EasingFunction) -> Self {
        Self {
            id: KeyframeId::new(),
            time: OrderedFloat(time),
            value,
            easing,
        }
    }
}

impl Property {
    pub fn constant(value: PropertyValue) -> Self {
        Self {
            evaluator: "constant".to_string(),
            properties: HashMap::from([("value".to_string(), value)]),
        }
    }

    pub fn keyframe(mut keyframes: Vec<Keyframe>) -> Self {
        keyframes.sort_by_key(|keyframe| keyframe.time);
        let list = keyframes
            .iter()
            .filter_map(|kf| serde_json::to_value(kf).ok())
            .map(PropertyValue::from)
            .collect();

        let mut properties = HashMap::from([("keyframes".to_string(), PropertyValue::Array(list))]);

        // Store the first keyframe's value as a fallback "value" property
        // This ensures evaluators can return a value of the correct type (e.g., Vec2)
        // even if the keyframe list is empty/invalid during evaluation.
        if let Some(first) = keyframes.first() {
            properties.insert("value".to_string(), first.value.clone());
        }

        Self {
            evaluator: "keyframe".to_string(),
            properties,
        }
    }

    pub fn expression(expression: String) -> Self {
        Self {
            evaluator: "expression".to_string(),
            properties: HashMap::from([(
                "expression".to_string(),
                PropertyValue::String(expression),
            )]),
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

    pub fn get_static_value(&self) -> Option<&PropertyValue> {
        if self.evaluator == "constant" {
            self.value()
        } else {
            None
        }
    }

    pub fn expression_text(&self) -> Option<&str> {
        self.properties
            .get("expression")
            .and_then(|value| match value {
                PropertyValue::String(expr) => Some(expr.as_str()),
                _ => None,
            })
    }

    /// Find the index of a keyframe at the given time (within tolerance).
    /// Returns None if no keyframe exists at that time or if this is not a keyframe property.
    pub fn keyframe_index_at(&self, time: f64, tolerance: f64) -> Option<usize> {
        if self.evaluator != "keyframe" {
            return None;
        }
        self.keyframes()
            .iter()
            .position(|k| (k.time.into_inner() - time).abs() < tolerance)
    }

    pub fn keyframe_id_at(&self, time: f64, tolerance: f64) -> Option<KeyframeId> {
        if self.evaluator != "keyframe" {
            return None;
        }
        self.keyframes()
            .iter()
            .find(|keyframe| (keyframe.time.into_inner() - time).abs() < tolerance)
            .map(|keyframe| keyframe.id)
    }

    pub fn keyframe_by_id(&self, id: KeyframeId) -> Option<Keyframe> {
        self.keyframes()
            .into_iter()
            .find(|keyframe| keyframe.id == id)
    }

    pub fn keyframe_index_by_id(&self, id: KeyframeId) -> Option<usize> {
        self.keyframes()
            .iter()
            .position(|keyframe| keyframe.id == id)
    }

    /// Check if a keyframe exists at the given time.
    pub fn has_keyframe_at(&self, time: f64, tolerance: f64) -> bool {
        self.keyframe_index_at(time, tolerance).is_some()
    }

    /// Add or update a keyframe at the given time.
    /// If a keyframe already exists at the time, updates its value and optionally its easing.
    /// If easing is None, preserves the existing easing for updates; uses Linear for new keyframes.
    /// If this is a constant property, converts it to a keyframe property.
    /// Returns true if successful.
    pub fn upsert_keyframe(
        &mut self,
        time: f64,
        value: PropertyValue,
        easing: Option<EasingFunction>,
    ) -> bool {
        self.upsert_keyframe_with_id(time, value, easing).is_some()
    }

    /// Add or update a keyframe and return the persistent identity of the
    /// affected key. A tolerance match keeps the existing identity.
    pub fn upsert_keyframe_with_id(
        &mut self,
        time: f64,
        value: PropertyValue,
        easing: Option<EasingFunction>,
    ) -> Option<KeyframeId> {
        const TOLERANCE: f64 = 0.001;

        if self.evaluator == "constant" {
            // Convert to keyframe property
            let kf = Keyframe::new(time, value, easing.unwrap_or(EasingFunction::Linear));
            let id = kf.id;
            *self = Property::keyframe(vec![kf]);
            return Some(id);
        }

        if self.evaluator == "keyframe" {
            let mut kfs = self.keyframes();

            // Check for existing keyframe at this time
            let id = if let Some(idx) = kfs
                .iter()
                .position(|k| (k.time.into_inner() - time).abs() < TOLERANCE)
            {
                // Update existing keyframe, preserving easing if not specified
                let preserved_easing = kfs[idx].easing.clone();
                kfs[idx].value = value;
                kfs[idx].easing = easing.unwrap_or(preserved_easing);
                kfs[idx].id
            } else {
                // Add new keyframe
                let keyframe = Keyframe::new(time, value, easing.unwrap_or(EasingFunction::Linear));
                let id = keyframe.id;
                kfs.push(keyframe);
                kfs.sort_by_key(|k| k.time);
                id
            };

            // Preserve existing property attributes (like interpolation mode)
            let existing_props = self.properties.clone();
            *self = Property::keyframe(kfs);
            for (k, v) in existing_props {
                if k != "keyframes" && k != "value" {
                    self.properties.insert(k, v);
                }
            }
            return Some(id);
        }

        // Other evaluator types (expression, etc.) - cannot add keyframes
        None
    }

    /// Update one keyframe without using its mutable sorted position as its
    /// identity. This remains stable when a time edit crosses neighbouring
    /// keyframes.
    pub fn update_keyframe_by_id(&mut self, id: KeyframeId, update: KeyframeUpdate) -> bool {
        if self.evaluator != "keyframe" {
            return false;
        }

        let mut kfs = self.keyframes();
        let Some(kf) = kfs.iter_mut().find(|keyframe| keyframe.id == id) else {
            return false;
        };
        if let Some(t) = update.time {
            kf.time = OrderedFloat(t);
        }
        if let Some(v) = update.value {
            kf.value = v;
        }
        if let Some(e) = update.easing {
            kf.easing = e;
        }

        kfs.sort_by_key(|k| k.time);

        // Preserve existing property attributes (like interpolation mode)
        let existing_props = self.properties.clone();
        *self = Property::keyframe(kfs);
        for (k, v) in existing_props {
            if k != "keyframes" && k != "value" {
                self.properties.insert(k, v);
            }
        }
        true
    }

    /// Remove the identified keyframe. Removing the final key explicitly
    /// returns the property to a constant containing that key's authored value,
    /// preserving its PropertyValue type.
    pub fn remove_keyframe_by_id(&mut self, id: KeyframeId) -> bool {
        if self.evaluator != "keyframe" {
            return false;
        }

        let mut kfs = self.keyframes();
        let Some(index) = kfs.iter().position(|keyframe| keyframe.id == id) else {
            return false;
        };
        let removed = kfs.remove(index);

        if kfs.is_empty() {
            *self = Property::constant(removed.value);
            return true;
        }

        // Preserve existing property attributes
        let existing_props = self.properties.clone();
        *self = Property::keyframe(kfs);
        for (k, v) in existing_props {
            if k != "keyframes" && k != "value" {
                self.properties.insert(k, v);
            }
        }
        true
    }

    /// Evaluate the property at a specific time.
    /// If constant, returns the constant value.
    /// If expression, returns default value (eval not supported here yet).
    /// If keyframes, interpolates between the two nearest keyframes.
    pub fn evaluate_at(&self, time: f64) -> PropertyValue {
        match self.evaluator.as_str() {
            "constant" => {
                // Return value or default
                self.value()
                    .cloned()
                    .unwrap_or(PropertyValue::Number(OrderedFloat(0.0)))
            }
            "keyframe" => {
                let kfs = self.keyframes();
                if kfs.is_empty() {
                    return self
                        .value()
                        .cloned()
                        .unwrap_or(PropertyValue::Number(OrderedFloat(0.0)));
                }

                // If only one keyframe, return its value
                if kfs.len() == 1 {
                    return kfs[0].value.clone();
                }

                // If before first keyframe
                if time <= kfs[0].time.into_inner() {
                    return kfs[0].value.clone();
                }

                let Some(last_keyframe) = kfs.last() else {
                    return self
                        .value()
                        .cloned()
                        .unwrap_or(PropertyValue::Number(OrderedFloat(0.0)));
                };

                // If after last keyframe
                if time >= last_keyframe.time.into_inner() {
                    return last_keyframe.value.clone();
                }

                // Find the segment [k1, k2] containing time
                for window in kfs.windows(2) {
                    let k1 = &window[0];
                    let k2 = &window[1];
                    let t1 = k1.time.into_inner();
                    let t2 = k2.time.into_inner();

                    if time >= t1 && time < t2 {
                        let duration = t2 - t1;
                        if duration <= f64::EPSILON {
                            return k1.value.clone();
                        }

                        let t_norm = (time - t1) / duration;
                        let t_eased = k1.easing.apply(t_norm); // Use Easing from START keyframe

                        return PropertyValue::interpolate(&k1.value, &k2.value, t_eased);
                    }
                }

                // Should not reach here
                last_keyframe.value.clone()
            }
            _ => self
                .value()
                .cloned()
                .unwrap_or(PropertyValue::Number(OrderedFloat(0.0))),
        }
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

    /// Creates a PropertyMap populated with default values from the given definitions.
    pub fn from_definitions(defs: &[PropertyDefinition]) -> Self {
        let mut map = Self::new();
        for def in defs {
            map.set(
                def.name.clone(),
                Property::constant(def.default_value.clone()),
            );
        }
        map
    }

    pub fn get(&self, key: &str) -> Option<&Property> {
        self.properties.get(key)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Property> {
        self.properties.get_mut(key)
    }

    pub fn set(&mut self, key: String, property: Property) {
        self.properties.insert(key, property);
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Property)> {
        self.properties.iter()
    }

    /// Update a property value or upsert a keyframe if the property is keyframed.
    /// This centralizes the logic for property updates.
    pub fn update_property_or_keyframe(
        &mut self,
        key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<EasingFunction>,
    ) {
        if let Some(prop) = self.properties.get_mut(key) {
            if prop.evaluator == "keyframe" {
                prop.upsert_keyframe(time, value, easing);
            } else {
                // If constant, update directly. If we wanted to promote to keyframe auto-magically on "add keyframe" action,
                // that's handled by add_keyframe calling upsert_keyframe.
                // But for simple updates (drag value), we just set constant.
                self.properties
                    .insert(key.to_string(), Property::constant(value));
            }
        } else {
            // New property, default to constant
            self.properties
                .insert(key.to_string(), Property::constant(value));
        }
    }

    /// Explicitly enables keyframing for a property and inserts or updates a
    /// key at `time`. Unlike a normal property edit, this promotes constants
    /// to the keyframe evaluator.
    pub fn upsert_keyframe(
        &mut self,
        key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<EasingFunction>,
    ) -> bool {
        self.upsert_keyframe_with_id(key, time, value, easing)
            .is_some()
    }

    pub fn upsert_keyframe_with_id(
        &mut self,
        key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<EasingFunction>,
    ) -> Option<KeyframeId> {
        if let Some(property) = self.properties.get_mut(key) {
            property.upsert_keyframe_with_id(time, value, easing)
        } else {
            let keyframe = Keyframe::new(time, value, easing.unwrap_or(EasingFunction::Linear));
            let id = keyframe.id;
            self.properties
                .insert(key.to_string(), Property::keyframe(vec![keyframe]));
            Some(id)
        }
    }

    pub fn get_constant_value(&self, key: &str) -> Option<&PropertyValue> {
        // Legacy support? Or specific usage?
        self.get(key)
            .and_then(|property| match property.evaluator.as_str() {
                "constant" => property.value(),
                _ => None, // Returns None if keyframed, preventing editing base value?
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

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.get_constant_value(key)
            .and_then(|pv| pv.get_as::<bool>())
    }
}

// === UI Property Definitions ===

/// Defines how a property should be displayed and edited in the UI
#[derive(Debug, Clone, PartialEq)]
pub enum PropertyUiType {
    Float {
        min: f64,
        max: f64,
        step: f64,
        suffix: String,
        min_hard_limit: bool,
        max_hard_limit: bool,
    },
    Integer {
        min: i64,
        max: i64,
        suffix: String,
        min_hard_limit: bool,
        max_hard_limit: bool,
    },
    Color,
    Text,
    MultilineText,
    Bool,
    Vec2 {
        suffix: String,
    },
    Vec3 {
        suffix: String,
    },
    Vec4 {
        suffix: String,
    },
    Dropdown {
        options: Vec<String>,
    },
    Font,
}

/// Defines a property with its metadata for UI rendering
#[derive(Debug, Clone)]
pub struct PropertyDefinition {
    name: String,
    label: String,
    ui_type: PropertyUiType,
    default_value: PropertyValue,
}

impl PropertyDefinition {
    pub fn new(
        name: &str,
        ui_type: PropertyUiType,
        label: &str,
        default_value: PropertyValue,
    ) -> Self {
        // Validation
        if !default_value.is_compatible_with(&ui_type) {
            log::error!(
                "Property type mismatch for '{}': ui_type={:?}, default_value={:?}",
                name,
                ui_type,
                default_value
            );
        }

        Self {
            name: name.to_string(),
            label: label.to_string(),
            ui_type,
            default_value,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn label(&self) -> &str {
        &self.label
    }

    pub fn ui_type(&self) -> &PropertyUiType {
        &self.ui_type
    }

    pub fn default_value(&self) -> &PropertyValue {
        &self.default_value
    }

    pub fn set_default_value(&mut self, value: PropertyValue) {
        if !value.is_compatible_with(&self.ui_type) {
            log::warn!(
                "Setting incompatible default value for '{}': expected {:?}, got {:?}",
                self.name,
                self.ui_type,
                value
            );
        }
        self.default_value = value;
    }

    /// Validates the PropertyDefinition itself, including UI constraints and
    /// its default. Runtime/plugin descriptors can share this check before
    /// exposing editable state or constructing graph Nodes.
    pub fn validate_definition(&self) -> Result<(), String> {
        if self.name.trim().is_empty() {
            return Err("Property name must not be empty".to_string());
        }
        if self.label.trim().is_empty() {
            return Err(format!("Property '{}' label must not be empty", self.name));
        }
        match &self.ui_type {
            PropertyUiType::Float { min, max, step, .. } => {
                if !min.is_finite() || !max.is_finite() || !step.is_finite() {
                    return Err(format!(
                        "Property '{}' float bounds and step must be finite",
                        self.name
                    ));
                }
                if min > max {
                    return Err(format!(
                        "Property '{}' float minimum cannot exceed maximum",
                        self.name
                    ));
                }
                if *step <= 0.0 {
                    return Err(format!(
                        "Property '{}' float step must be greater than zero",
                        self.name
                    ));
                }
            }
            PropertyUiType::Integer { min, max, .. } if min > max => {
                return Err(format!(
                    "Property '{}' integer minimum cannot exceed maximum",
                    self.name
                ));
            }
            PropertyUiType::Dropdown { options } => {
                if options.is_empty() {
                    return Err(format!(
                        "Property '{}' dropdown must have at least one option",
                        self.name
                    ));
                }
                let mut unique = HashSet::new();
                for option in options {
                    if option.trim().is_empty() {
                        return Err(format!(
                            "Property '{}' dropdown options must not be empty",
                            self.name
                        ));
                    }
                    if !unique.insert(option) {
                        return Err(format!(
                            "Property '{}' dropdown option {:?} is duplicated",
                            self.name, option
                        ));
                    }
                }
                let PropertyValue::String(default) = &self.default_value else {
                    return Err(format!(
                        "Property '{}' dropdown default must be a string",
                        self.name
                    ));
                };
                if !options.contains(default) {
                    return Err(format!(
                        "Property '{}' dropdown default {:?} is not an option",
                        self.name, default
                    ));
                }
            }
            _ => {}
        }
        self.validate_value(&self.default_value)
    }

    /// Validate an authored value against the definition's type and hard
    /// numeric bounds. Soft min/max values remain UI guidance and are not
    /// mutation constraints.
    pub fn validate_value(&self, value: &PropertyValue) -> Result<(), String> {
        if !value.is_compatible_with(&self.ui_type) {
            return Err(format!(
                "Property '{}' expects {:?}, got {:?}",
                self.name, self.ui_type, value
            ));
        }
        match (&self.ui_type, value) {
            (
                PropertyUiType::Float {
                    min,
                    max,
                    min_hard_limit,
                    max_hard_limit,
                    ..
                },
                PropertyValue::Number(value),
            ) => {
                let value = value.into_inner();
                if !value.is_finite() {
                    return Err(format!("Property '{}' must be finite", self.name));
                }
                if *min_hard_limit && value < *min {
                    return Err(format!(
                        "Property '{}' cannot be less than {min}",
                        self.name
                    ));
                }
                if *max_hard_limit && value > *max {
                    return Err(format!(
                        "Property '{}' cannot be greater than {max}",
                        self.name
                    ));
                }
            }
            (
                PropertyUiType::Integer {
                    min,
                    max,
                    min_hard_limit,
                    max_hard_limit,
                    ..
                },
                PropertyValue::Integer(value),
            ) => {
                if *min_hard_limit && value < min {
                    return Err(format!(
                        "Property '{}' cannot be less than {min}",
                        self.name
                    ));
                }
                if *max_hard_limit && value > max {
                    return Err(format!(
                        "Property '{}' cannot be greater than {max}",
                        self.name
                    ));
                }
            }
            (PropertyUiType::Vec2 { .. }, PropertyValue::Vec2(value))
                if !value.x.is_finite() || !value.y.is_finite() =>
            {
                return Err(format!(
                    "Property '{}' vector components must be finite",
                    self.name
                ));
            }
            (PropertyUiType::Vec3 { .. }, PropertyValue::Vec3(value))
                if !value.x.is_finite() || !value.y.is_finite() || !value.z.is_finite() =>
            {
                return Err(format!(
                    "Property '{}' vector components must be finite",
                    self.name
                ));
            }
            (PropertyUiType::Vec4 { .. }, PropertyValue::Vec4(value))
                if !value.x.is_finite()
                    || !value.y.is_finite()
                    || !value.z.is_finite()
                    || !value.w.is_finite() =>
            {
                return Err(format!(
                    "Property '{}' vector components must be finite",
                    self.name
                ));
            }
            (PropertyUiType::Dropdown { options }, PropertyValue::String(value))
                if !options.contains(value) =>
            {
                return Err(format!(
                    "Property '{}' dropdown value {:?} is not an option",
                    self.name, value
                ));
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod keyframe_tests {
    use super::*;

    fn number(value: f64) -> PropertyValue {
        PropertyValue::Number(OrderedFloat(value))
    }

    #[test]
    fn missing_property_is_promoted_from_the_supplied_default_value() {
        let mut properties = PropertyMap::new();

        let id = properties
            .upsert_keyframe_with_id("opacity", 1.25, number(100.0), None)
            .expect("a missing direct property should be keyframeable");

        let property = properties
            .get("opacity")
            .expect("property should be created");
        assert_eq!(property.evaluator, "keyframe");
        assert_eq!(
            property.keyframes(),
            vec![Keyframe {
                id,
                time: OrderedFloat(1.25),
                value: number(100.0),
                easing: EasingFunction::Linear,
            }]
        );
    }

    #[test]
    fn tolerance_upsert_updates_one_key_and_preserves_identity_and_easing() {
        let mut property = Property::constant(number(10.0));
        let first_id = property
            .upsert_keyframe_with_id(1.0, number(20.0), Some(EasingFunction::EaseInQuad))
            .expect("constant should promote");

        let matched_id = property
            .upsert_keyframe_with_id(1.0005, number(30.0), None)
            .expect("keyframe should update");
        assert_eq!(matched_id, first_id);
        assert_eq!(property.keyframes().len(), 1);
        assert_eq!(property.keyframes()[0].value, number(30.0));
        assert_eq!(property.keyframes()[0].easing, EasingFunction::EaseInQuad);

        let distinct_id = property
            .upsert_keyframe_with_id(1.002, number(40.0), None)
            .expect("time outside tolerance should insert");
        assert_ne!(distinct_id, first_id);
        assert_eq!(property.keyframes().len(), 2);
    }

    #[test]
    fn removing_the_last_keyframe_restores_its_typed_value_as_a_constant() {
        let value = PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(12.0),
            y: OrderedFloat(34.0),
        });
        let mut property = Property::constant(value.clone());
        let id = property
            .upsert_keyframe_with_id(2.0, value.clone(), None)
            .expect("constant should promote");

        assert!(property.remove_keyframe_by_id(id));
        assert_eq!(property.evaluator, "constant");
        assert_eq!(property.value(), Some(&value));
        assert!(property.keyframes().is_empty());
    }

    #[test]
    fn stable_identity_survives_crossing_and_continues_to_edit_the_same_key() {
        let mut property = Property::constant(number(0.0));
        let moving_id = property
            .upsert_keyframe_with_id(1.0, number(10.0), None)
            .expect("first key should insert");
        let stationary_id = property
            .upsert_keyframe_with_id(2.0, number(20.0), None)
            .expect("second key should insert");

        assert!(property.update_keyframe_by_id(
            moving_id,
            KeyframeUpdate {
                time: Some(3.0),
                ..Default::default()
            }
        ));
        assert_eq!(
            property
                .keyframes()
                .iter()
                .map(|keyframe| keyframe.id)
                .collect::<Vec<_>>(),
            vec![stationary_id, moving_id]
        );

        assert!(property.update_keyframe_by_id(
            moving_id,
            KeyframeUpdate {
                value: Some(number(99.0)),
                easing: Some(EasingFunction::Constant),
                ..Default::default()
            }
        ));
        let moved = property
            .keyframe_by_id(moving_id)
            .expect("moving key should still exist");
        let stationary = property
            .keyframe_by_id(stationary_id)
            .expect("stationary key should still exist");
        assert_eq!(moved.time, OrderedFloat(3.0));
        assert_eq!(moved.value, number(99.0));
        assert_eq!(moved.easing, EasingFunction::Constant);
        assert_eq!(stationary.time, OrderedFloat(2.0));
        assert_eq!(stationary.value, number(20.0));
    }

    #[test]
    fn easing_and_keyframe_identity_survive_serialization_roundtrip() {
        let first = Keyframe::new(0.0, number(0.0), EasingFunction::EaseInQuad);
        let second = Keyframe::new(1.0, number(10.0), EasingFunction::Linear);
        let property = Property::keyframe(vec![second.clone(), first.clone()]);

        assert_eq!(property.evaluate_at(0.5), number(2.5));
        let json = serde_json::to_string(&property).expect("property should serialize");
        assert!(json.contains("\"id\""));
        let loaded: Property = serde_json::from_str(&json).expect("property should deserialize");

        assert_eq!(loaded, property);
        assert_eq!(
            loaded
                .keyframes()
                .iter()
                .map(|keyframe| keyframe.id)
                .collect::<Vec<_>>(),
            vec![first.id, second.id]
        );
        assert_eq!(loaded.evaluate_at(0.5), number(2.5));
    }
}
