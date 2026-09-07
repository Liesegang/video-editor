//! Point-stream tracing shared by geometry and attribute stages.

use std::collections::HashSet;

use crate::model::authoring::{ModuleDefinition, ModulePortAddress};
use crate::model::node::{PARTICLE_SYSTEM_PORT, POINT_SOURCE_PORT, PointNodeRole};

use super::{address, point_role, single_input_source};

#[derive(Clone, Copy)]
pub(super) enum PointStage {
    StoreAttribute(uuid::Uuid),
    SetPosition(uuid::Uuid),
    SetSize(uuid::Uuid),
    Passthrough(uuid::Uuid),
}

/// Point stream selected by one Sprite endpoint before source recognition.
/// Render-stage operations are ordered upstream-to-downstream.
pub(super) struct PointStreamTrace {
    pub(super) terminal_source: ModulePortAddress,
    pub(super) stages: Vec<PointStage>,
}

impl PointStreamTrace {
    pub(super) fn stores(&self) -> impl Iterator<Item = uuid::Uuid> + '_ {
        self.stages.iter().filter_map(|stage| match stage {
            PointStage::StoreAttribute(node_id) => Some(*node_id),
            PointStage::SetPosition(_) | PointStage::SetSize(_) | PointStage::Passthrough(_) => {
                None
            }
        })
    }
}

pub(super) fn trace_point_stream(
    definition: &ModuleDefinition,
    renderer_node_id: uuid::Uuid,
) -> Result<Option<PointStreamTrace>, String> {
    let renderer_input = address(renderer_node_id, PARTICLE_SYSTEM_PORT);
    let Some(mut source) = single_input_source(definition, &renderer_input) else {
        return Ok(None);
    };
    let mut stages = Vec::new();
    let mut visited = HashSet::new();
    loop {
        let Some(node) = definition.graph.nodes.get(&source.node_id) else {
            return Ok(None);
        };
        let Some(role) = point_role(node) else {
            break;
        };
        let stage = match role {
            PointNodeRole::StoreAttribute(_) => PointStage::StoreAttribute(node.id),
            PointNodeRole::SetPosition => PointStage::SetPosition(node.id),
            PointNodeRole::SetSize => PointStage::SetSize(node.id),
            PointNodeRole::Grid | PointNodeRole::Info => break,
        };
        if source.port != POINT_SOURCE_PORT || !visited.insert(node.id) {
            return Ok(None);
        }
        if !node.enabled {
            return Ok(None);
        }
        stages.push(if node.bypassed {
            PointStage::Passthrough(node.id)
        } else {
            stage
        });
        let point_input = address(node.id, POINT_SOURCE_PORT);
        let Some(upstream) = single_input_source(definition, &point_input) else {
            return Ok(None);
        };
        source = upstream;
    }
    stages.reverse();
    Ok(Some(PointStreamTrace {
        terminal_source: source,
        stages,
    }))
}
