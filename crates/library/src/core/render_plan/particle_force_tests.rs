use ordered_float::OrderedFloat;

use super::RenderPlanCompiler;
use super::compiler::compile_module;
use super::evaluate_render_plan_frame;
use super::particle_tests::{
    connection, particle_fixture, particle_node_id, particle_renderer_and_output, particle_scenes,
};
use crate::model::frame::particle::{PARTICLE_MAX_FORCES, ParticleForce};
use crate::model::node::{Node, PARTICLE_SYSTEM_PORT, ParticleNodeRole};
use crate::model::project::property::{Property, PropertyValue};
use crate::model::property::Vec3;
use crate::plugin::PluginManager;

fn vec3(x: f64, y: f64, z: f64) -> Vec3 {
    Vec3 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
        z: OrderedFloat(z),
    }
}

fn set_constant(node: &mut Node, key: &str, value: PropertyValue) {
    node.set_property(key.to_string(), Property::constant(value))
        .unwrap_or_else(|error| panic!("set {key}: {error}"));
}

fn add_force_node(
    definition: &mut crate::model::authoring::ModuleDefinition,
    role: ParticleNodeRole,
) -> uuid::Uuid {
    let node = Node::new_catalog_node(role.catalog_id())
        .unwrap_or_else(|error| panic!("create {role:?}: {error}"));
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

fn canonical_stage_ids(
    fixture: &super::particle_tests::ParticleFixture,
) -> (uuid::Uuid, uuid::Uuid, uuid::Uuid, uuid::Uuid) {
    (
        particle_node_id(fixture, ParticleNodeRole::Emitter.catalog_id()),
        particle_node_id(fixture, ParticleNodeRole::ShapeLocation.catalog_id()),
        particle_node_id(fixture, ParticleNodeRole::Initialize.catalog_id()),
        particle_renderer_and_output(fixture).0,
    )
}

#[test]
fn repeated_force_kinds_compile_in_authored_execution_order() {
    let mut fixture = particle_fixture(1);
    let (emitter, shape, initialize, renderer) = canonical_stage_ids(&fixture);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .expect("Particle definition");
    let gravity_a = add_force_node(definition, ParticleNodeRole::Gravity);
    let drag = add_force_node(definition, ParticleNodeRole::Drag);
    let gravity_b = add_force_node(definition, ParticleNodeRole::Gravity);
    replace_particle_chain(
        definition,
        &[
            emitter, shape, initialize, gravity_a, drag, gravity_b, renderer,
        ],
    );

    let compiled = compile_module(definition).expect("compile repeated force chain");
    let forces = &compiled.particle_renderers[&renderer].force_nodes;
    assert_eq!(
        forces
            .iter()
            .map(|force| (force.node_id, force.role))
            .collect::<Vec<_>>(),
        vec![
            (gravity_a, ParticleNodeRole::Gravity),
            (drag, ParticleNodeRole::Drag),
            (gravity_b, ParticleNodeRole::Gravity),
        ]
    );
}

#[test]
fn force_chain_rejects_cross_stage_cycles_and_more_than_the_bounded_limit() {
    let fixture = particle_fixture(1);
    let (emitter, shape, initialize, renderer) = canonical_stage_ids(&fixture);

    let mut cross_stage = fixture.project.module_definitions[&fixture.definition_id].clone();
    let gravity = add_force_node(&mut cross_stage, ParticleNodeRole::Gravity);
    replace_particle_chain(
        &mut cross_stage,
        &[emitter, shape, gravity, initialize, renderer],
    );
    assert!(
        compile_module(&cross_stage)
            .expect("invalid Particle stage is a stable no-image plan")
            .particle_renderers
            .is_empty()
    );

    let mut cyclic = fixture.project.module_definitions[&fixture.definition_id].clone();
    let gravity = add_force_node(&mut cyclic, ParticleNodeRole::Gravity);
    let drag = add_force_node(&mut cyclic, ParticleNodeRole::Drag);
    replace_particle_chain(&mut cyclic, &[gravity, drag, renderer]);
    cyclic.graph.connections.push(connection(
        drag,
        PARTICLE_SYSTEM_PORT,
        gravity,
        PARTICLE_SYSTEM_PORT,
        0,
    ));
    let cycle_error = compile_module(&cyclic).expect_err("Particle cycle must fail closed");
    assert!(cycle_error.to_ascii_lowercase().contains("cycle"));

    for (count, expected) in [
        (PARTICLE_MAX_FORCES, true),
        (PARTICLE_MAX_FORCES + 1, false),
    ] {
        let mut bounded = fixture.project.module_definitions[&fixture.definition_id].clone();
        let mut chain = vec![emitter, shape, initialize];
        for _ in 0..count {
            chain.push(add_force_node(&mut bounded, ParticleNodeRole::Gravity));
        }
        chain.push(renderer);
        replace_particle_chain(&mut bounded, &chain);
        assert_eq!(
            compile_module(&bounded)
                .expect("bounded Particle plan")
                .particle_renderers
                .contains_key(&renderer),
            expected,
            "force count {count}"
        );
    }
}

#[test]
fn bypassed_force_is_omitted_without_reordering_neighboring_forces() {
    let mut fixture = particle_fixture(1);
    let (emitter, shape, initialize, renderer) = canonical_stage_ids(&fixture);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .expect("Particle definition");
    let gravity = add_force_node(definition, ParticleNodeRole::Gravity);
    let turbulence = add_force_node(definition, ParticleNodeRole::Turbulence);
    let drag = add_force_node(definition, ParticleNodeRole::Drag);
    definition
        .graph
        .nodes
        .get_mut(&turbulence)
        .expect("Turbulence")
        .bypassed = true;
    replace_particle_chain(
        definition,
        &[
            emitter, shape, initialize, gravity, turbulence, drag, renderer,
        ],
    );

    let compiled = compile_module(definition).expect("compile bypassed force");
    assert_eq!(
        compiled.particle_renderers[&renderer]
            .force_nodes
            .iter()
            .map(|force| force.role)
            .collect::<Vec<_>>(),
        vec![ParticleNodeRole::Gravity, ParticleNodeRole::Drag]
    );
}

#[test]
fn all_force_property_values_map_to_the_scene_in_exact_order() {
    let mut fixture = particle_fixture(1);
    let (emitter, shape, initialize, renderer) = canonical_stage_ids(&fixture);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .expect("Particle definition");
    let gravity = add_force_node(definition, ParticleNodeRole::Gravity);
    let drag = add_force_node(definition, ParticleNodeRole::Drag);
    let turbulence = add_force_node(definition, ParticleNodeRole::Turbulence);
    let vortex = add_force_node(definition, ParticleNodeRole::Vortex);
    let point = add_force_node(definition, ParticleNodeRole::Point);

    set_constant(
        definition.graph.nodes.get_mut(&gravity).expect("Gravity"),
        "force",
        PropertyValue::Vec3(vec3(1.0, 2.0, 3.0)),
    );
    set_constant(
        definition.graph.nodes.get_mut(&drag).expect("Drag"),
        "coefficient",
        PropertyValue::Number(OrderedFloat(0.25)),
    );
    let turbulence_node = definition
        .graph
        .nodes
        .get_mut(&turbulence)
        .expect("Turbulence");
    set_constant(
        turbulence_node,
        "strength",
        PropertyValue::Number(OrderedFloat(7.5)),
    );
    set_constant(
        turbulence_node,
        "frequency",
        PropertyValue::Number(OrderedFloat(0.75)),
    );
    set_constant(turbulence_node, "octaves", PropertyValue::Integer(3));
    set_constant(
        turbulence_node,
        "evolution",
        PropertyValue::Number(OrderedFloat(1.25)),
    );
    set_constant(turbulence_node, "seed", PropertyValue::Integer(42));
    let vortex_node = definition.graph.nodes.get_mut(&vortex).expect("Vortex");
    set_constant(
        vortex_node,
        "axis",
        PropertyValue::Vec3(vec3(0.0, 0.0, 1.0)),
    );
    set_constant(
        vortex_node,
        "center",
        PropertyValue::Vec3(vec3(10.0, 20.0, 30.0)),
    );
    set_constant(
        vortex_node,
        "strength",
        PropertyValue::Number(OrderedFloat(-4.0)),
    );
    let point_node = definition.graph.nodes.get_mut(&point).expect("Point Force");
    set_constant(
        point_node,
        "target",
        PropertyValue::Vec3(vec3(40.0, 50.0, 60.0)),
    );
    set_constant(
        point_node,
        "strength",
        PropertyValue::Number(OrderedFloat(9.0)),
    );
    set_constant(
        point_node,
        "radius",
        PropertyValue::Number(OrderedFloat(120.0)),
    );
    set_constant(
        point_node,
        "falloff",
        PropertyValue::Number(OrderedFloat(2.5)),
    );
    replace_particle_chain(
        definition,
        &[
            emitter, shape, initialize, gravity, drag, turbulence, vortex, point, renderer,
        ],
    );

    fixture.project.validate().expect("valid force project");
    let plan = RenderPlanCompiler::compile(&fixture.project).expect("compiled force project");
    let frame = evaluate_render_plan_frame(
        &fixture.project,
        &plan,
        &PluginManager::default(),
        30,
        1.0,
        None,
    )
    .expect("evaluated force project");
    assert_eq!(
        particle_scenes(&frame.items)[0].parameters.forces,
        vec![
            ParticleForce::Gravity {
                acceleration: vec3(1.0, 2.0, 3.0),
            },
            ParticleForce::Drag {
                coefficient: OrderedFloat(0.25),
            },
            ParticleForce::Turbulence {
                strength: OrderedFloat(7.5),
                frequency: OrderedFloat(0.75),
                octaves: 3,
                evolution: OrderedFloat(1.25),
                seed: 42,
            },
            ParticleForce::Vortex {
                axis: vec3(0.0, 0.0, 1.0),
                center: vec3(10.0, 20.0, 30.0),
                strength: OrderedFloat(-4.0),
            },
            ParticleForce::Point {
                target: vec3(40.0, 50.0, 60.0),
                strength: OrderedFloat(9.0),
                radius: OrderedFloat(120.0),
                falloff: OrderedFloat(2.5),
            },
        ]
    );
}

#[test]
fn instance_force_overrides_share_one_compiled_executable() {
    let mut fixture = particle_fixture(2);
    let gravity_parameter = fixture.project.module_definitions[&fixture.definition_id]
        .interface
        .parameters
        .iter()
        .find(|parameter| parameter.name == "Gravity")
        .expect("published Gravity")
        .id;
    for (instance_id, acceleration) in fixture
        .instance_ids
        .iter()
        .copied()
        .zip([vec3(2.0, 3.0, 4.0), vec3(-5.0, 6.0, 7.0)])
    {
        fixture
            .project
            .module_instances
            .get_mut(&instance_id)
            .expect("Particle instance")
            .parameter_overrides
            .insert(gravity_parameter, PropertyValue::Vec3(acceleration));
    }

    let plan = RenderPlanCompiler::compile(&fixture.project).expect("compiled shared definition");
    assert_eq!(plan.module_definitions.len(), 1);
    let frame = evaluate_render_plan_frame(
        &fixture.project,
        &plan,
        &PluginManager::default(),
        30,
        1.0,
        None,
    )
    .expect("evaluated instance overrides");
    let scenes = particle_scenes(&frame.items);
    assert_eq!(scenes.len(), 2);
    assert_eq!(scenes[0].executable_hash, scenes[1].executable_hash);
    assert_eq!(
        scenes
            .iter()
            .map(|scene| match &scene.parameters.forces[0] {
                ParticleForce::Gravity { acceleration } => *acceleration,
                force => panic!("first force is not Gravity: {force:?}"),
            })
            .collect::<Vec<_>>(),
        vec![vec3(2.0, 3.0, 4.0), vec3(-5.0, 6.0, 7.0)]
    );
}

#[test]
fn disabled_new_force_produces_no_particle_image() {
    let mut fixture = particle_fixture(1);
    let (emitter, shape, initialize, renderer) = canonical_stage_ids(&fixture);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .expect("Particle definition");
    let point = add_force_node(definition, ParticleNodeRole::Point);
    definition
        .graph
        .nodes
        .get_mut(&point)
        .expect("Point Force")
        .enabled = false;
    replace_particle_chain(definition, &[emitter, shape, initialize, point, renderer]);

    let compiled = compile_module(definition).expect("disabled force compiles to no image");
    assert!(compiled.particle_renderers.is_empty());
}
