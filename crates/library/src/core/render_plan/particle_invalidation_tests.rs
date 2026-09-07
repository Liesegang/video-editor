use super::particle_tests::{
    ParticleFixture, particle_fixture, particle_node_id, particle_renderer_and_output,
    particle_scenes,
};
use super::point_tests::point_fixture as point_field_fixture;
use super::{RenderPlanCompiler, evaluate_render_plan_frame};
use crate::model::frame::color::Color;
use crate::model::frame::point::PointSceneFrame;
use crate::model::node::{Node, PARTICLE_SYSTEM_PORT, ParticleNodeRole};
use crate::model::project::property::PropertyValue;
use crate::plugin::PluginManager;

fn compiled_scene(fixture: &ParticleFixture) -> ([u8; 32], PointSceneFrame) {
    let plan = RenderPlanCompiler::compile(&fixture.project).expect("compiled Particle plan");
    let fingerprint = plan.module_definitions[&fixture.definition_id].fingerprint;
    let frame = evaluate_render_plan_frame(
        &fixture.project,
        &plan,
        &PluginManager::default(),
        30,
        1.0,
        None,
    )
    .expect("evaluated Particle frame");
    let scenes = particle_scenes(&frame.items);
    assert_eq!(scenes.len(), 1);
    (fingerprint, scenes[0].clone())
}

#[test]
fn render_only_definition_edit_changes_fingerprint_but_keeps_simulation_identity() {
    let mut fixture = particle_fixture(1);
    let (before_fingerprint, before) = compiled_scene(&fixture);
    let (renderer_id, _) = particle_renderer_and_output(&fixture);
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    let published_color = definition
        .interface
        .parameters
        .iter_mut()
        .find(|parameter| {
            parameter.target.node_id == renderer_id && parameter.target.port == "color"
        })
        .expect("published Sprite Color");
    published_color.default_value = PropertyValue::Color(Color {
        r: 240,
        g: 35,
        b: 80,
        a: 190,
    });

    let (after_fingerprint, after) = compiled_scene(&fixture);
    assert_ne!(before_fingerprint, after_fingerprint);
    assert_eq!(before.invocation, after.invocation);
    assert_eq!(before.source_node_id, after.source_node_id);
    assert_eq!(before.source, after.source);
    assert_ne!(before.color, after.color);
}

#[test]
fn render_only_point_schema_edit_keeps_the_particle_source_identity() {
    let (mut fixture, nodes) = point_field_fixture(1);
    let (before_fingerprint, before) = compiled_scene(&fixture);
    fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap()
        .graph
        .nodes
        .get_mut(&nodes.store)
        .unwrap()
        .name = "renamed heat".to_string();

    let (after_fingerprint, after) = compiled_scene(&fixture);
    assert_ne!(before_fingerprint, after_fingerprint);
    assert_eq!(before.invocation, after.invocation);
    assert_eq!(before.source_node_id, after.source_node_id);
    assert_eq!(before.source, after.source);
    assert_ne!(before.point_program, after.point_program);
}

#[test]
fn changing_the_particle_producer_is_visible_beyond_the_stable_invocation_key() {
    let mut fixture = particle_fixture(1);
    let (before_fingerprint, before) = compiled_scene(&fixture);
    let original_emitter = particle_node_id(&fixture, ParticleNodeRole::Emitter.catalog_id());
    let replacement =
        Node::new_catalog_node(ParticleNodeRole::Emitter.catalog_id()).expect("Particle Emitter");
    let replacement_id = replacement.id;
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    definition.graph.nodes.insert(replacement_id, replacement);
    definition
        .graph
        .connections
        .iter_mut()
        .find(|connection| {
            connection.from.node_id == original_emitter
                && connection.from.port == PARTICLE_SYSTEM_PORT
        })
        .expect("Emitter stream connection")
        .from
        .node_id = replacement_id;
    definition.topology_revision = definition.topology_revision.saturating_add(1);

    let (after_fingerprint, after) = compiled_scene(&fixture);
    assert_ne!(before_fingerprint, after_fingerprint);
    assert_eq!(before.invocation, after.invocation);
    assert_eq!(before.source, after.source);
    assert_eq!(before.source_node_id, original_emitter);
    assert_eq!(after.source_node_id, replacement_id);
    assert_ne!(before.source_node_id, after.source_node_id);
}
