//! RenderPlan evaluation of a compiled Particle Module invocation.

use ordered_float::OrderedFloat;

use super::frame_values::{required_color, required_number, required_string, transparent};
use super::*;
use crate::core::render_plan::CompiledParticleDefinition;
use crate::model::authoring::ModuleOutputId;
use crate::model::frame::particle::{
    ParticleEmitterShape, ParticleForce, ParticleSceneFrame, ParticleSceneParameters,
    SceneInvocationKey,
};
use crate::model::node::ParticleNodeRole;
use crate::model::property::Vec3;

impl ModuleImageRuntime<'_> {
    pub(super) fn evaluate_particle_renderer(
        &mut self,
        output_id: ModuleOutputId,
        particle: &CompiledParticleDefinition,
    ) -> Result<FrameItem, LibraryError> {
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
        let renderer_node = self.particle_node(particle.renderer_node_id)?;
        let point_program = particle
            .point_program
            .as_ref()
            .map(|program| self.sample_point_program(program))
            .transpose()?;
        let color = if point_program.is_some() {
            // The Point program owns per-point color. Never send a varying
            // branch through the ordinary frame-uniform property evaluator.
            crate::model::frame::color::Color::white()
        } else {
            required_color(
                &self.node_values(&renderer_node)?,
                "color",
                "Sprite Renderer",
            )?
        };
        let capacity = required_u32(&emitter, "capacity", "Particle Emitter")?;
        let seed = required_u32(&emitter, "seed", "Particle Emitter")?;
        let logical_width = u32::try_from(self.width).map_err(|_| {
            LibraryError::Validation("Particle canvas width exceeds GPU limits".to_string())
        })?;
        let logical_height = u32::try_from(self.height).map_err(|_| {
            LibraryError::Validation("Particle canvas height exceeds GPU limits".to_string())
        })?;
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
            color,
        };
        let scene = ParticleSceneFrame {
            invocation: SceneInvocationKey {
                instance_path: self.instance_path.clone(),
                module_instance_id: self.invocation.instance_id,
                state_slot_id: particle.state_slot_id,
                output_id,
            },
            random_stream_id: particle.emitter_node_id,
            executable_hash: self.definition.fingerprint,
            target_step: ParticleSceneFrame::target_step_for_time(self.local_time)
                .map_err(LibraryError::Validation)?,
            logical_width,
            logical_height,
            parameters,
            point_program,
        };
        scene.validate().map_err(LibraryError::Validation)?;
        let object = FrameItem::Object(FrameObject {
            source_node_id: particle.renderer_node_id,
            spatial_transform_node_id: None,
            spatial_transform: Box::default(),
            content_bounds: Some(FrameBounds::new(
                0.0,
                0.0,
                logical_width as f32,
                logical_height as f32,
            )),
            content: FrameContent::ParticleScene {
                scene,
                effects: Vec::new(),
                transform: Transform::default(),
            },
        });
        Ok(FrameItem::Group(FrameGroup {
            source_id: renderer_node.id,
            kind: FrameGroupKind::Node,
            width: self.width,
            height: self.height,
            background_color: transparent(),
            transform: Transform::default(),
            blend_mode: renderer_node.blend_mode,
            effect_time: OrderedFloat(self.local_time.to_seconds_f64()),
            effects: Vec::new(),
            items: vec![object],
        }))
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
        | ParticleNodeRole::SpriteRenderer => Err(LibraryError::Validation(format!(
            "Particle executable contains non-force role {role:?} in its force list"
        ))),
    }
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

fn finite_f32(value: f64, label: &str) -> Result<OrderedFloat<f32>, LibraryError> {
    let value = value as f32;
    if value.is_finite() {
        Ok(OrderedFloat(value))
    } else {
        Err(LibraryError::Validation(format!(
            "Particle {label} must fit a finite GPU float"
        )))
    }
}

fn required_u32(
    values: &HashMap<String, PropertyValue>,
    key: &str,
    owner: &str,
) -> Result<u32, LibraryError> {
    let value = match values.get(key) {
        Some(PropertyValue::Integer(value)) => *value,
        _ => {
            return Err(frame_values::type_error(
                &format!("{owner} {key}"),
                "Integer",
            ));
        }
    };
    u32::try_from(value).map_err(|_| {
        LibraryError::Validation(format!("{owner} {key} must fit an unsigned 32-bit value"))
    })
}

fn required_vec3(
    values: &HashMap<String, PropertyValue>,
    key: &str,
    owner: &str,
) -> Result<Vec3, LibraryError> {
    match values.get(key) {
        Some(PropertyValue::Vec3(value)) => Ok(*value),
        _ => Err(frame_values::type_error(&format!("{owner} {key}"), "Vec3")),
    }
}
