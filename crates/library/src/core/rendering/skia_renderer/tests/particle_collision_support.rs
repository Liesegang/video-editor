use super::*;
use crate::model::point::{PointAttributeSchema, PointInstruction, PointRenderProgram};
use crate::model::property::{ColorValue, PropertyValue};
use crate::rendering::scene_runtime::{PointFieldReadback, PointInvocationStats};

pub(super) const STEP_SECONDS: f32 = 1.0 / 120.0;

pub(super) fn transparent() -> Color {
    Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    }
}

pub(super) fn color_program(color: Color) -> PointRenderProgram {
    PointRenderProgram {
        schema: PointAttributeSchema::new(Vec::new()).unwrap(),
        instructions: vec![PointInstruction::Constant {
            value: PropertyValue::ColorValue(ColorValue::from_straight_srgba8(&color)),
        }],
        ramps: Vec::new(),
        color_register: 0,
        position_register: None,
        size_register: None,
        sprite_selection_register: None,
    }
}

pub(super) fn deterministic_scene(
    target_step: u64,
    position: [f64; 3],
    velocity: [f64; 3],
) -> PointSceneFrame {
    let mut scene = particle_scene(target_step);
    scene.point_program = Some(color_program(Color::white()));
    let parameters = particle_parameters_mut(&mut scene);
    parameters.capacity = 64;
    parameters.emission_rate = 120.0.into();
    parameters.lifetime_seconds = 10.0.into();
    parameters.emitter_position = particle_vec3(position[0], position[1], position[2]);
    parameters.velocity_min = particle_vec3(velocity[0], velocity[1], velocity[2]);
    parameters.velocity_max = particle_vec3(velocity[0], velocity[1], velocity[2]);
    parameters.forces.clear();
    parameters.collisions.clear();
    parameters.size_min = 4.0.into();
    parameters.size_max = 4.0.into();
    scene
}

pub(super) fn fields(renderer: &SkiaRenderer, scene: &PointSceneFrame) -> Vec<PointFieldReadback> {
    renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .read_point_fields(&scene.invocation)
        .unwrap()
}

pub(super) fn serial(fields: &[PointFieldReadback], serial: u32) -> &PointFieldReadback {
    fields
        .iter()
        .find(|point| point.serial == serial)
        .unwrap_or_else(|| panic!("missing live Particle serial {serial}: {fields:?}"))
}

pub(super) fn stats(renderer: &SkiaRenderer, scene: &PointSceneFrame) -> PointInvocationStats {
    renderer
        .scene_runtime
        .as_ref()
        .unwrap()
        .invocation_stats(&scene.invocation)
        .unwrap()
}

pub(super) fn assert_vec3_near(actual: [f32; 3], expected: [f32; 3], tolerance: f32, label: &str) {
    for (axis, (actual, expected)) in actual.into_iter().zip(expected).enumerate() {
        assert!(
            (actual - expected).abs() <= tolerance,
            "{label}[{axis}] expected {expected}, got {actual}"
        );
    }
}
