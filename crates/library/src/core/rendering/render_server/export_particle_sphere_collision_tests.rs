use ordered_float::OrderedFloat;

use super::particle_collision_tests::{particle_scene, vec3};
use super::*;
use crate::core::render_plan::evaluate_render_plan_frame;
use crate::model::authoring::{ModuleConnection, ModuleConnectionId};
use crate::model::frame::particle::{ParticleCollider, ParticleCollisionMode};
use crate::model::frame::point::PointSceneSource;
use crate::model::node::{Node, NodeContent, ParticleNodeRole};
use crate::model::property::{Property, PropertyValue};

fn set_constant(node: &mut Node, key: &str, value: PropertyValue) {
    node.set_property(key.to_string(), Property::constant(value))
        .unwrap_or_else(|error| panic!("set Sphere Collision {key}: {error}"));
}

fn sphere_collision_export_project() -> Arc<AuthoringProject> {
    let mut project = particle_export_project().as_ref().clone();
    let definition = project.module_definitions.values_mut().next().unwrap();
    let native_node = |catalog_id: &str| {
        definition
            .graph
            .nodes
            .values()
            .find(|node| {
                matches!(
                    node.content(),
                    NodeContent::NativeOperation(content) if content.catalog_id == catalog_id
                )
            })
            .unwrap_or_else(|| panic!("Particle factory omitted {catalog_id}"))
            .id
    };
    let plane_id = native_node(ParticleNodeRole::CollisionPlane.catalog_id());
    let renderer_id = native_node(ParticleNodeRole::SpriteRenderer.catalog_id());
    let route = definition
        .graph
        .connections
        .iter()
        .find(|connection| {
            connection.from.node_id == plane_id && connection.to.node_id == renderer_id
        })
        .expect("Plane-to-Sprite route")
        .clone();

    let mut sphere = Node::new_catalog_node(ParticleNodeRole::CollisionSphere.catalog_id())
        .expect("Sphere Collision catalog Node");
    set_constant(
        &mut sphere,
        "center",
        PropertyValue::Vec3(vec3(0.0, 0.0, 0.0)),
    );
    set_constant(
        &mut sphere,
        "radius",
        PropertyValue::Number(OrderedFloat(30.0)),
    );
    set_constant(
        &mut sphere,
        "particle_radius",
        PropertyValue::Number(OrderedFloat(2.0)),
    );
    set_constant(
        &mut sphere,
        "bounce",
        PropertyValue::Number(OrderedFloat(0.7)),
    );
    set_constant(
        &mut sphere,
        "friction",
        PropertyValue::Number(OrderedFloat(0.3)),
    );
    let sphere_id = sphere.id;
    definition.graph.nodes.insert(sphere_id, sphere);
    definition
        .graph
        .connections
        .retain(|connection| connection.id != route.id);
    definition.graph.connections.extend([
        ModuleConnection {
            id: ModuleConnectionId::new(),
            from: route.from,
            to: ModulePortAddress {
                node_id: sphere_id,
                port: "particles".to_string(),
            },
            order: 0,
            blend_mode: crate::model::BlendMode::Normal,
        },
        ModuleConnection {
            id: ModuleConnectionId::new(),
            from: ModulePortAddress {
                node_id: sphere_id,
                port: "particles".to_string(),
            },
            to: route.to,
            order: 0,
            blend_mode: crate::model::BlendMode::Normal,
        },
    ]);
    definition.topology_revision += 1;
    project.validate().unwrap();
    Arc::new(project)
}

#[test]
fn sphere_collision_reaches_the_export_frame_through_the_particle_graph() {
    let project = sphere_collision_export_project();
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
        vec![ParticleCollider::Sphere {
            center: vec3(0.0, 0.0, 0.0),
            radius: OrderedFloat(30.0),
            particle_radius: OrderedFloat(2.0),
            mode: ParticleCollisionMode::Solid,
            bounce: OrderedFloat(0.7),
            friction: OrderedFloat(0.3),
        }]
    );
}

#[cfg(all(feature = "gl", target_os = "windows"))]
#[test]
#[ignore = "requires an idle desktop OpenGL 4.3 GPU"]
fn authoring_sphere_collision_png_export_matches_preview_and_is_nontransparent() {
    assert_point_png_export_matches_preview(sphere_collision_export_project());
}
