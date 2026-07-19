//! Shared property operations for handlers.
//!
//! This module provides a unified interface for property updates across
//! directly-owned and nested property maps.

use crate::animation::EasingFunction;
use crate::error::LibraryError;
use crate::model::project::Project;
use crate::model::property::{Property, PropertyMap, PropertyTarget, PropertyValue};
use uuid::Uuid;

/// Explicit owner of an editable property tree.
///
/// The owner disambiguates whether a direct property map belongs to a timeline
/// Clip or to a leaf Node. Nested targets use their persistent model identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PropertyOwner {
    Clip(Uuid),
    Node(Uuid),
}

impl PropertyOwner {
    pub fn id(self) -> Uuid {
        match self {
            Self::Clip(id) | Self::Node(id) => id,
        }
    }
}

pub fn property_map(
    project: &Project,
    owner: PropertyOwner,
    target: PropertyTarget,
) -> Result<&PropertyMap, LibraryError> {
    match owner {
        PropertyOwner::Clip(clip_id) => project
            .get_clip(clip_id)
            .ok_or_else(|| LibraryError::Project(format!("Clip {clip_id} not found")))?
            .property_map(target),
        PropertyOwner::Node(node_id) => project
            .get_node(node_id)
            .ok_or_else(|| LibraryError::Project(format!("Node {node_id} not found")))?
            .property_map(target),
    }
    .ok_or_else(|| {
        LibraryError::Project(format!("Property target {target:?} not found on {owner:?}"))
    })
}

pub fn property_map_mut(
    project: &mut Project,
    owner: PropertyOwner,
    target: PropertyTarget,
) -> Result<&mut PropertyMap, LibraryError> {
    match owner {
        PropertyOwner::Clip(clip_id) => project
            .get_clip_mut(clip_id)
            .ok_or_else(|| LibraryError::Project(format!("Clip {clip_id} not found")))?
            .property_map_mut(target),
        PropertyOwner::Node(node_id) => project
            .get_node_mut(node_id)
            .ok_or_else(|| LibraryError::Project(format!("Node {node_id} not found")))?
            .property_map_mut(target),
    }
    .ok_or_else(|| {
        LibraryError::Project(format!("Property target {target:?} not found on {owner:?}"))
    })
}

/// Target types for nested property operations
pub enum PropertyContainer<'a> {
    /// Property map directly owned by a Clip or Node.
    Direct(&'a mut crate::model::property::PropertyMap),
    Effect(&'a mut crate::model::EffectConfig),
    Style(&'a mut crate::model::style::StyleInstance),
    Effector(&'a mut crate::model::ensemble::EffectorInstance),
    Decorator(&'a mut crate::model::ensemble::DecoratorInstance),
}

impl PropertyContainer<'_> {
    /// Get mutable reference to the property by key
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Property> {
        match self {
            PropertyContainer::Direct(map) => map.get_mut(key),
            PropertyContainer::Effect(effect) => effect.properties.get_mut(key),
            PropertyContainer::Style(style) => style.properties.get_mut(key),
            PropertyContainer::Effector(effector) => effector.properties.get_mut(key),
            PropertyContainer::Decorator(decorator) => decorator.properties.get_mut(key),
        }
    }

    /// Set a property value
    pub fn set(&mut self, key: String, prop: Property) {
        match self {
            PropertyContainer::Direct(map) => map.set(key, prop),
            PropertyContainer::Effect(effect) => {
                effect.properties.set(key, prop);
            }
            PropertyContainer::Style(style) => {
                style.properties.set(key, prop);
            }
            PropertyContainer::Effector(effector) => {
                effector.properties.set(key, prop);
            }
            PropertyContainer::Decorator(decorator) => {
                decorator.properties.set(key, prop);
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
