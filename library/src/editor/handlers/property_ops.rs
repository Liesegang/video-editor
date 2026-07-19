//! Shared property operations for handlers.
//!
//! This module provides a unified interface for property updates across
//! directly-owned property maps.

use crate::error::LibraryError;
use crate::model::project::Project;
use crate::model::property::PropertyMap;
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

pub fn property_map(project: &Project, owner: PropertyOwner) -> Result<&PropertyMap, LibraryError> {
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

pub fn property_map_mut(
    project: &mut Project,
    owner: PropertyOwner,
) -> Result<&mut PropertyMap, LibraryError> {
    match owner {
        PropertyOwner::Clip(clip_id) => project
            .get_clip_mut(clip_id)
            .map(|clip| &mut clip.properties)
            .ok_or_else(|| LibraryError::Project(format!("Clip {clip_id} not found"))),
        PropertyOwner::Node(node_id) => project
            .get_node_mut(node_id)
            .map(|node| node.properties_mut())
            .ok_or_else(|| LibraryError::Project(format!("Node {node_id} not found"))),
    }
}
