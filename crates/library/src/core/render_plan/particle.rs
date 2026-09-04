//! Compilation of the bounded executable Particle Node chain.

use std::collections::HashMap;

use crate::model::authoring::{ModuleDefinition, ModuleOutputId, ModulePortAddress};
use crate::model::node::{Node, NodeContent};
use crate::model::project::{IMAGE_OUTPUT_PORT, PortDataType};
use crate::plugin::property_name_from_port;

use super::{CompiledModuleOutput, CompiledParticleDefinition};

const PARTICLES_PORT: &str = "particles";
const EMITTER: &str = "native.particle.emitter";
const INITIALIZE: &str = "native.particle.initialize";
const GRAVITY: &str = "native.particle.gravity-force";
const DRAG: &str = "native.particle.drag-force";
const SPRITE: &str = "native.particle.sprite-renderer";

pub(super) fn compile_particle_outputs(
    definition: &ModuleDefinition,
    outputs: &HashMap<ModuleOutputId, CompiledModuleOutput>,
) -> Result<HashMap<ModuleOutputId, CompiledParticleDefinition>, String> {
    let mut compiled = HashMap::new();
    for (output_id, output) in outputs {
        let Some(source) = output.source(PortDataType::Image) else {
            continue;
        };
        let Some(node) = definition.graph.nodes.get(&source.node_id) else {
            continue;
        };
        if native_identity(node) != Some(SPRITE) {
            continue;
        }
        if source.port != IMAGE_OUTPUT_PORT {
            return Err(format!(
                "Particle Sprite Node {} must reach Output through its Image port",
                node.id
            ));
        }
        require_executable(node, SPRITE)?;
        let renderer_node_id = node.id;
        let drag_node_id = require_upstream(definition, renderer_node_id, DRAG)?;
        let gravity_node_id = require_upstream(definition, drag_node_id, GRAVITY)?;
        let initialize_node_id = require_upstream(definition, gravity_node_id, INITIALIZE)?;
        let emitter_node_id = require_upstream(definition, initialize_node_id, EMITTER)?;
        if definition.graph.connections.iter().any(|connection| {
            connection.to.node_id == emitter_node_id && connection.to.port == PARTICLES_PORT
        }) {
            return Err(format!(
                "Particle Emitter Node {emitter_node_id} cannot consume a ParticleSystem"
            ));
        }
        require_static_simulation_inputs(
            definition,
            emitter_node_id,
            &["capacity", "rate", "lifetime", "seed"],
        )?;
        require_static_simulation_inputs(
            definition,
            initialize_node_id,
            &["velocity_min", "velocity_max", "size_min", "size_max"],
        )?;
        require_static_simulation_inputs(definition, gravity_node_id, &["force"])?;
        require_static_simulation_inputs(definition, drag_node_id, &["coefficient"])?;
        compiled.insert(
            *output_id,
            CompiledParticleDefinition {
                emitter_node_id,
                initialize_node_id,
                gravity_node_id,
                drag_node_id,
                renderer_node_id,
                state_slot_id: emitter_node_id,
            },
        );
    }
    Ok(compiled)
}

fn require_static_simulation_inputs(
    definition: &ModuleDefinition,
    node_id: uuid::Uuid,
    property_keys: &[&str],
) -> Result<(), String> {
    let node = definition
        .graph
        .nodes
        .get(&node_id)
        .ok_or_else(|| format!("Particle simulation reaches missing Node {node_id}"))?;
    for key in property_keys {
        let property = node.properties().get(key).ok_or_else(|| {
            format!("Particle Node {node_id} is missing required Property '{key}'")
        })?;
        if property.evaluator != "constant" {
            return Err(format!(
                "Particle simulation Property {node_id}:{key} uses '{}'; the first GPU slice accepts constant/instance values only because deterministic step-sampled automation is not implemented",
                property.evaluator
            ));
        }
    }
    if let Some(connection) = definition.graph.connections.iter().find(|connection| {
        connection.to.node_id == node_id
            && property_keys.contains(
                &property_name_from_port(&connection.to.port).unwrap_or(&connection.to.port),
            )
    }) {
        return Err(format!(
            "Particle simulation Property {}:{} is driven by a Node connection; step-sampled value inputs are not implemented in the first GPU slice",
            connection.to.node_id, connection.to.port
        ));
    }
    Ok(())
}

fn require_upstream(
    definition: &ModuleDefinition,
    target_node_id: uuid::Uuid,
    expected_catalog_id: &str,
) -> Result<uuid::Uuid, String> {
    let target = ModulePortAddress {
        node_id: target_node_id,
        port: PARTICLES_PORT.to_string(),
    };
    let mut incoming = definition
        .graph
        .connections
        .iter()
        .filter(|connection| connection.to == target);
    let source = incoming.next().ok_or_else(|| {
        format!(
            "Particle Node {target_node_id} requires one '{PARTICLES_PORT}' input from {expected_catalog_id}"
        )
    })?;
    if incoming.next().is_some() || source.from.port != PARTICLES_PORT {
        return Err(format!(
            "Particle Node {target_node_id} has an invalid ParticleSystem input"
        ));
    }
    let node = definition
        .graph
        .nodes
        .get(&source.from.node_id)
        .ok_or_else(|| "Particle chain reaches a missing Node".to_string())?;
    require_executable(node, expected_catalog_id)?;
    Ok(node.id)
}

fn require_executable(node: &Node, expected_catalog_id: &str) -> Result<(), String> {
    if native_identity(node) != Some(expected_catalog_id) {
        return Err(format!(
            "Particle chain expected {expected_catalog_id}, found Node '{}'",
            node.name
        ));
    }
    if !node.enabled || node.bypassed {
        return Err(format!(
            "Executable Particle Node {} cannot be disabled or bypassed",
            node.id
        ));
    }
    Ok(())
}

fn native_identity(node: &Node) -> Option<&str> {
    match node.content() {
        NodeContent::NativeOperation(operation) => Some(operation.catalog_id.as_str()),
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
            .particle_outputs
            .get(&fixture.output_id)
            .expect("particle executable");
        assert_eq!(particle.state_slot_id, particle.emitter_node_id);
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
        assert!(error.contains("step-sampled automation is not implemented"));
    }
}
