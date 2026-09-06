use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

use crate::model::property::PropertyValue;

pub const POINT_MAX_ATTRIBUTE_COUNT: usize = 16;
pub const POINT_ATTRIBUTE_NAME_MAX_BYTES: usize = 128;

/// Stable authored identity of a custom Point attribute.
///
/// Display names are intentionally not identities. A Store Attribute Node can
/// derive this ID from its own stable Node UUID, so renaming never disconnects
/// downstream references.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[serde(transparent)]
pub struct PointAttributeId(Uuid);

impl PointAttributeId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub const fn from_uuid(id: Uuid) -> Self {
        Self(id)
    }

    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

impl Default for PointAttributeId {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for PointAttributeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

/// Stable identity of the Node or runtime source that produced a Point set.
///
/// This is not serialized beside every Point. For reusable or nested Modules,
/// the runtime must derive it from the scoped invocation/source identity (not
/// the reusable Node UUID alone), then pair it with an exact `u32` serial.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct PointProducerId(Uuid);

impl PointProducerId {
    pub const fn from_uuid(id: Uuid) -> Self {
        Self(id)
    }

    pub const fn as_uuid(self) -> Uuid {
        self.0
    }
}

/// Stable runtime Point identity. Buffer slot and floating-point attributes
/// must never be substituted for this producer-and-serial pair.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub struct PointId {
    producer: PointProducerId,
    serial: u32,
}

impl PointId {
    pub const fn new(producer: PointProducerId, serial: u32) -> Self {
        Self { producer, serial }
    }

    pub const fn producer(self) -> PointProducerId {
        self.producer
    }

    pub const fn serial(self) -> u32 {
        self.serial
    }
}

/// GPU-compatible authored attribute types.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PointAttributeElementType {
    Number,
    Integer,
    Vec2,
    Vec3,
    Vec4,
    Color,
}

#[derive(Serialize, Clone, PartialEq, Eq, Debug, Hash)]
pub struct PointAttributeDefinition {
    id: PointAttributeId,
    display_name: String,
    element_type: PointAttributeElementType,
    default_value: PropertyValue,
}

impl PointAttributeDefinition {
    pub fn new(
        id: PointAttributeId,
        display_name: impl Into<String>,
        element_type: PointAttributeElementType,
        default_value: PropertyValue,
    ) -> Result<Self, String> {
        let definition = Self {
            id,
            display_name: display_name.into(),
            element_type,
            default_value,
        };
        definition.validate()?;
        Ok(definition)
    }

    pub const fn id(&self) -> PointAttributeId {
        self.id
    }

    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    pub const fn element_type(&self) -> PointAttributeElementType {
        self.element_type
    }

    pub const fn default_value(&self) -> &PropertyValue {
        &self.default_value
    }

    pub fn rename(&mut self, display_name: impl Into<String>) -> Result<(), String> {
        let display_name = display_name.into();
        validate_display_name(&display_name)?;
        self.display_name = display_name;
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_display_name(&self.display_name)?;
        self.element_type
            .validate_authored_default(&self.default_value)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PointAttributeDefinitionWire {
    id: PointAttributeId,
    display_name: String,
    element_type: PointAttributeElementType,
    default_value: PropertyValue,
}

impl<'de> Deserialize<'de> for PointAttributeDefinition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = PointAttributeDefinitionWire::deserialize(deserializer)?;
        Self::new(
            wire.id,
            wire.display_name,
            wire.element_type,
            wire.default_value,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Serialize, Clone, PartialEq, Eq, Debug, Hash)]
#[serde(transparent)]
pub struct PointAttributeSchema {
    attributes: Vec<PointAttributeDefinition>,
}

impl PointAttributeSchema {
    pub fn new(attributes: Vec<PointAttributeDefinition>) -> Result<Self, String> {
        validate_attributes(&attributes)?;
        Ok(Self { attributes })
    }

    pub fn attributes(&self) -> &[PointAttributeDefinition] {
        &self.attributes
    }

    pub fn attribute(&self, id: PointAttributeId) -> Option<&PointAttributeDefinition> {
        self.attributes.iter().find(|attribute| attribute.id == id)
    }

    pub fn rename_attribute(
        &mut self,
        id: PointAttributeId,
        display_name: impl Into<String>,
    ) -> Result<(), String> {
        let display_name = display_name.into();
        validate_display_name(&display_name)?;
        if self
            .attributes
            .iter()
            .any(|attribute| attribute.id != id && attribute.display_name == display_name)
        {
            return Err(format!(
                "Point attribute display name '{display_name}' is already in use"
            ));
        }
        let attribute = self
            .attributes
            .iter_mut()
            .find(|attribute| attribute.id == id)
            .ok_or_else(|| format!("Point attribute {id} does not exist"))?;
        attribute.rename(display_name)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_attributes(&self.attributes)
    }
}

impl<'de> Deserialize<'de> for PointAttributeSchema {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let attributes = Vec::<PointAttributeDefinition>::deserialize(deserializer)?;
        Self::new(attributes).map_err(serde::de::Error::custom)
    }
}

fn validate_attributes(attributes: &[PointAttributeDefinition]) -> Result<(), String> {
    if attributes.len() > POINT_MAX_ATTRIBUTE_COUNT {
        return Err(format!(
            "Point schema has {} attributes, exceeding the maximum of {POINT_MAX_ATTRIBUTE_COUNT}",
            attributes.len()
        ));
    }
    let mut ids = HashSet::with_capacity(attributes.len());
    let mut names = HashSet::with_capacity(attributes.len());
    for attribute in attributes {
        attribute.validate()?;
        if !ids.insert(attribute.id) {
            return Err(format!(
                "Point schema contains duplicate attribute ID {}",
                attribute.id
            ));
        }
        if !names.insert(attribute.display_name.as_str()) {
            return Err(format!(
                "Point schema contains duplicate display name '{}'",
                attribute.display_name
            ));
        }
    }
    Ok(())
}

fn validate_display_name(display_name: &str) -> Result<(), String> {
    if display_name.trim().is_empty() {
        return Err("Point attribute display name must not be empty".to_string());
    }
    if display_name.trim() != display_name {
        return Err(
            "Point attribute display name must not have surrounding whitespace".to_string(),
        );
    }
    if display_name.len() > POINT_ATTRIBUTE_NAME_MAX_BYTES {
        return Err(format!(
            "Point attribute display name exceeds {POINT_ATTRIBUTE_NAME_MAX_BYTES} UTF-8 bytes"
        ));
    }
    Ok(())
}

impl PointAttributeElementType {
    /// Validate the persisted value without requiring a runtime color backend.
    /// Color-space resolution is deferred to GPU layout derivation, allowing a
    /// structurally valid authored color reference to survive load/save even
    /// when the current machine cannot transform that space.
    pub(super) fn validate_authored_default(self, value: &PropertyValue) -> Result<(), String> {
        match (self, value) {
            (Self::Number, PropertyValue::Number(value)) => {
                narrow_f32(value.into_inner())
                    .map_err(|error| format!("Point Number default {error}"))?;
                Ok(())
            }
            (Self::Integer, PropertyValue::Integer(value)) => {
                i32::try_from(*value).map_err(|_| {
                    "Point Integer default must fit exactly in signed i32".to_string()
                })?;
                Ok(())
            }
            (Self::Vec2, PropertyValue::Vec2(value)) => validate_vector_components(&[
                ("x", value.x.into_inner()),
                ("y", value.y.into_inner()),
            ]),
            (Self::Vec3, PropertyValue::Vec3(value)) => validate_vector_components(&[
                ("x", value.x.into_inner()),
                ("y", value.y.into_inner()),
                ("z", value.z.into_inner()),
            ]),
            (Self::Vec4, PropertyValue::Vec4(value)) => validate_vector_components(&[
                ("x", value.x.into_inner()),
                ("y", value.y.into_inner()),
                ("z", value.z.into_inner()),
                ("w", value.w.into_inner()),
            ]),
            // ColorValue construction/deserialization already enforces finite
            // components and straight alpha. Space resolution is not a model
            // invariant and belongs to layout packing below that boundary.
            (Self::Color, PropertyValue::ColorValue(_)) => Ok(()),
            (expected, actual) => Err(default_type_mismatch(expected, actual)),
        }
    }
}

fn validate_vector_components(components: &[(&str, f64)]) -> Result<(), String> {
    for (component, value) in components {
        narrow_f32(*value).map_err(|error| format!("component {component} {error}"))?;
    }
    Ok(())
}

/// Narrow with ordinary IEEE-754 rounding, while rejecting loss of a finite,
/// non-zero authored value to zero and rejecting overflow to infinity.
pub(super) fn narrow_f32(value: f64) -> Result<f32, &'static str> {
    let narrowed = value as f32;
    if !value.is_finite() || !narrowed.is_finite() {
        return Err("must be finite and representable as f32");
    }
    if value != 0.0 && narrowed == 0.0 {
        return Err("must not underflow to zero when represented as f32");
    }
    Ok(narrowed)
}

pub(super) fn default_type_mismatch(
    expected: PointAttributeElementType,
    actual: &PropertyValue,
) -> String {
    format!(
        "Point {expected:?} attribute requires a matching default, received {}",
        property_value_kind(actual)
    )
}

fn property_value_kind(value: &PropertyValue) -> &'static str {
    match value {
        PropertyValue::Integer(_) => "Integer",
        PropertyValue::Number(_) => "Number",
        PropertyValue::String(_) => "String",
        PropertyValue::Boolean(_) => "Boolean",
        PropertyValue::Vec2(_) => "Vec2",
        PropertyValue::Vec3(_) => "Vec3",
        PropertyValue::Vec4(_) => "Vec4",
        PropertyValue::ColorValue(_) | PropertyValue::Color(_) => "Color",
        PropertyValue::Path(_) => "Path",
        PropertyValue::Gradient(_) => "Gradient",
        PropertyValue::Pattern(_) => "Pattern",
        PropertyValue::Array(_) => "Array",
        PropertyValue::Map(_) => "Map",
        PropertyValue::OpaqueJson(_) => "opaque JSON",
    }
}
