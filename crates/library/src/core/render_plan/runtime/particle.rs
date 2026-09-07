//! RenderPlan evaluation of a compiled Particle Module invocation.

use ordered_float::OrderedFloat;

use super::frame_values::{
    finite_f32, required_number, required_string, required_u32, required_vec3,
};
use super::*;
use crate::core::render_plan::CompiledParticleSource;
use crate::model::frame::particle::{
    ParticleCollider, ParticleCollisionMode, ParticleEmitterShape, ParticleForce,
    ParticleSceneParameters,
};
use crate::model::frame::point::PointSceneSource;
use crate::model::node::ParticleNodeRole;
use crate::model::property::Vec3;

impl ModuleImageRuntime<'_> {
    pub(super) fn sample_particle_source(
        &mut self,
        particle: &CompiledParticleSource,
    ) -> Result<PointSceneSource, LibraryError> {
        let emitter = self.particle_node_values(particle.emitter_node_id)?;
        let initialize = particle
            .initialize_node_id
            .map(|node_id| self.particle_node_values(node_id))
            .transpose()?;
        let shape_location = particle
            .shape_location_node_id
            .map(|node_id| self.particle_node_values(node_id))
            .transpose()?;
        let forces = particle
            .force_nodes
            .iter()
            .map(|force| {
                self.particle_node_values(force.node_id)
                    .and_then(|values| particle_force(force.role, &values))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let collisions = particle
            .collision_nodes
            .iter()
            .map(|collision| {
                self.particle_node_values(collision.node_id)
                    .and_then(|values| particle_collision(collision.role, &values))
            })
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect();
        let capacity = required_u32(&emitter, "capacity", "Particle Emitter")?;
        let seed = required_u32(&emitter, "seed", "Particle Emitter")?;
        let parameters = ParticleSceneParameters {
            capacity,
            emission_rate: finite_f32(
                required_number(&emitter, "rate", "Particle Emitter")?,
                "emission rate",
            )?,
            lifetime_seconds: finite_f32(
                required_number(&emitter, "lifetime", "Particle Emitter")?,
                "lifetime",
            )?,
            seed,
            emitter_shape: optional_emitter_shape(shape_location.as_ref())?,
            emitter_position: optional_vec3(
                shape_location.as_ref(),
                "position",
                "Emitter Shape",
                neutral_vec3(),
            )?,
            emitter_radius: optional_f32(
                shape_location.as_ref(),
                "radius",
                "Emitter Shape",
                0.0,
                "emitter radius",
            )?,
            emitter_size: optional_vec3(
                shape_location.as_ref(),
                "size",
                "Emitter Shape",
                neutral_vec3(),
            )?,
            emitter_surface_only: optional_bool(
                shape_location.as_ref(),
                "surface_only",
                "Emitter Shape",
                false,
            )?,
            velocity_min: optional_vec3(
                initialize.as_ref(),
                "velocity_min",
                "Birth Attributes",
                neutral_vec3(),
            )?,
            velocity_max: optional_vec3(
                initialize.as_ref(),
                "velocity_max",
                "Birth Attributes",
                neutral_vec3(),
            )?,
            forces,
            collisions,
            size_min: optional_f32(
                initialize.as_ref(),
                "size_min",
                "Birth Attributes",
                1.0,
                "minimum size",
            )?,
            size_max: optional_f32(
                initialize.as_ref(),
                "size_max",
                "Birth Attributes",
                1.0,
                "maximum size",
            )?,
        };
        Ok(PointSceneSource::Particle {
            target_step: ParticleSceneParameters::target_step_for_time(self.local_time)
                .map_err(LibraryError::Validation)?,
            parameters,
        })
    }

    fn particle_node_values(
        &mut self,
        node_id: uuid::Uuid,
    ) -> Result<HashMap<String, PropertyValue>, LibraryError> {
        let node = self.particle_node(node_id)?;
        self.node_values(&node)
    }

    fn particle_node(&self, node_id: uuid::Uuid) -> Result<CompiledNode, LibraryError> {
        self.definition.nodes.get(&node_id).cloned().ok_or_else(|| {
            LibraryError::Validation(format!(
                "Compiled Particle executable reaches missing Node {node_id}"
            ))
        })
    }
}

fn neutral_vec3() -> Vec3 {
    Vec3 {
        x: OrderedFloat(0.0),
        y: OrderedFloat(0.0),
        z: OrderedFloat(0.0),
    }
}

fn particle_force(
    role: ParticleNodeRole,
    values: &HashMap<String, PropertyValue>,
) -> Result<ParticleForce, LibraryError> {
    match role {
        ParticleNodeRole::Gravity => Ok(ParticleForce::Gravity {
            acceleration: required_vec3(values, "force", "Gravity Force")?,
        }),
        ParticleNodeRole::Drag => Ok(ParticleForce::Drag {
            coefficient: required_f32(values, "coefficient", "Drag Force", "drag")?,
        }),
        ParticleNodeRole::Turbulence => Ok(ParticleForce::Turbulence {
            strength: required_f32(values, "strength", "Turbulence", "turbulence strength")?,
            frequency: required_f32(values, "frequency", "Turbulence", "turbulence frequency")?,
            octaves: required_u32(values, "octaves", "Turbulence")?,
            evolution: required_f32(values, "evolution", "Turbulence", "turbulence evolution")?,
            seed: required_u32(values, "seed", "Turbulence")?,
        }),
        ParticleNodeRole::Vortex => Ok(ParticleForce::Vortex {
            axis: required_vec3(values, "axis", "Vortex Force")?,
            center: required_vec3(values, "center", "Vortex Force")?,
            strength: required_f32(values, "strength", "Vortex Force", "vortex strength")?,
        }),
        ParticleNodeRole::Point => Ok(ParticleForce::Point {
            target: required_vec3(values, "target", "Point Force")?,
            strength: required_f32(values, "strength", "Point Force", "point force strength")?,
            radius: required_f32(values, "radius", "Point Force", "point force radius")?,
            falloff: required_f32(values, "falloff", "Point Force", "point force falloff")?,
        }),
        ParticleNodeRole::Emitter
        | ParticleNodeRole::ShapeLocation
        | ParticleNodeRole::Initialize
        | ParticleNodeRole::CollisionPlane
        | ParticleNodeRole::CollisionSphere
        | ParticleNodeRole::SpriteRenderer => Err(LibraryError::Validation(format!(
            "Particle executable contains non-force role {role:?} in its force list"
        ))),
    }
}

fn particle_collision(
    role: ParticleNodeRole,
    values: &HashMap<String, PropertyValue>,
) -> Result<Option<ParticleCollider>, LibraryError> {
    let node = match role {
        ParticleNodeRole::CollisionPlane => "Collision Plane",
        ParticleNodeRole::CollisionSphere => "Collision Sphere",
        _ => {
            return Err(LibraryError::Validation(format!(
                "Particle executable contains non-collision role {role:?} in its collision list"
            )));
        }
    };
    let active = match values.get("active") {
        Some(PropertyValue::Boolean(value)) => *value,
        _ => {
            return Err(frame_values::type_error(
                &format!("{node} active"),
                "Boolean",
            ));
        }
    };
    if !active {
        return Ok(None);
    }
    let collider = match role {
        ParticleNodeRole::CollisionPlane => ParticleCollider::Plane {
            plane_point: required_vec3(values, "plane_point", node)?,
            plane_normal: required_vec3(values, "plane_normal", node)?,
            radius: required_f32(values, "radius", node, "collision radius")?,
            bounce: required_f32(values, "bounce", node, "collision bounce")?,
            friction: required_f32(values, "friction", node, "collision friction")?,
        },
        ParticleNodeRole::CollisionSphere => ParticleCollider::Sphere {
            center: required_vec3(values, "center", node)?,
            radius: required_f32(values, "radius", node, "collision sphere radius")?,
            particle_radius: required_f32(
                values,
                "particle_radius",
                node,
                "collision sphere particle radius",
            )?,
            mode: match required_string(values, "mode", node)?.as_str() {
                "Solid" => ParticleCollisionMode::Solid,
                "Container" => ParticleCollisionMode::Container,
                value => {
                    return Err(LibraryError::Validation(format!(
                        "{node} has unknown mode '{value}'"
                    )));
                }
            },
            bounce: required_f32(values, "bounce", node, "collision bounce")?,
            friction: required_f32(values, "friction", node, "collision friction")?,
        },
        _ => {
            return Err(LibraryError::Validation(format!(
                "Particle executable contains non-collision role {role:?} in its collision list"
            )));
        }
    };
    Ok(Some(collider))
}

fn optional_emitter_shape(
    values: Option<&HashMap<String, PropertyValue>>,
) -> Result<ParticleEmitterShape, LibraryError> {
    let Some(values) = values else {
        return Ok(ParticleEmitterShape::Point);
    };
    match required_string(values, "shape", "Emitter Shape")?.as_str() {
        "Point" => Ok(ParticleEmitterShape::Point),
        "Box" => Ok(ParticleEmitterShape::Box),
        "Sphere" => Ok(ParticleEmitterShape::Sphere),
        value => Err(LibraryError::Validation(format!(
            "Emitter Shape has unknown shape '{value}'"
        ))),
    }
}

fn optional_bool(
    values: Option<&HashMap<String, PropertyValue>>,
    key: &str,
    owner: &str,
    neutral: bool,
) -> Result<bool, LibraryError> {
    let Some(values) = values else {
        return Ok(neutral);
    };
    match values.get(key) {
        Some(PropertyValue::Boolean(value)) => Ok(*value),
        _ => Err(frame_values::type_error(
            &format!("{owner} {key}"),
            "Boolean",
        )),
    }
}

fn optional_vec3(
    values: Option<&HashMap<String, PropertyValue>>,
    key: &str,
    owner: &str,
    neutral: Vec3,
) -> Result<Vec3, LibraryError> {
    values.map_or(Ok(neutral), |values| required_vec3(values, key, owner))
}

fn optional_f32(
    values: Option<&HashMap<String, PropertyValue>>,
    key: &str,
    owner: &str,
    neutral: f64,
    label: &str,
) -> Result<OrderedFloat<f32>, LibraryError> {
    finite_f32(
        values.map_or(Ok(neutral), |values| required_number(values, key, owner))?,
        label,
    )
}

fn required_f32(
    values: &HashMap<String, PropertyValue>,
    key: &str,
    owner: &str,
    label: &str,
) -> Result<OrderedFloat<f32>, LibraryError> {
    finite_f32(required_number(values, key, owner)?, label)
}
