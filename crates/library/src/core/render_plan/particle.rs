//! Compilation of the bounded executable Particle Node chain.

use std::collections::{HashMap, HashSet};

use crate::model::authoring::{ModuleDefinition, ModulePortAddress};
use crate::model::node::{Node, NodeContent, PARTICLE_SYSTEM_PORT, ParticleNodeRole};

use super::CompiledParticleDefinition;

pub(super) fn compile_particle_renderers(
    definition: &ModuleDefinition,
    active_nodes: &HashSet<uuid::Uuid>,
) -> HashMap<uuid::Uuid, CompiledParticleDefinition> {
    let mut compiled = HashMap::new();
    let mut candidate_ids = active_nodes.iter().copied().collect::<Vec<_>>();
    candidate_ids.sort_unstable();
    for renderer_node_id in candidate_ids {
        let Some(node) = definition.graph.nodes.get(&renderer_node_id) else {
            continue;
        };
        if native_role(node) != Some(ParticleNodeRole::SpriteRenderer) {
            continue;
        }
        // Disabled Nodes produce no output before resolving their descriptor,
        // properties, or upstream topology. Particle endpoints cannot
        // type-preservingly bypass, so bypassing either endpoint is likewise
        // a stable no-image result.
        if !node.enabled || node.bypassed {
            continue;
        }
        if let Some(particle) = compile_particle_chain(definition, renderer_node_id) {
            compiled.insert(renderer_node_id, particle);
        }
    }
    compiled
}

#[derive(Default)]
struct ParticleStages {
    emitter: Option<uuid::Uuid>,
    initialize: Option<uuid::Uuid>,
    gravity: Option<uuid::Uuid>,
    drag: Option<uuid::Uuid>,
}

/// Compile the implemented typed stages while allowing omitted modifiers
/// (for example Emitter -> Gravity -> Sprite). Incomplete, disabled,
/// unsupported, duplicate, or out-of-order chains are a stable no-image
/// result while the Node Editor is being rewired; they never turn a
/// model-valid Project into a RenderPlan compilation failure.
fn compile_particle_chain(
    definition: &ModuleDefinition,
    renderer_node_id: uuid::Uuid,
) -> Option<CompiledParticleDefinition> {
    let mut stages = ParticleStages::default();
    let mut downstream_rank = ParticleNodeRole::SpriteRenderer.execution_rank();
    let mut downstream_node_id = renderer_node_id;
    loop {
        let node = single_particle_source(definition, downstream_node_id)?;
        if !node.enabled {
            return None;
        }
        let role = native_role(node)?;
        if role == ParticleNodeRole::SpriteRenderer || role.execution_rank() >= downstream_rank {
            return None;
        }
        downstream_rank = role.execution_rank();
        let slot = match role {
            ParticleNodeRole::Emitter => &mut stages.emitter,
            ParticleNodeRole::Initialize => &mut stages.initialize,
            ParticleNodeRole::Gravity => &mut stages.gravity,
            ParticleNodeRole::Drag => &mut stages.drag,
            ParticleNodeRole::SpriteRenderer => return None,
        };
        if slot.is_some() {
            return None;
        }
        if role == ParticleNodeRole::Emitter {
            if node.bypassed || has_particle_input(definition, node.id) {
                return None;
            }
            *slot = Some(node.id);
            break;
        }
        if !node.bypassed {
            *slot = Some(node.id);
        }
        downstream_node_id = node.id;
    }
    Some(CompiledParticleDefinition {
        emitter_node_id: stages.emitter?,
        initialize_node_id: stages.initialize,
        gravity_node_id: stages.gravity,
        drag_node_id: stages.drag,
        renderer_node_id,
        // A fused executable is owned by this concrete renderer chain. Two
        // branches from one Emitter must never evict each other's SSBO state.
        state_slot_id: renderer_node_id,
    })
}

fn single_particle_source(
    definition: &ModuleDefinition,
    target_node_id: uuid::Uuid,
) -> Option<&Node> {
    let target = ModulePortAddress {
        node_id: target_node_id,
        port: PARTICLE_SYSTEM_PORT.to_string(),
    };
    let mut incoming = definition
        .graph
        .connections
        .iter()
        .filter(|connection| connection.to == target);
    let source = incoming.next()?;
    if incoming.next().is_some() || source.from.port != PARTICLE_SYSTEM_PORT {
        return None;
    }
    definition.graph.nodes.get(&source.from.node_id)
}

fn has_particle_input(definition: &ModuleDefinition, node_id: uuid::Uuid) -> bool {
    definition.graph.connections.iter().any(|connection| {
        connection.to.node_id == node_id && connection.to.port == PARTICLE_SYSTEM_PORT
    })
}

fn native_role(node: &Node) -> Option<ParticleNodeRole> {
    match node.content() {
        NodeContent::NativeOperation(operation) => {
            ParticleNodeRole::from_catalog_id(&operation.catalog_id)
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::core::render_plan::compiler::compile_module;
    use crate::editor::ParticleNodeClipFactory;
    use crate::model::node::NodeContent;
    use crate::model::project::property::{Property, PropertyValue};

    #[test]
    fn particle_topology_compiles_once_at_the_definition_boundary() {
        let fixture = ParticleNodeClipFactory::create("Particles").expect("fixture");
        let compiled = compile_module(&fixture.definition).expect("compiled");
        let particle = compiled
            .particle_renderers
            .values()
            .next()
            .expect("particle executable");
        assert_eq!(particle.state_slot_id, particle.renderer_node_id);
        assert_eq!(compiled.particle_renderers.len(), 1);
        assert_eq!(compiled.nodes.len(), 5);
    }

    #[test]
    fn simulation_property_expression_is_rejected_instead_of_replayed_incorrectly() {
        let mut fixture = ParticleNodeClipFactory::create("Particles").expect("fixture");
        let emitter = fixture
            .definition
            .graph
            .nodes
            .values_mut()
            .find(|node| {
                matches!(
                    node.content(),
                    NodeContent::NativeOperation(operation)
                        if operation.catalog_id == "native.particle.emitter"
                )
            })
            .expect("emitter");
        emitter
            .set_property(
                "rate".to_string(),
                Property::expression(
                    "time * 100.0".to_string(),
                    PropertyValue::Number(ordered_float::OrderedFloat(120.0)),
                ),
            )
            .expect("known property");

        let error = compile_module(&fixture.definition).unwrap_err();
        assert!(error.contains("must remain constant"));
        assert!(error.contains("fixed-step parameter schedule"));
    }
}
