//! Shared property operations for handlers.
//!
//! This module provides a unified interface for property updates across
//! directly-owned and nested property maps.

use crate::error::LibraryError;
use crate::model::project::Project;
use crate::model::property::{PropertyMap, PropertyTarget};
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
