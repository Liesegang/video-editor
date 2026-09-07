use ordered_float::OrderedFloat;

use super::compiler::compile_module;
use super::particle_tests::{
    connection, particle_fixture, particle_node_id, particle_renderer_and_output, particle_scenes,
    particle_source as scene_particle_source,
};
use super::{RenderPlanCompiler, evaluate_render_plan_frame};
use crate::model::authoring::{ModulePortAddress, PublishedParameter, PublishedParameterId};
use crate::model::frame::particle::{
    PARTICLE_MAX_COLLIDERS, ParticleCollider, ParticleCollisionMode,
};
use crate::model::node::{Node, PARTICLE_SYSTEM_PORT, ParticleNodeRole};
use crate::model::project::PortDataType;
use crate::model::property::{PropertyValue, Vec3};
use crate::plugin::PluginManager;

fn vec3(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
        z: OrderedFloat(z),
    }
}

fn add_collision(
    definition: &mut crate::model::authoring::ModuleDefinition,
    role: ParticleNodeRole,
) -> uuid::Uuid {
    let node = Node::new_catalog_node(role.catalog_id()).expect("implemented collision");
    let id = node.id;
    definition.graph.nodes.insert(id, node);
    id
}

fn replace_particle_chain(
    definition: &mut crate::model::authoring::ModuleDefinition,
    node_ids: &[uuid::Uuid],
) {
    definition.graph.connections.retain(|connection| {
        connection.from.port != PARTICLE_SYSTEM_PORT && connection.to.port != PARTICLE_SYSTEM_PORT
    });
    definition
        .graph
        .connections
        .extend(node_ids.windows(2).map(|pair| {
            connection(
                pair[0],
                PARTICLE_SYSTEM_PORT,
                pair[1],
                PARTICLE_SYSTEM_PORT,
                0,
            )
        }));
    definition.topology_revision += 1;
}

fn stage_ids(
    fixture: &super::particle_tests::ParticleFixture,
) -> (uuid::Uuid, uuid::Uuid, uuid::Uuid, uuid::Uuid, uuid::Uuid) {
    (
        particle_node_id(fixture, ParticleNodeRole::Emitter.catalog_id()),
        particle_node_id(fixture, ParticleNodeRole::ShapeLocation.catalog_id()),
        particle_node_id(fixture, ParticleNodeRole::Initialize.catalog_id()),
        particle_node_id(fixture, ParticleNodeRole::Gravity.catalog_id()),
        particle_renderer_and_output(fixture).0,
    )
}

fn compiled_particle_source(
    renderer: &super::CompiledPointRenderer,
) -> &super::CompiledParticleSource {
    let super::CompiledPointSource::Particle(source) = &renderer.source else {
        panic!("expected Particle source");
    };
    source
}

#[test]
fn repeated_collisions_compile_after_forces_in_authored_order_and_respect_the_bound() {
    let fixture = particle_fixture(1);
    let (emitter, shape, initialize, gravity, renderer) = stage_ids(&fixture);
    for (count, expected) in [
        (PARTICLE_MAX_COLLIDERS, true),
        (PARTICLE_MAX_COLLIDERS + 1, false),
    ] {
        let mut definition = fixture.project.module_definitions[&fixture.definition_id].clone();
        let mut collisions = Vec::new();
        for _ in 0..count {
            let role = if collisions.len() % 2 == 0 {
                ParticleNodeRole::CollisionPlane
            } else {
                ParticleNodeRole::CollisionSphere
            };
            collisions.push(add_collision(&mut definition, role));
        }
        let mut chain = vec![emitter, shape, initialize, gravity];
        chain.extend(collisions.iter().copied());
        chain.push(renderer);
        replace_particle_chain(&mut definition, &chain);
        let compiled = compile_module(&definition).expect("bounded collision plan");
        assert_eq!(compiled.point_renderers.contains_key(&renderer), expected);
        if expected {
            assert_eq!(
                compiled_particle_source(&compiled.point_renderers[&renderer])
                    .collision_nodes
                    .iter()
                    .map(|modifier| modifier.node_id)
                    .collect::<Vec<_>>(),
                collisions
            );
        }
    }

    let mut wrong_order = fixture.project.module_definitions[&fixture.definition_id].clone();
    let collision = add_collision(&mut wrong_order, ParticleNodeRole::CollisionSphere);
    replace_particle_chain(
        &mut wrong_order,
        &[emitter, shape, initialize, collision, gravity, renderer],
    );
    assert!(
        compile_module(&wrong_order)
            .expect("out-of-order chain is a stable no-image plan")
            .point_renderers
            .is_empty()
    );
}

fn publish_collision_parameter(
    definition: &mut crate::model::authoring::ModuleDefinition,
    node_id: uuid::Uuid,
    key: &str,
    name: &str,
    data_type: PortDataType,
) -> PublishedParameterId {
    let default_value = definition.graph.nodes[&node_id]
        .properties()
        .get(key)
        .and_then(|property| property.value())
        .cloned()
        .expect("collision default");
    let id = PublishedParameterId::new();
    definition.interface.parameters.push(PublishedParameter {
        id,
        name: name.to_string(),
        data_type,
        default_value,
        target: ModulePortAddress {
            node_id,
            port: key.to_string(),
        },
    });
    id
}

#[test]
fn sphere_collision_values_and_instance_overrides_sample_without_recompiling() {
    let mut fixture = particle_fixture(2);
    let (emitter, shape, initialize, gravity, renderer) = stage_ids(&fixture);
    let plane = particle_node_id(&fixture, ParticleNodeRole::CollisionPlane.catalog_id());
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .expect("Particle definition");
    let sphere = add_collision(definition, ParticleNodeRole::CollisionSphere);
    replace_particle_chain(
        definition,
        &[emitter, shape, initialize, gravity, plane, sphere, renderer],
    );
    let active = publish_collision_parameter(
        definition,
        sphere,
        "active",
        "Sphere Enabled",
        PortDataType::Boolean,
    );
    let center = publish_collision_parameter(
        definition,
        sphere,
        "center",
        "Sphere Center",
        PortDataType::Vec3,
    );
    let radius = publish_collision_parameter(
        definition,
        sphere,
        "radius",
        "Sphere Radius",
        PortDataType::Number,
    );
    let particle_radius = publish_collision_parameter(
        definition,
        sphere,
        "particle_radius",
        "Particle Radius",
        PortDataType::Number,
    );
    let mode = publish_collision_parameter(
        definition,
        sphere,
        "mode",
        "Sphere Mode",
        PortDataType::String,
    );
    definition.interface_version += 1;
    for (index, instance_id) in fixture.instance_ids.iter().copied().enumerate() {
        let overrides = &mut fixture
            .project
            .module_instances
            .get_mut(&instance_id)
            .expect("Particle instance")
            .parameter_overrides;
        overrides.insert(active, PropertyValue::Boolean(index == 0));
        overrides.insert(center, PropertyValue::Vec3(vec3(10.0, 20.0, 30.0)));
        overrides.insert(radius, PropertyValue::Number(OrderedFloat(100.0)));
        overrides.insert(particle_radius, PropertyValue::Number(OrderedFloat(4.0)));
        overrides.insert(mode, PropertyValue::String("Container".to_string()));
    }
    let plan = RenderPlanCompiler::compile(&fixture.project).expect("shared Sphere definition");
    assert_eq!(plan.module_definitions.len(), 1);
    let frame = evaluate_render_plan_frame(
        &fixture.project,
        &plan,
        &PluginManager::default(),
        30,
        1.0,
        None,
    )
    .expect("sample Sphere collisions");
    let scenes = particle_scenes(&frame.items);
    let first = scenes
        .iter()
        .find(|scene| scene.invocation.module_instance_id == fixture.instance_ids[0])
        .expect("active instance");
    assert_eq!(
        scene_particle_source(first).1.collisions,
        vec![ParticleCollider::Sphere {
            center: vec3(10.0, 20.0, 30.0),
            radius: OrderedFloat(100.0),
            particle_radius: OrderedFloat(4.0),
            mode: ParticleCollisionMode::Container,
            bounce: OrderedFloat(0.5),
            friction: OrderedFloat(0.1),
        }]
    );
    let second = scenes
        .iter()
        .find(|scene| scene.invocation.module_instance_id == fixture.instance_ids[1])
        .expect("inactive sibling");
    assert!(scene_particle_source(second).1.collisions.is_empty());
}

#[test]
fn bypass_and_active_are_distinct_collision_controls() {
    let mut fixture = particle_fixture(1);
    let (_, _, _, _, renderer) = stage_ids(&fixture);
    let collision = particle_node_id(&fixture, ParticleNodeRole::CollisionPlane.catalog_id());
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .expect("Particle definition");
    let compiled = compile_module(definition).expect("factory collision");
    assert_eq!(
        compiled_particle_source(&compiled.point_renderers[&renderer])
            .collision_nodes
            .len(),
        1,
        "active=false stays compiled so instance overrides share the definition"
    );

    definition
        .graph
        .nodes
        .get_mut(&collision)
        .expect("Collision Plane")
        .bypassed = true;
    let compiled = compile_module(definition).expect("bypassed collision");
    assert!(
        compiled_particle_source(&compiled.point_renderers[&renderer])
            .collision_nodes
            .is_empty()
    );
}

#[test]
fn bypassed_sphere_preserves_the_particle_chain_without_a_collision_stage() {
    let fixture = particle_fixture(1);
    let (emitter, shape, initialize, gravity, renderer) = stage_ids(&fixture);
    let mut definition = fixture.project.module_definitions[&fixture.definition_id].clone();
    let sphere = add_collision(&mut definition, ParticleNodeRole::CollisionSphere);
    definition
        .graph
        .nodes
        .get_mut(&sphere)
        .expect("Collision Sphere")
        .bypassed = true;
    replace_particle_chain(
        &mut definition,
        &[emitter, shape, initialize, gravity, sphere, renderer],
    );
    let compiled = compile_module(&definition).expect("bypassed Sphere plan");
    assert!(
        compiled_particle_source(&compiled.point_renderers[&renderer])
            .collision_nodes
            .is_empty()
    );
}

#[test]
fn active_collision_values_and_instance_overrides_sample_without_recompiling() {
    let mut fixture = particle_fixture(2);
    let definition = &fixture.project.module_definitions[&fixture.definition_id];
    let parameter = |name: &str| {
        definition
            .interface
            .parameters
            .iter()
            .find(|parameter| parameter.name == name)
            .unwrap_or_else(|| panic!("missing {name}"))
            .id
    };
    let active = parameter("Collision Enabled");
    let point = parameter("Plane Point");
    let normal = parameter("Plane Normal");
    let radius = parameter("Radius");
    let bounce = parameter("Bounce");
    let friction = parameter("Friction");
    for (index, instance_id) in fixture.instance_ids.iter().copied().enumerate() {
        let overrides = &mut fixture
            .project
            .module_instances
            .get_mut(&instance_id)
            .expect("Particle instance")
            .parameter_overrides;
        overrides.insert(active, PropertyValue::Boolean(index == 0));
        overrides.insert(point, PropertyValue::Vec3(vec3(10.0, 20.0, 30.0)));
        overrides.insert(normal, PropertyValue::Vec3(vec3(0.0, 0.0, 2.0)));
        overrides.insert(radius, PropertyValue::Number(OrderedFloat(4.0)));
        overrides.insert(bounce, PropertyValue::Number(OrderedFloat(0.75)));
        overrides.insert(friction, PropertyValue::Number(OrderedFloat(0.25)));
    }
    let plan = RenderPlanCompiler::compile(&fixture.project).expect("shared collision definition");
    assert_eq!(plan.module_definitions.len(), 1);
    let frame = evaluate_render_plan_frame(
        &fixture.project,
        &plan,
        &PluginManager::default(),
        30,
        1.0,
        None,
    )
    .expect("sample collisions");
    let scenes = particle_scenes(&frame.items);
    assert_eq!(scenes.len(), 2);
    let first = scenes
        .iter()
        .find(|scene| scene.invocation.module_instance_id == fixture.instance_ids[0])
        .expect("active instance");
    assert_eq!(
        scene_particle_source(first).1.collisions,
        vec![ParticleCollider::Plane {
            plane_point: vec3(10.0, 20.0, 30.0),
            plane_normal: vec3(0.0, 0.0, 2.0),
            radius: OrderedFloat(4.0),
            bounce: OrderedFloat(0.75),
            friction: OrderedFloat(0.25),
        }]
    );
    let second = scenes
        .iter()
        .find(|scene| scene.invocation.module_instance_id == fixture.instance_ids[1])
        .expect("inactive sibling");
    assert!(scene_particle_source(second).1.collisions.is_empty());
}

#[test]
fn disabled_collision_invalidates_the_connected_particle_chain() {
    let mut fixture = particle_fixture(1);
    let collision = particle_node_id(&fixture, ParticleNodeRole::CollisionPlane.catalog_id());
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .expect("Particle definition");
    definition.graph.nodes.get_mut(&collision).unwrap().enabled = false;
    assert!(
        compile_module(definition)
            .expect("disabled stage is a stable no-image plan")
            .point_renderers
            .is_empty()
    );
}
