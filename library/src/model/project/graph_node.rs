//! Generic graph node for data-flow graph.

use crate::model::project::property::PropertyMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A generic graph node that can represent any node type in the data-flow graph.
///
/// Instead of having separate types for effects, styles, effectors, etc.,
/// all graph nodes share this single structure. The `type_id` field references
/// a `NodeTypeDefinition` registered in the `PluginManager` to determine
/// the node's behavior, pins, and default properties.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct GraphNode {
    pub id: Uuid,
    /// References a NodeTypeDefinition registered in PluginManager.
    /// Examples: "effect.blur", "style.fill", "math.add", "effector.transform"
    pub type_id: String,
    pub properties: PropertyMap,
}

impl GraphNode {
    pub fn new(type_id: &str, properties: PropertyMap) -> Self {
        Self {
            id: Uuid::new_v4(),
            type_id: type_id.to_string(),
            properties,
        }
    }

    pub fn new_with_id(id: Uuid, type_id: &str, properties: PropertyMap) -> Self {
        Self {
            id,
            type_id: type_id.to_string(),
            properties,
        }
    }
}
