use ordered_float::OrderedFloat;

use super::*;
use crate::core::render_plan::evaluate_render_plan_frame;
use crate::model::frame::entity::{FrameContent, FrameItem};
use crate::model::frame::particle::ParticleCollider;
use crate::model::frame::point::{PointSceneFrame, PointSceneSource};
use crate::model::property::{PropertyValue, Vec3};

fn vec3(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
        z: OrderedFloat(z),
    }
}

fn collision_export_project() -> Arc<AuthoringProject> {
    let project = particle_export_project();
    let definition = project.module_definitions.values().next().unwrap();
    let parameter = |name: &str| {
        definition
            .interface
            .parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .unwrap_or_else(|| panic!("Particle factory did not publish {name}"))
            .id
    };
    let active = parameter("Collision Enabled");
    let point = parameter("Plane Point");
    let normal = parameter("Plane Normal");
    let radius = parameter("Radius");
    let bounce = parameter("Bounce");
    let friction = parameter("Friction");
    let instance_id = *project.module_instances.keys().next().unwrap();
    let service = TimelineEditorService::new(project.as_ref().clone()).unwrap();
    for (parameter_id, value) in [
        (active, PropertyValue::Boolean(true)),
        (point, PropertyValue::Vec3(vec3(0.0, 20.0, 0.0))),
        (normal, PropertyValue::Vec3(vec3(0.0, -1.0, 0.0))),
        (radius, PropertyValue::Number(OrderedFloat(2.0))),
        (bounce, PropertyValue::Number(OrderedFloat(0.7))),
        (friction, PropertyValue::Number(OrderedFloat(0.3))),
    ] {
        service
            .set_module_parameter(instance_id, parameter_id, value)
            .unwrap();
    }
    service.snapshot().unwrap()
}

fn particle_scene(items: &[FrameItem]) -> Option<&PointSceneFrame> {
    for item in items {
        match item {
            FrameItem::Object(object) => {
                if let FrameContent::PointScene { scene, .. } = &object.content
                    && matches!(&scene.source, PointSceneSource::Particle { .. })
                {
                    return Some(scene);
                }
            }
            FrameItem::Group(group) => {
                if let Some(scene) = particle_scene(&group.items) {
                    return Some(scene);
                }
            }
            FrameItem::Transition(transition) => {
                if let Some(scene) = particle_scene(std::slice::from_ref(&transition.from.item))
                    .or_else(|| particle_scene(std::slice::from_ref(&transition.to.item)))
                {
                    return Some(scene);
                }
            }
        }
    }
    None
}

#[test]
fn collision_published_parameters_reach_the_export_frame() {
    let project = collision_export_project();
    let plan = RenderPlanCompiler::compile(project.as_ref()).unwrap();
    let frame = evaluate_render_plan_frame(
        project.as_ref(),
        &plan,
        &PluginManager::default(),
        60,
        1.0,
        None,
    )
    .unwrap();
    let scene = particle_scene(&frame.items).expect("export frame Particle scene");
    let PointSceneSource::Particle { parameters, .. } = &scene.source else {
        panic!("expected Particle producer")
    };
    assert_eq!(
        parameters.collisions,
        vec![ParticleCollider::Plane {
            plane_point: vec3(0.0, 20.0, 0.0),
            plane_normal: vec3(0.0, -1.0, 0.0),
            radius: OrderedFloat(2.0),
            bounce: OrderedFloat(0.7),
            friction: OrderedFloat(0.3),
        }]
    );
}

#[cfg(all(feature = "gl", target_os = "windows"))]
#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn authoring_collision_particle_png_export_matches_preview_and_is_nontransparent() {
    assert_point_png_export_matches_preview(collision_export_project());
}
