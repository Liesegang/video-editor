//! RenderPlan evaluation of a compiled Particle Module invocation.

use ordered_float::OrderedFloat;

use super::frame_values::{required_color, required_number};
use super::*;
use crate::core::render_plan::CompiledParticleDefinition;
use crate::model::authoring::ModuleOutputId;
use crate::model::frame::particle::{
    ParticleSceneFrame, ParticleSceneParameters, SceneInvocationKey,
};
use crate::model::property::Vec3;

impl ModuleImageRuntime<'_> {
    pub(super) fn evaluate_particle_terminal(
        &mut self,
        output_id: ModuleOutputId,
        particle: &CompiledParticleDefinition,
    ) -> Result<FrameItem, LibraryError> {
        self.reject_unsampled_simulation_automation(particle)?;
        let emitter = self.particle_node_values(particle.emitter_node_id)?;
        let initialize = self.particle_node_values(particle.initialize_node_id)?;
        let gravity = self.particle_node_values(particle.gravity_node_id)?;
        let drag = self.particle_node_values(particle.drag_node_id)?;
        let renderer = self.particle_node_values(particle.renderer_node_id)?;
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
            velocity_min: required_vec3(&initialize, "velocity_min", "Initialize Particle")?,
            velocity_max: required_vec3(&initialize, "velocity_max", "Initialize Particle")?,
            gravity: required_vec3(&gravity, "force", "Gravity Force")?,
            drag: finite_f32(required_number(&drag, "coefficient", "Drag Force")?, "drag")?,
            size_min: finite_f32(
                required_number(&initialize, "size_min", "Initialize Particle")?,
                "minimum size",
            )?,
            size_max: finite_f32(
                required_number(&initialize, "size_max", "Initialize Particle")?,
                "maximum size",
            )?,
            color: required_color(&renderer, "color", "Sprite Renderer")?,
        };
        let scene = ParticleSceneFrame {
            invocation: SceneInvocationKey {
                instance_path: self.instance_path.clone(),
                module_instance_id: self.invocation.instance_id,
                state_slot_id: particle.state_slot_id,
                output_id,
            },
            executable_hash: self.definition.fingerprint,
            target_step: ParticleSceneFrame::target_step_for_time(self.local_time)
                .map_err(LibraryError::Validation)?,
            logical_width,
            logical_height,
            parameters,
        };
        scene.validate().map_err(LibraryError::Validation)?;
        Ok(FrameItem::Object(FrameObject {
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
        }))
    }

    fn particle_node_values(
        &mut self,
        node_id: uuid::Uuid,
    ) -> Result<HashMap<String, PropertyValue>, LibraryError> {
        let node = self
            .definition
            .nodes
            .get(&node_id)
            .cloned()
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "Compiled Particle executable reaches missing Node {node_id}"
                ))
            })?;
        self.node_values(&node)
    }

    fn reject_unsampled_simulation_automation(
        &self,
        particle: &CompiledParticleDefinition,
    ) -> Result<(), LibraryError> {
        let simulation_nodes = [
            particle.emitter_node_id,
            particle.initialize_node_id,
            particle.gravity_node_id,
            particle.drag_node_id,
        ];
        for parameter_id in self.invocation.automation_tracks.keys() {
            let parameter = self
                .definition
                .parameters
                .get(parameter_id)
                .ok_or_else(|| {
                    LibraryError::Validation(format!(
                        "Particle invocation automates missing Published Parameter {parameter_id}"
                    ))
                })?;
            if simulation_nodes.contains(&parameter.target.node_id) {
                return Err(LibraryError::Render(format!(
                    "Particle simulation parameter '{}' is animated, but the first GPU slice does not yet carry a fixed-step parameter schedule; remove this automation instead of rendering an incorrect history",
                    parameter.name
                )));
            }
        }
        Ok(())
    }
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
