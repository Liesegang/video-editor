//! Shared property operations for handlers.
//!
//! This module provides a unified interface for property updates across
//! clip properties, effect properties, and style properties.

use crate::animation::EasingFunction;
use crate::error::LibraryError;
use crate::model::project::property::{Property, PropertyValue};

/// Target types for nested property operations
pub enum PropertyContainer<'a> {
    /// Direct clip property
    Clip(&'a mut crate::model::project::property::PropertyMap),
    /// Effect property (effect index, property map)
    Effect(&'a mut crate::model::project::EffectConfig),
    /// Style property (style index, property map)
    Style(&'a mut crate::model::project::style::StyleInstance),
}

impl<'a> PropertyContainer<'a> {
    /// Get mutable reference to the property by key
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Property> {
        match self {
            PropertyContainer::Clip(map) => map.get_mut(key),
            PropertyContainer::Effect(effect) => effect.properties.get_mut(key),
            PropertyContainer::Style(style) => style.properties.get_mut(key),
        }
    }

    /// Set a property value
    pub fn set(&mut self, key: String, prop: Property) {
        match self {
            PropertyContainer::Clip(map) => map.set(key, prop),
            PropertyContainer::Effect(effect) => {
                effect.properties.set(key, prop);
            }
            PropertyContainer::Style(style) => {
                style.properties.set(key, prop);
            }
        }
    }
}

/// Update a property value or keyframe at the given time.
/// Creates constant property if property doesn't exist.
pub fn upsert_property_or_keyframe(
    container: &mut PropertyContainer,
    property_key: &str,
    time: f64,
    value: PropertyValue,
    easing: Option<EasingFunction>,
) -> Result<(), LibraryError> {
    if let Some(prop) = container.get_mut(property_key) {
        if prop.evaluator == "keyframe" {
            prop.upsert_keyframe(time, value, easing);
        } else {
            // Update as constant
            let key = property_key.to_string();
            container.set(key, Property::constant(value));
        }
    } else {
        // Property doesn't exist, create as constant
        container.set(property_key.to_string(), Property::constant(value));
    }
    Ok(())
}

/// Update a keyframe at the given index.
pub fn update_keyframe_at_index(
    container: &mut PropertyContainer,
    property_key: &str,
    index: usize,
    new_time: Option<f64>,
    new_value: Option<PropertyValue>,
    new_easing: Option<EasingFunction>,
) -> Result<(), LibraryError> {
    let prop = container
        .get_mut(property_key)
        .ok_or_else(|| LibraryError::Project(format!("Property {} not found", property_key)))?;

    if !prop.update_keyframe_at_index(index, new_time, new_value, new_easing) {
        return Err(LibraryError::Project(
            "Failed to update keyframe (not a keyframe property or index out of bounds)"
                .to_string(),
        ));
    }
    Ok(())
}

/// Remove a keyframe at the given index.
pub fn remove_keyframe_at_index(
    container: &mut PropertyContainer,
    property_key: &str,
    index: usize,
) -> Result<(), LibraryError> {
    let prop = container
        .get_mut(property_key)
        .ok_or_else(|| LibraryError::Project(format!("Property {} not found", property_key)))?;

    if !prop.remove_keyframe_at_index(index) {
        return Err(LibraryError::Project(
            "Failed to remove keyframe (not a keyframe property or index out of bounds)"
                .to_string(),
        ));
    }
    Ok(())
}

/// Set a property attribute.
pub fn set_property_attribute(
    container: &mut PropertyContainer,
    property_key: &str,
    attribute_key: &str,
    attribute_value: PropertyValue,
) -> Result<(), LibraryError> {
    let prop = container
        .get_mut(property_key)
        .ok_or_else(|| LibraryError::Project(format!("Property {} not found", property_key)))?;

    prop.properties
        .insert(attribute_key.to_string(), attribute_value);
    Ok(())
}
