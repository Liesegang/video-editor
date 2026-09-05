//! RenderPlan evaluation of a compiled Particle Module invocation.

use ordered_float::OrderedFloat;

use super::frame_values::{required_color, required_number, transparent};
use super::*;
use crate::core::render_plan::CompiledParticleDefinition;
use crate::model::authoring::ModuleOutputId;
use crate::model::frame::particle::{
    ParticleSceneFrame, ParticleSceneParameters, SceneInvocationKey,
};
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
        let gravity = particle
            .gravity_node_id
            .map(|node_id| self.particle_node_values(node_id))
            .transpose()?;
        let drag = particle
            .drag_node_id
            .map(|node_id| self.particle_node_values(node_id))
            .transpose()?;
        let renderer_node = self.particle_node(particle.renderer_node_id)?;
        let renderer = self.node_values(&renderer_node)?;
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
            velocity_min: optional_vec3(
                initialize.as_ref(),
                "velocity_min",
                "Initialize Particle",
                neutral_vec3(),
            )?,
            velocity_max: optional_vec3(
                initialize.as_ref(),
                "velocity_max",
                "Initialize Particle",
                neutral_vec3(),
            )?,
            gravity: optional_vec3(gravity.as_ref(), "force", "Gravity Force", neutral_vec3())?,
            drag: optional_f32(drag.as_ref(), "coefficient", "Drag Force", 0.0, "drag")?,
            size_min: optional_f32(
                initialize.as_ref(),
                "size_min",
                "Initialize Particle",
                1.0,
                "minimum size",
            )?,
            size_max: optional_f32(
                initialize.as_ref(),
                "size_max",
                "Initialize Particle",
                1.0,
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
            random_stream_id: particle.emitter_node_id,
            executable_hash: self.definition.fingerprint,
            target_step: ParticleSceneFrame::target_step_for_time(self.local_time)
                .map_err(LibraryError::Validation)?,
            logical_width,
            logical_height,
            parameters,
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
