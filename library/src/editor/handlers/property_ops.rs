//! Shared property operations for handlers.
//!
//! This module provides a unified interface for property updates across
//! directly-owned property maps.

use crate::error::LibraryError;
use crate::model::project::Project;
use crate::model::property::{KeyframeId, KeyframeUpdate, Property, PropertyMap, PropertyValue};
use uuid::Uuid;

/// Explicit owner of an editable property tree.
///
/// The owner disambiguates whether a direct property map belongs to a timeline
/// Clip or to a Node.
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

pub(crate) fn property_map(
    project: &Project,
    owner: PropertyOwner,
) -> Result<&PropertyMap, LibraryError> {
    match owner {
        PropertyOwner::Clip(clip_id) => project
            .get_clip(clip_id)
            .map(|clip| &clip.properties)
            .ok_or_else(|| LibraryError::Project(format!("Clip {clip_id} not found"))),
        PropertyOwner::Node(node_id) => project
            .get_node(node_id)
            .map(|node| node.properties())
            .ok_or_else(|| LibraryError::Project(format!("Node {node_id} not found"))),
    }
}

pub(crate) fn replace_property(
    project: &mut Project,
    owner: PropertyOwner,
    property_key: &str,
    property: Property,
) -> Result<(), LibraryError> {
    match owner {
        PropertyOwner::Clip(clip_id) => {
            let clip = project
                .get_clip_mut(clip_id)
                .ok_or_else(|| LibraryError::Project(format!("Clip {clip_id} not found")))?;
            if clip.properties.get(property_key).is_none() {
                return Err(missing_property(owner, property_key));
            }
            clip.properties.set(property_key.to_string(), property);
            Ok(())
        }
        PropertyOwner::Node(node_id) => project
            .get_node_mut(node_id)
            .ok_or_else(|| LibraryError::Project(format!("Node {node_id} not found")))?
            .set_property(property_key.to_string(), property)
            .map_err(LibraryError::Project),
    }
}

pub(crate) fn upsert_keyframe_with_id(
    project: &mut Project,
    owner: PropertyOwner,
    property_key: &str,
    time: f64,
    value: PropertyValue,
    easing: Option<crate::animation::EasingFunction>,
) -> Result<KeyframeId, LibraryError> {
    let id = match owner {
        PropertyOwner::Clip(clip_id) => {
            let clip = project
                .get_clip_mut(clip_id)
                .ok_or_else(|| LibraryError::Project(format!("Clip {clip_id} not found")))?;
            if clip.properties.get(property_key).is_none() {
                return Err(missing_property(owner, property_key));
            }
            clip.properties
                .upsert_keyframe_with_id(property_key, time, value, easing)
        }
        PropertyOwner::Node(node_id) => project
            .get_node_mut(node_id)
            .ok_or_else(|| LibraryError::Project(format!("Node {node_id} not found")))?
            .upsert_keyframe_with_id(property_key, time, value, easing),
    };
    id.ok_or_else(|| LibraryError::Project(format!("Property {property_key} cannot be keyframed")))
}

pub(crate) fn update_keyframe_by_id(
    project: &mut Project,
    owner: PropertyOwner,
    property_key: &str,
    keyframe_id: KeyframeId,
    update: KeyframeUpdate,
) -> Result<(), LibraryError> {
    let updated = match owner {
        PropertyOwner::Clip(clip_id) => {
            let clip = project
                .get_clip_mut(clip_id)
                .ok_or_else(|| LibraryError::Project(format!("Clip {clip_id} not found")))?;
            clip.properties
                .get_mut(property_key)
                .ok_or_else(|| missing_property(owner, property_key))?
                .update_keyframe_by_id(keyframe_id, update)
        }
        PropertyOwner::Node(node_id) => project
            .get_node_mut(node_id)
            .ok_or_else(|| LibraryError::Project(format!("Node {node_id} not found")))?
            .update_keyframe_by_id(property_key, keyframe_id, update),
    };
    if updated {
        Ok(())
    } else {
        Err(LibraryError::Project(format!(
            "Failed to update keyframe {keyframe_id} for property {property_key}"
        )))
    }
}

pub(crate) fn remove_keyframe_by_id(
    project: &mut Project,
    owner: PropertyOwner,
    property_key: &str,
    keyframe_id: KeyframeId,
) -> Result<(), LibraryError> {
    let removed = match owner {
        PropertyOwner::Clip(clip_id) => {
            let clip = project
                .get_clip_mut(clip_id)
                .ok_or_else(|| LibraryError::Project(format!("Clip {clip_id} not found")))?;
            clip.properties
                .get_mut(property_key)
                .ok_or_else(|| missing_property(owner, property_key))?
                .remove_keyframe_by_id(keyframe_id)
        }
        PropertyOwner::Node(node_id) => project
            .get_node_mut(node_id)
            .ok_or_else(|| LibraryError::Project(format!("Node {node_id} not found")))?
            .remove_keyframe_by_id(property_key, keyframe_id),
    };
    if removed {
        Ok(())
    } else {
        Err(LibraryError::Project(format!(
            "Failed to remove keyframe {keyframe_id} for property {property_key}"
        )))
    }
}

pub(crate) fn set_property_attribute(
    project: &mut Project,
    owner: PropertyOwner,
    property_key: &str,
    attribute_key: String,
    attribute_value: PropertyValue,
) -> Result<(), LibraryError> {
    match owner {
        PropertyOwner::Clip(clip_id) => {
            let clip = project
                .get_clip_mut(clip_id)
                .ok_or_else(|| LibraryError::Project(format!("Clip {clip_id} not found")))?;
            let property = clip
                .properties
                .get_mut(property_key)
                .ok_or_else(|| missing_property(owner, property_key))?;
            property.properties.insert(attribute_key, attribute_value);
            Ok(())
        }
        PropertyOwner::Node(node_id) => {
            let node = project
                .get_node_mut(node_id)
                .ok_or_else(|| LibraryError::Project(format!("Node {node_id} not found")))?;
            if node.set_property_attribute(property_key, attribute_key, attribute_value) {
                Ok(())
            } else {
                Err(missing_property(owner, property_key))
            }
        }
    }
}

fn missing_property(owner: PropertyOwner, property_key: &str) -> LibraryError {
    LibraryError::Project(format!("Property {property_key} not found on {owner:?}"))
}
