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
use crate::model::property::{Property, PropertyValue};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParticlePublishedParameters {
    pub capacity: PublishedParameterId,
    pub emission_rate: PublishedParameterId,
    pub lifetime: PublishedParameterId,
    pub seed: PublishedParameterId,
    pub emitter_shape: PublishedParameterId,
    pub emitter_position: PublishedParameterId,
    pub emitter_radius: PublishedParameterId,
    pub emitter_size: PublishedParameterId,
    pub emitter_surface_only: PublishedParameterId,
    pub velocity_min: PublishedParameterId,
    pub velocity_max: PublishedParameterId,
    pub size_min: PublishedParameterId,
    pub size_max: PublishedParameterId,
    pub gravity: PublishedParameterId,
    pub turbulence_strength: PublishedParameterId,
    pub turbulence_frequency: PublishedParameterId,
    pub turbulence_octaves: PublishedParameterId,
    pub turbulence_evolution: PublishedParameterId,
    pub turbulence_seed: PublishedParameterId,
    pub drag: PublishedParameterId,
    pub collision_active: PublishedParameterId,
    pub collision_plane_point: PublishedParameterId,
    pub collision_plane_normal: PublishedParameterId,
    pub collision_radius: PublishedParameterId,
    pub collision_bounce: PublishedParameterId,
    pub collision_friction: PublishedParameterId,
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
        let mut shape_location =
            Node::new_catalog_node(ParticleNodeRole::ShapeLocation.catalog_id())
                .map_err(LibraryError::Validation)?;
        let mut initialize = Node::new_catalog_node(ParticleNodeRole::Initialize.catalog_id())
            .map_err(LibraryError::Validation)?;
        let mut gravity = Node::new_catalog_node(ParticleNodeRole::Gravity.catalog_id())
            .map_err(LibraryError::Validation)?;
        let mut turbulence = Node::new_catalog_node(ParticleNodeRole::Turbulence.catalog_id())
            .map_err(LibraryError::Validation)?;
        let mut drag = Node::new_catalog_node(ParticleNodeRole::Drag.catalog_id())
            .map_err(LibraryError::Validation)?;
        let mut collision = Node::new_catalog_node(ParticleNodeRole::CollisionPlane.catalog_id())
            .map_err(LibraryError::Validation)?;
        collision
            .set_property(
                "active".to_string(),
                Property::constant(PropertyValue::Boolean(false)),
            )
            .map_err(LibraryError::Validation)?;
        let mut renderer = Node::new_catalog_node(ParticleNodeRole::SpriteRenderer.catalog_id())
            .map_err(LibraryError::Validation)?;
        // Vec3 rows exceed the generic 240px header minimum. Author both the
        // presentation size and placement so Fit and Clean Layout share the
        // same usable bounds instead of reintroducing overlapping controls.
        for (index, node) in [
            &mut emitter,
            &mut shape_location,
            &mut initialize,
            &mut gravity,
            &mut turbulence,
            &mut drag,
            &mut collision,
            &mut renderer,
        ]
        .into_iter()
        .chain(definition.graph.nodes.get_mut(&output_node_id))
        .enumerate()
        {
            node.ui_size[0] = crate::model::node::PROPERTY_NODE_UI_WIDTH;
            node.ui_position = [
                index as f32
                    * (crate::model::node::PROPERTY_NODE_UI_WIDTH
                        + crate::model::node::NODE_LAYOUT_COLUMN_GAP),
                140.0,
            ];
        }

        let emitter_id = emitter.id;
        let shape_location_id = shape_location.id;
        let initialize_id = initialize.id;
        let gravity_id = gravity.id;
        let turbulence_id = turbulence.id;
        let drag_id = drag.id;
        let collision_id = collision.id;
        let renderer_id = renderer.id;
        definition.graph.nodes.extend([
            (emitter_id, emitter),
            (shape_location_id, shape_location),
            (initialize_id, initialize),
            (gravity_id, gravity),
            (turbulence_id, turbulence),
            (drag_id, drag),
            (collision_id, collision),
            (renderer_id, renderer),
        ]);
        definition.graph.connections = vec![
            connection(
                emitter_id,
                PARTICLE_SYSTEM_PORT,
                shape_location_id,
                PARTICLE_SYSTEM_PORT,
            ),
            connection(
                shape_location_id,
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
                turbulence_id,
                PARTICLE_SYSTEM_PORT,
            ),
            connection(
                turbulence_id,
                PARTICLE_SYSTEM_PORT,
                drag_id,
                PARTICLE_SYSTEM_PORT,
            ),
            connection(
                drag_id,
                PARTICLE_SYSTEM_PORT,
                collision_id,
                PARTICLE_SYSTEM_PORT,
            ),
            connection(
                collision_id,
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
        let emitter_shape = publish(
            &mut definition,
            shape_location_id,
            "shape",
            "Emitter Shape",
            PortDataType::String,
        )?;
        let emitter_position = publish(
            &mut definition,
            shape_location_id,
            "position",
            "Emitter Position",
            PortDataType::Vec3,
        )?;
        let emitter_radius = publish(
            &mut definition,
            shape_location_id,
            "radius",
            "Emitter Radius",
            PortDataType::Number,
        )?;
        let emitter_size = publish(
            &mut definition,
            shape_location_id,
            "size",
            "Emitter Size",
            PortDataType::Vec3,
        )?;
        let emitter_surface_only = publish(
            &mut definition,
            shape_location_id,
            "surface_only",
            "Emitter Surface Only",
            PortDataType::Boolean,
        )?;
        let velocity_min = publish(
            &mut definition,
            initialize_id,
            "velocity_min",
            "Birth Velocity Min",
            PortDataType::Vec3,
        )?;
        let velocity_max = publish(
            &mut definition,
            initialize_id,
            "velocity_max",
            "Birth Velocity Max",
            PortDataType::Vec3,
        )?;
        let size_min = publish(
            &mut definition,
            initialize_id,
            "size_min",
            "Birth Size Min",
            PortDataType::Number,
        )?;
        let size_max = publish(
            &mut definition,
            initialize_id,
            "size_max",
            "Birth Size Max",
            PortDataType::Number,
        )?;
        let gravity_parameter = publish(
            &mut definition,
            gravity_id,
            "force",
            "Gravity",
            PortDataType::Vec3,
        )?;
        let turbulence_strength = publish(
            &mut definition,
            turbulence_id,
            "strength",
            "Turbulence Strength",
            PortDataType::Number,
        )?;
        let turbulence_frequency = publish(
            &mut definition,
            turbulence_id,
            "frequency",
            "Turbulence Frequency",
            PortDataType::Number,
        )?;
        let turbulence_octaves = publish(
            &mut definition,
            turbulence_id,
            "octaves",
            "Turbulence Octaves",
            PortDataType::Integer,
        )?;
        let turbulence_evolution = publish(
            &mut definition,
            turbulence_id,
            "evolution",
            "Turbulence Evolution",
            PortDataType::Number,
        )?;
        let turbulence_seed = publish(
            &mut definition,
            turbulence_id,
            "seed",
            "Turbulence Seed",
            PortDataType::Integer,
        )?;
        let drag_parameter = publish(
            &mut definition,
            drag_id,
            "coefficient",
            "Drag",
            PortDataType::Number,
        )?;
        let collision_active = publish(
            &mut definition,
            collision_id,
            "active",
            "Collision Enabled",
            PortDataType::Boolean,
        )?;
        let collision_plane_point = publish(
            &mut definition,
            collision_id,
            "plane_point",
            "Plane Point",
            PortDataType::Vec3,
        )?;
        let collision_plane_normal = publish(
            &mut definition,
            collision_id,
            "plane_normal",
            "Plane Normal",
            PortDataType::Vec3,
        )?;
        let collision_radius = publish(
            &mut definition,
            collision_id,
            "radius",
            "Radius",
            PortDataType::Number,
        )?;
        let collision_bounce = publish(
            &mut definition,
            collision_id,
            "bounce",
            "Bounce",
            PortDataType::Number,
        )?;
        let collision_friction = publish(
            &mut definition,
            collision_id,
            "friction",
            "Friction",
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
                emitter_shape,
                emitter_position,
                emitter_radius,
                emitter_size,
                emitter_surface_only,
                velocity_min,
                velocity_max,
                size_min,
                size_max,
                gravity: gravity_parameter,
                turbulence_strength,
                turbulence_frequency,
                turbulence_octaves,
                turbulence_evolution,
                turbulence_seed,
                drag: drag_parameter,
                collision_active,
                collision_plane_point,
                collision_plane_normal,
                collision_radius,
                collision_bounce,
                collision_friction,
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
