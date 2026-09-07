//! Point render endpoints share source tracing and field compilation.

use std::collections::{HashMap, HashSet};

use crate::core::render_plan::{CompiledPointRenderStyle, CompiledPointRenderer};
use crate::model::authoring::{ModuleDefinition, ModulePortAddress};
use crate::model::node::{
    Node, NodeContent, PARTICLE_SPRITE_RENDERER_CATALOG_ID, PARTICLE_SYSTEM_PORT,
    POINT_COLOR_INPUT_PORT, POINT_CONNECTIONS_PORT, POINT_LINE_RENDERER_CATALOG_ID,
    POINT_SOURCE_PORT, PointConnectionNodeRole, SPRITE_SELECTION_INPUT_PORT,
};

use super::stream::trace_point_stream;
use super::{
    address, compile_point_program, compile_point_source, single_input_source,
    validate_point_field_consumers,
};

pub(in crate::core::render_plan) fn compile_point_renderers(
    definition: &ModuleDefinition,
    active_nodes: &HashSet<uuid::Uuid>,
) -> Result<HashMap<uuid::Uuid, CompiledPointRenderer>, String> {
    validate_point_field_consumers(definition, active_nodes)?;
    let mut compiled = HashMap::new();
    let mut candidates = active_nodes.iter().copied().collect::<Vec<_>>();
    candidates.sort_unstable();
    for node_id in candidates {
        let Some(node) = definition.graph.nodes.get(&node_id) else {
            continue;
        };
        if !node.enabled || node.bypassed {
            continue;
        }
        let Some((input, render_style)) = point_endpoint(definition, node)? else {
            continue;
        };
        let Some(trace) = trace_point_stream(definition, &input)? else {
            continue;
        };
        let Some(source) = compile_point_source(definition, &trace.terminal_source)? else {
            continue;
        };
        let point_program = compile_point_program(
            definition,
            node_id,
            render_style,
            &trace,
            &source.lineage,
            source.capabilities,
        )?;
        compiled.insert(
            node_id,
            CompiledPointRenderer {
                source: source.source,
                render_style,
                point_program,
                renderer_node_id: node_id,
                state_slot_id: node_id,
            },
        );
    }
    Ok(compiled)
}

pub(super) fn point_endpoint(
    definition: &ModuleDefinition,
    node: &Node,
) -> Result<Option<(ModulePortAddress, CompiledPointRenderStyle)>, String> {
    let NodeContent::NativeOperation(operation) = node.content() else {
        return Ok(None);
    };
    match operation.catalog_id.as_str() {
        PARTICLE_SPRITE_RENDERER_CATALOG_ID => Ok(Some((
            address(node.id, PARTICLE_SYSTEM_PORT),
            CompiledPointRenderStyle::Sprites,
        ))),
        POINT_LINE_RENDERER_CATALOG_ID => {
            let Some(source) =
                single_input_source(definition, &address(node.id, POINT_CONNECTIONS_PORT))
            else {
                return Ok(None);
            };
            let Some(connections) = definition.graph.nodes.get(&source.node_id) else {
                return Ok(None);
            };
            if !connections.enabled || connections.bypassed {
                return Ok(None);
            }
            if source.port != POINT_CONNECTIONS_PORT
                || !matches!(connections.content(),
                    NodeContent::NativeOperation(operation)
                        if PointConnectionNodeRole::from_catalog_id(&operation.catalog_id)
                            == Some(PointConnectionNodeRole::ConnectPoints)
                )
            {
                return Err(
                    "Line Renderer requires a Connect Points topology in the same Module".into(),
                );
            }
            Ok(Some((
                address(connections.id, POINT_SOURCE_PORT),
                CompiledPointRenderStyle::Lines {
                    connections_node_id: connections.id,
                },
            )))
        }
        _ => Ok(None),
    }
}

pub(super) fn supports_point_field_input(node: &Node, port: &str) -> bool {
    let NodeContent::NativeOperation(operation) = node.content() else {
        return false;
    };
    match operation.catalog_id.as_str() {
        PARTICLE_SPRITE_RENDERER_CATALOG_ID => {
            matches!(port, POINT_COLOR_INPUT_PORT | SPRITE_SELECTION_INPUT_PORT)
        }
        POINT_LINE_RENDERER_CATALOG_ID => port == POINT_COLOR_INPUT_PORT,
        _ => false,
    }
}
