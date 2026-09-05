//! Authoritative factory for the first executable GPU Particle Node Clip.
//!
//! Inspector controls are published views over the same private definition
//! shown by the production Node Editor. There is deliberately no parallel
//! particle settings document.

use std::collections::HashMap;

use crate::editor::timeline_editor_service::{ModuleItemPlacement, TimelineEditorService};
use crate::error::LibraryError;
use crate::model::BlendMode;
use crate::model::authoring::{
    ChangeSet, ModuleConnection, ModuleConnectionId, ModuleDefinition, ModuleDefinitionId,
    ModuleDefinitionSharing, ModuleInstanceId, ModuleOutputId, ModulePortAddress,
    PublishedParameter, PublishedParameterId, TimelineInterval, TimelineItemId, TimelineTrackId,
};
use crate::model::node::{Node, PARTICLE_SYSTEM_PORT, ParticleNodeRole};
use crate::model::project::{IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, PortDataType};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParticlePublishedParameters {
    pub capacity: PublishedParameterId,
    pub emission_rate: PublishedParameterId,
    pub lifetime: PublishedParameterId,
    pub seed: PublishedParameterId,
    pub velocity_min: PublishedParameterId,
    pub velocity_max: PublishedParameterId,
    pub size_min: PublishedParameterId,
    pub size_max: PublishedParameterId,
    pub gravity: PublishedParameterId,
    pub drag: PublishedParameterId,
    pub color: PublishedParameterId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParticleNodeClipDefinition {
    pub definition: ModuleDefinition,
    pub output_id: ModuleOutputId,
    pub parameters: ParticlePublishedParameters,
}

/// Timeline-owned placement for one explicitly requested Particle Node Clip.
/// Particle settings remain published Module parameters; this carries only
/// ordinary placement state and therefore cannot become a second settings
/// document.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParticleNodeClipPlacement {
    pub track_id: TimelineTrackId,
    pub name: String,
    pub interval: TimelineInterval,
    pub layer: i64,
}

/// Stable identities created by one atomic Particle Node Clip edit.
#[derive(Clone, Debug, PartialEq)]
pub struct ParticleNodeClipCreation {
    pub item_id: TimelineItemId,
    pub definition_id: ModuleDefinitionId,
    pub instance_id: ModuleInstanceId,
    pub output_id: ModuleOutputId,
    pub parameters: ParticlePublishedParameters,
    pub changes: ChangeSet,
}

pub struct ParticleNodeClipFactory;

impl ParticleNodeClipFactory {
    pub fn create(name: impl Into<String>) -> Result<ParticleNodeClipDefinition, LibraryError> {
        let (mut definition, output_id) =
            ModuleDefinition::new_image(name, ModuleDefinitionSharing::Private);
        let output_node_id = definition
            .output(output_id)
            .ok_or_else(|| {
                LibraryError::Validation("Particle Module lost its Output terminal".to_string())
            })?
            .node_id;

        let mut emitter = Node::new_catalog_node(ParticleNodeRole::Emitter.catalog_id())
            .map_err(LibraryError::Validation)?;
        emitter.ui_position = [0.0, 140.0];
        let mut initialize = Node::new_catalog_node(ParticleNodeRole::Initialize.catalog_id())
            .map_err(LibraryError::Validation)?;
        initialize.ui_position = [280.0, 140.0];
        let mut gravity = Node::new_catalog_node(ParticleNodeRole::Gravity.catalog_id())
            .map_err(LibraryError::Validation)?;
        gravity.ui_position = [560.0, 70.0];
        let mut drag = Node::new_catalog_node(ParticleNodeRole::Drag.catalog_id())
            .map_err(LibraryError::Validation)?;
        drag.ui_position = [560.0, 260.0];
        let mut renderer = Node::new_catalog_node(ParticleNodeRole::SpriteRenderer.catalog_id())
            .map_err(LibraryError::Validation)?;
        renderer.ui_position = [840.0, 140.0];
        if let Some(output) = definition.graph.nodes.get_mut(&output_node_id) {
            output.ui_position = [1_120.0, 140.0];
        }

        let emitter_id = emitter.id;
        let initialize_id = initialize.id;
        let gravity_id = gravity.id;
        let drag_id = drag.id;
        let renderer_id = renderer.id;
        definition.graph.nodes.extend([
            (emitter_id, emitter),
            (initialize_id, initialize),
            (gravity_id, gravity),
            (drag_id, drag),
            (renderer_id, renderer),
        ]);
        definition.graph.connections = vec![
            connection(
                emitter_id,
                PARTICLE_SYSTEM_PORT,
                initialize_id,
                PARTICLE_SYSTEM_PORT,
            ),
            connection(
                initialize_id,
                PARTICLE_SYSTEM_PORT,
                gravity_id,
                PARTICLE_SYSTEM_PORT,
            ),
            connection(
                gravity_id,
                PARTICLE_SYSTEM_PORT,
                drag_id,
                PARTICLE_SYSTEM_PORT,
            ),
            connection(
                drag_id,
                PARTICLE_SYSTEM_PORT,
                renderer_id,
                PARTICLE_SYSTEM_PORT,
            ),
            connection(
                renderer_id,
                IMAGE_OUTPUT_PORT,
                output_node_id,
                IMAGE_INPUT_PORT,
            ),
        ];

        let capacity = publish(
            &mut definition,
            emitter_id,
            "capacity",
            "Capacity",
            PortDataType::Integer,
        )?;
        let emission_rate = publish(
            &mut definition,
            emitter_id,
            "rate",
            "Emission Rate",
            PortDataType::Number,
        )?;
        let lifetime = publish(
            &mut definition,
            emitter_id,
            "lifetime",
            "Lifetime",
            PortDataType::Number,
        )?;
        let seed = publish(
            &mut definition,
            emitter_id,
            "seed",
            "Seed",
            PortDataType::Integer,
        )?;
        let velocity_min = publish(
            &mut definition,
            initialize_id,
            "velocity_min",
            "Velocity Min",
            PortDataType::Vec3,
        )?;
        let velocity_max = publish(
            &mut definition,
            initialize_id,
            "velocity_max",
            "Velocity Max",
            PortDataType::Vec3,
        )?;
        let size_min = publish(
            &mut definition,
            initialize_id,
            "size_min",
            "Size Min",
            PortDataType::Number,
        )?;
        let size_max = publish(
            &mut definition,
            initialize_id,
            "size_max",
            "Size Max",
            PortDataType::Number,
        )?;
        let gravity_parameter = publish(
            &mut definition,
            gravity_id,
            "force",
            "Gravity",
            PortDataType::Vec3,
        )?;
        let drag_parameter = publish(
            &mut definition,
            drag_id,
            "coefficient",
            "Drag",
            PortDataType::Number,
        )?;
        let color = publish(
            &mut definition,
            renderer_id,
            "color",
            "Color",
            PortDataType::Color,
        )?;
        definition.topology_revision = 2;
        definition.interface_version = 2;
        definition.validate().map_err(LibraryError::Validation)?;

        Ok(ParticleNodeClipDefinition {
            definition,
            output_id,
            parameters: ParticlePublishedParameters {
                capacity,
                emission_rate,
                lifetime,
                seed,
                velocity_min,
                velocity_max,
                size_min,
                size_max,
                gravity: gravity_parameter,
                drag: drag_parameter,
                color,
            },
        })
    }
}

impl TimelineEditorService {
    /// Creates exactly one private Particle Module and places exactly one
    /// Timeline item that invokes it. Ordinary clips and sibling items remain
    /// ordinary Timeline sources and are never expanded into Nodes.
    pub fn create_particle_node_clip(
        &self,
        placement: ParticleNodeClipPlacement,
    ) -> Result<ParticleNodeClipCreation, LibraryError> {
        let particle = ParticleNodeClipFactory::create(placement.name.clone())?;
        let definition_id = particle.definition.id;
        let output_id = particle.output_id;
        let parameters = particle.parameters;
        let (item_id, instance_id, changes) = self.create_private_module_item(
            particle.definition,
            ModuleItemPlacement {
                track_id: placement.track_id,
                name: placement.name,
                output_id,
                interval: placement.interval,
                layer: placement.layer,
                parameter_overrides: HashMap::new(),
                input_bindings: HashMap::new(),
            },
        )?;
        Ok(ParticleNodeClipCreation {
            item_id,
            definition_id,
            instance_id,
            output_id,
            parameters,
            changes,
        })
    }
}

fn connection(
    from_node: uuid::Uuid,
    from_port: &str,
    to_node: uuid::Uuid,
    to_port: &str,
) -> ModuleConnection {
    ModuleConnection {
        id: ModuleConnectionId::new(),
        from: ModulePortAddress {
            node_id: from_node,
            port: from_port.to_string(),
        },
        to: ModulePortAddress {
            node_id: to_node,
            port: to_port.to_string(),
        },
        order: 0,
        blend_mode: BlendMode::Normal,
    }
}

fn publish(
    definition: &mut ModuleDefinition,
    node_id: uuid::Uuid,
    property: &str,
    name: &str,
    data_type: PortDataType,
) -> Result<PublishedParameterId, LibraryError> {
    let default_value = definition
        .graph
        .nodes
        .get(&node_id)
        .and_then(|node| node.properties().get(property))
        .and_then(|property| property.value())
        .cloned()
        .ok_or_else(|| {
            LibraryError::Validation(format!(
                "Particle Node {node_id} has no authored '{property}' default"
            ))
        })?;
    let id = PublishedParameterId::new();
    definition.interface.parameters.push(PublishedParameter {
        id,
        name: name.to_string(),
        data_type,
        default_value,
        target: ModulePortAddress {
            node_id,
            port: property.to_string(),
        },
    });
    Ok(id)
}

#[cfg(test)]
#[path = "particle_node_clip_tests.rs"]
mod tests;
