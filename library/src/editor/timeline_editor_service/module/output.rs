//! Authoritative editing guards for dedicated Module Output terminals.

use crate::model::authoring::ModuleDefinition;
use crate::model::node::{Node, NodeContent};

pub(super) fn require_insertable_processing_node(node: &Node) -> Result<(), String> {
    if matches!(node.content(), NodeContent::ModuleOutput(_)) {
        Err(
            "Dedicated Output Nodes are created by the Module definition factory, not the ordinary Node catalog"
                .to_string(),
        )
    } else {
        Ok(())
    }
}

pub(super) fn require_removable_processing_node(
    definition: &ModuleDefinition,
    node_id: uuid::Uuid,
) -> Result<(), String> {
    if matches!(
        definition.graph.nodes.get(&node_id).map(Node::content),
        Some(NodeContent::ModuleOutput(_))
    ) {
        Err(format!(
            "Module Output Node {node_id} is a required render terminal and cannot be deleted"
        ))
    } else {
        Ok(())
    }
}

pub(super) fn require_output_state(
    node: &Node,
    enabled: bool,
    bypassed: bool,
) -> Result<(), String> {
    if matches!(node.content(), NodeContent::ModuleOutput(_)) && (!enabled || bypassed) {
        Err("A Module Output Node cannot be disabled or bypassed".to_string())
    } else {
        Ok(())
    }
}
