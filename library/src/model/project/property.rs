use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

use ordered_float::OrderedFloat;
use std::hash::{Hash, Hasher};
use uuid::Uuid;

use crate::animation::EasingFunction;
use crate::model::frame::color::Color;

mod color_value;
mod evaluation;

pub use color_value::{ColorSpaceRef, ColorValue, ColorValueError};
pub use evaluation::PropertySampleError;
pub use value::{PropertyValue, TryGetProperty, Vec2, Vec3, Vec4};

mod value;
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

    /// Creates a Python Expression property with an authored, type-defining
    /// input value. The Inspector must supply a value compatible with the
    /// property's [`PropertyDefinition`]. Evaluation errors are reported by
    /// the registered evaluator and never silently substitute this value.
    pub fn expression(expression: String, fallback: PropertyValue) -> Self {
        Self {
            evaluator: "expression".to_string(),
            properties: HashMap::from([
                ("expression".to_string(), PropertyValue::String(expression)),
                ("value".to_string(), fallback),
            ]),
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
}

#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Eq, Debug)]
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

    pub(crate) fn remove(&mut self, key: &str) -> Option<Property> {
        self.properties.remove(key)
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
            match prop.evaluator.as_str() {
                "keyframe" => {
                    prop.upsert_keyframe(time, value, easing);
                }
                "constant" => {
                    *prop = Property::constant(value);
                }
                _ => {
                    // Expression and plugin evaluators own their authored mode.
                    // A normal value edit changes the typed `value` input; it
                    // must not silently replace the evaluator with constant.
                    prop.properties.insert("value".to_string(), value);
                }
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
        min: f64,
        max: f64,
        step: f64,
        suffix: String,
        min_hard_limit: bool,
        max_hard_limit: bool,
    },
    Vec3 {
        min: f64,
        max: f64,
        step: f64,
        suffix: String,
        min_hard_limit: bool,
        max_hard_limit: bool,
    },
    Vec4 {
        min: f64,
        max: f64,
        step: f64,
        suffix: String,
        min_hard_limit: bool,
        max_hard_limit: bool,
    },
    Dropdown {
        options: Vec<String>,
    },
    Font,
}

impl PropertyUiType {
    const DEFAULT_VECTOR_MIN: f64 = -1_000_000.0;
    const DEFAULT_VECTOR_MAX: f64 = 1_000_000.0;
    const DEFAULT_VECTOR_STEP: f64 = 0.1;

    pub fn vec2(suffix: impl Into<String>) -> Self {
        Self::vec2_with_range(
            Self::DEFAULT_VECTOR_MIN,
            Self::DEFAULT_VECTOR_MAX,
            Self::DEFAULT_VECTOR_STEP,
            suffix,
            false,
            false,
        )
    }

    pub fn vec2_with_range(
        min: f64,
        max: f64,
        step: f64,
        suffix: impl Into<String>,
        min_hard_limit: bool,
        max_hard_limit: bool,
    ) -> Self {
        Self::Vec2 {
            min,
            max,
            step,
            suffix: suffix.into(),
            min_hard_limit,
            max_hard_limit,
        }
    }

    pub fn vec3(suffix: impl Into<String>) -> Self {
        Self::vec3_with_range(
            Self::DEFAULT_VECTOR_MIN,
            Self::DEFAULT_VECTOR_MAX,
            Self::DEFAULT_VECTOR_STEP,
            suffix,
            false,
            false,
        )
    }

    pub fn vec3_with_range(
        min: f64,
        max: f64,
        step: f64,
        suffix: impl Into<String>,
        min_hard_limit: bool,
        max_hard_limit: bool,
    ) -> Self {
        Self::Vec3 {
            min,
            max,
            step,
            suffix: suffix.into(),
            min_hard_limit,
            max_hard_limit,
        }
    }

    pub fn vec4(suffix: impl Into<String>) -> Self {
        Self::vec4_with_range(
            Self::DEFAULT_VECTOR_MIN,
            Self::DEFAULT_VECTOR_MAX,
            Self::DEFAULT_VECTOR_STEP,
            suffix,
            false,
            false,
        )
    }

    pub fn vec4_with_range(
        min: f64,
        max: f64,
        step: f64,
        suffix: impl Into<String>,
        min_hard_limit: bool,
        max_hard_limit: bool,
    ) -> Self {
        Self::Vec4 {
            min,
            max,
            step,
            suffix: suffix.into(),
            min_hard_limit,
            max_hard_limit,
        }
    }
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
            PropertyUiType::Float { min, max, step, .. }
            | PropertyUiType::Vec2 { min, max, step, .. }
            | PropertyUiType::Vec3 { min, max, step, .. }
            | PropertyUiType::Vec4 { min, max, step, .. } => {
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

        assert_eq!(property.evaluate_at(0.5).unwrap(), number(2.5));
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
        assert_eq!(loaded.evaluate_at(0.5).unwrap(), number(2.5));
    }

    #[test]
    fn value_edits_preserve_expression_and_plugin_evaluator_modes() {
        let mut properties = PropertyMap::new();
        properties.set(
            "expression".to_string(),
            Property::expression("value * 2".to_string(), number(3.0)),
        );
        properties.set(
            "plugin".to_string(),
            Property {
                evaluator: "third-party".to_string(),
                properties: HashMap::from([
                    ("value".to_string(), number(4.0)),
                    ("configuration".to_string(), PropertyValue::Boolean(true)),
                ]),
            },
        );

        properties.update_property_or_keyframe("expression", 0.0, number(5.0), None);
        properties.update_property_or_keyframe("plugin", 0.0, number(6.0), None);

        let expression = properties.get("expression").unwrap();
        assert_eq!(expression.evaluator, "expression");
        assert_eq!(expression.expression_text(), Some("value * 2"));
        assert_eq!(expression.value(), Some(&number(5.0)));
        let plugin = properties.get("plugin").unwrap();
        assert_eq!(plugin.evaluator, "third-party");
        assert_eq!(plugin.value(), Some(&number(6.0)));
        assert_eq!(
            plugin.properties.get("configuration"),
            Some(&PropertyValue::Boolean(true))
        );
    }
}
