//! Compilation of the bounded executable Particle Node chain.

use std::collections::HashSet;

use crate::model::authoring::{ModuleDefinition, ModulePortAddress};
use crate::model::frame::particle::PARTICLE_MAX_FORCES;
use crate::model::node::{Node, NodeContent, PARTICLE_SYSTEM_PORT, ParticleNodeRole};

use super::{CompiledParticleForce, CompiledParticleSource};

pub(super) struct ParticleSourceCompilation {
    pub source: CompiledParticleSource,
    pub lineage: HashSet<ModulePortAddress>,
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
pub(super) fn compile_particle_source(
    definition: &ModuleDefinition,
    first_source: &ModulePortAddress,
) -> Result<Option<ParticleSourceCompilation>, String> {
    let mut stages = ParticleStages::default();
    let mut downstream_rank = ParticleNodeRole::SpriteRenderer.execution_rank();
    let mut downstream_node_id = first_source.node_id;
    let mut visited = HashSet::new();
    let mut force_count = 0_usize;
    let mut first_source = Some(first_source.clone());
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
    Ok(Some(ParticleSourceCompilation {
        source: CompiledParticleSource {
            emitter_node_id,
            shape_location_node_id: stages.shape_location,
            initialize_node_id: stages.initialize,
            force_nodes: stages.forces,
        },
        lineage: particle_lineage,
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
        let point_renderer = compiled
            .point_renderers
            .values()
            .next()
            .expect("Point executable");
        assert_eq!(
            point_renderer.state_slot_id,
            point_renderer.renderer_node_id
        );
        assert!(matches!(
            &point_renderer.source,
            crate::core::render_plan::CompiledPointSource::Particle(_)
        ));
        assert_eq!(compiled.point_renderers.len(), 1);
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
