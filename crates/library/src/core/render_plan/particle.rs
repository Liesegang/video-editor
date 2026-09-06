//! Compilation of the bounded executable Particle Node chain.

use std::collections::{HashMap, HashSet};

use crate::model::authoring::{ModuleDefinition, ModulePortAddress};
use crate::model::frame::particle::PARTICLE_MAX_FORCES;
use crate::model::node::{Node, NodeContent, PARTICLE_SYSTEM_PORT, ParticleNodeRole};

use super::point::{compile_point_program, trace_point_stream, validate_point_field_consumers};
use super::{CompiledParticleDefinition, CompiledParticleForce};

pub(super) fn compile_particle_renderers(
    definition: &ModuleDefinition,
    active_nodes: &HashSet<uuid::Uuid>,
) -> Result<HashMap<uuid::Uuid, CompiledParticleDefinition>, String> {
    validate_point_field_consumers(definition, active_nodes)?;
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
        let Some(trace) = trace_point_stream(definition, renderer_node_id)? else {
            continue;
        };
        if let Some(particle) = compile_particle_chain(definition, renderer_node_id, &trace)? {
            compiled.insert(renderer_node_id, particle);
        }
    }
    Ok(compiled)
}

#[derive(Default)]
struct ParticleStages {
    emitter: Option<uuid::Uuid>,
    shape_location: Option<uuid::Uuid>,
    initialize: Option<uuid::Uuid>,
    forces: Vec<CompiledParticleForce>,
}

/// Compile the implemented typed stages while allowing omitted modifiers and
/// repeated force kinds (for example Emitter -> Gravity -> Drag -> Gravity ->
/// Sprite). Incomplete, disabled, unsupported, duplicate singleton, or
/// out-of-order chains are a stable no-image result while the Node Editor is
/// being rewired; they never turn a model-valid Project into a RenderPlan
/// compilation failure.
fn compile_particle_chain(
    definition: &ModuleDefinition,
    renderer_node_id: uuid::Uuid,
    point_trace: &super::point::PointStreamTrace,
) -> Result<Option<CompiledParticleDefinition>, String> {
    let mut stages = ParticleStages::default();
    let mut downstream_rank = ParticleNodeRole::SpriteRenderer.execution_rank();
    let mut downstream_node_id = renderer_node_id;
    let mut visited = HashSet::from([renderer_node_id]);
    let mut force_count = 0_usize;
    let mut first_source = Some(point_trace.particle_source.clone());
    let mut particle_lineage = HashSet::new();
    loop {
        let node = match first_source.take() {
            Some(source) => particle_source_node(definition, &source),
            None => single_particle_source(definition, downstream_node_id),
        };
        let Some(node) = node else {
            return Ok(None);
        };
        if !visited.insert(node.id) {
            return Ok(None);
        }
        if !node.enabled {
            return Ok(None);
        }
        let Some(role) = native_role(node) else {
            return Ok(None);
        };
        particle_lineage.insert(ModulePortAddress {
            node_id: node.id,
            port: PARTICLE_SYSTEM_PORT.to_string(),
        });
        let rank = role.execution_rank();
        let repeated_force = role.is_force() && rank == downstream_rank;
        if role == ParticleNodeRole::SpriteRenderer
            || rank > downstream_rank
            || (rank == downstream_rank && !repeated_force)
        {
            return Ok(None);
        }
        downstream_rank = rank;
        if role.is_force() {
            force_count += 1;
            if force_count > PARTICLE_MAX_FORCES {
                return Ok(None);
            }
            if !node.bypassed {
                stages.forces.push(CompiledParticleForce {
                    node_id: node.id,
                    role,
                });
            }
            downstream_node_id = node.id;
            continue;
        }
        let slot = match role {
            ParticleNodeRole::Emitter => &mut stages.emitter,
            ParticleNodeRole::ShapeLocation => &mut stages.shape_location,
            ParticleNodeRole::Initialize => &mut stages.initialize,
            ParticleNodeRole::Gravity
            | ParticleNodeRole::Drag
            | ParticleNodeRole::Turbulence
            | ParticleNodeRole::Vortex
            | ParticleNodeRole::Point => return Ok(None),
            ParticleNodeRole::SpriteRenderer => return Ok(None),
        };
        if slot.is_some() {
            return Ok(None);
        }
        if role == ParticleNodeRole::Emitter {
            if node.bypassed || has_particle_input(definition, node.id) {
                return Ok(None);
            }
            *slot = Some(node.id);
            break;
        }
        if !node.bypassed {
            *slot = Some(node.id);
        }
        downstream_node_id = node.id;
    }
    stages.forces.reverse();
    let Some(emitter_node_id) = stages.emitter else {
        return Ok(None);
    };
    let point_program =
        compile_point_program(definition, renderer_node_id, point_trace, &particle_lineage)?;
    Ok(Some(CompiledParticleDefinition {
        emitter_node_id,
        shape_location_node_id: stages.shape_location,
        initialize_node_id: stages.initialize,
        force_nodes: stages.forces,
        point_program,
        renderer_node_id,
        // A fused executable is owned by this concrete renderer chain. Two
        // branches from one Emitter must never evict each other's SSBO state.
        state_slot_id: renderer_node_id,
    }))
}

fn particle_source_node<'a>(
    definition: &'a ModuleDefinition,
    source: &ModulePortAddress,
) -> Option<&'a Node> {
    if source.port != PARTICLE_SYSTEM_PORT {
        return None;
    }
    definition.graph.nodes.get(&source.node_id)
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
        assert_eq!(compiled.nodes.len(), 7);
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
