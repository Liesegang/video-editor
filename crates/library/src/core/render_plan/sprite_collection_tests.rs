use super::particle_tests::{
    ParticleFixture, connection, particle_fixture, particle_node_id, point_scenes,
};
use super::point_tests::point_fixture;
use super::{RenderPlanCache, RenderPlanCompiler, evaluate_render_plan_frame};
use crate::model::animation::EasingFunction;
use crate::model::asset::{Asset, AssetKind};
use crate::model::authoring::{
    AutomationKeyframe, AutomationTrack, MediaTime, PublishedParameterId, SourceRef,
};
use crate::model::frame::point::{PointRenderStyle, PointSceneFrame, SpriteSelection};
use crate::model::node::{DataContent, Node, ParticleNodeRole};
use crate::model::project::connection::DATA_VALUE_OUTPUT_PORT;
use crate::model::property::{ImageCollectionValue, Property, PropertyValue};
use crate::plugin::PluginManager;
use ordered_float::OrderedFloat;

fn number(value: f64) -> PropertyValue {
    PropertyValue::Number(OrderedFloat(value))
}

fn renderer(fixture: &ParticleFixture) -> uuid::Uuid {
    particle_node_id(fixture, ParticleNodeRole::SpriteRenderer.catalog_id())
}

fn parameter(fixture: &ParticleFixture, port: &str) -> PublishedParameterId {
    fixture.project.module_definitions[&fixture.definition_id]
        .interface
        .parameters
        .iter()
        .find(|parameter| {
            parameter.target.node_id == renderer(fixture) && parameter.target.port == port
        })
        .unwrap()
        .id
}

fn images(fixture: &mut ParticleFixture) -> ImageCollectionValue {
    let assets = [
        Asset::new("Leaf", "leaf.png", AssetKind::Image),
        Asset::new("Petal", "petal.png", AssetKind::Image),
    ];
    let collection = ImageCollectionValue {
        assets: assets.iter().map(|asset| asset.id).collect(),
    };
    fixture.project.assets.extend(assets);
    collection
}

fn sample(fixture: &ParticleFixture, frame: u64) -> Vec<PointSceneFrame> {
    let plan = RenderPlanCompiler::compile(&fixture.project).unwrap();
    let frame = evaluate_render_plan_frame(
        &fixture.project,
        &plan,
        &PluginManager::default(),
        frame,
        1.0,
        None,
    )
    .unwrap();
    point_scenes(&frame.items).into_iter().cloned().collect()
}

#[test]
fn default_sprite_is_a_disc_and_collection_overrides_are_instance_local_render_only() {
    let mut fixture = particle_fixture(2);
    let collection = images(&mut fixture);
    let before = sample(&fixture, 15);
    assert!(
        before
            .iter()
            .all(|scene| scene.render_style == PointRenderStyle::default())
    );
    let collection_parameter = parameter(&fixture, "sprites");
    let mode_parameter = parameter(&fixture, "selection_mode");
    let selection_parameter = parameter(&fixture, "selection");
    let mut cache = RenderPlanCache::default();
    let (first_plan, _) = cache.compile(&fixture.project).unwrap();
    let overrides = &mut fixture
        .project
        .module_instances
        .get_mut(&fixture.instance_ids[0])
        .unwrap()
        .parameter_overrides;
    overrides.insert(
        collection_parameter,
        PropertyValue::ImageCollection(collection.clone()),
    );
    overrides.insert(mode_parameter, PropertyValue::String("value".into()));
    overrides.insert(selection_parameter, number(0.75));
    let (second_plan, stats) = cache.compile(&fixture.project).unwrap();
    assert_eq!(stats.compiled_definitions, 0);
    assert!(std::sync::Arc::ptr_eq(
        &first_plan.module_definitions[&fixture.definition_id],
        &second_plan.module_definitions[&fixture.definition_id]
    ));
    let after = sample(&fixture, 15);
    for scene in after {
        let previous = before
            .iter()
            .find(|previous| previous.invocation == scene.invocation)
            .unwrap();
        assert_eq!(
            scene.source, previous.source,
            "image edits must not alter simulation commands"
        );
        if scene.invocation.module_instance_id == fixture.instance_ids[0] {
            assert_eq!(
                scene.render_style,
                PointRenderStyle::Sprites {
                    images: collection.clone(),
                    selection: SpriteSelection::Value(OrderedFloat(0.75))
                }
            );
        } else {
            assert_eq!(scene.render_style, PointRenderStyle::default());
        }
    }
    let decoded = serde_json::from_str(&serde_json::to_string(&fixture.project).unwrap()).unwrap();
    assert_eq!(fixture.project, decoded);
}

#[test]
fn image_choice_uses_existing_published_timeline_keyframes() {
    let mut fixture = particle_fixture(1);
    let mode_parameter = parameter(&fixture, "selection_mode");
    let selection_parameter = parameter(&fixture, "selection");
    fixture
        .project
        .module_instances
        .get_mut(&fixture.instance_ids[0])
        .unwrap()
        .parameter_overrides
        .insert(mode_parameter, PropertyValue::String("value".into()));
    let SourceRef::Module(invocation) = &mut fixture
        .project
        .items
        .get_mut(&fixture.item_ids[0])
        .unwrap()
        .source
    else {
        panic!("module");
    };
    invocation.automation_tracks.insert(
        selection_parameter,
        AutomationTrack {
            keyframes: vec![
                AutomationKeyframe::new(MediaTime::zero(), number(0.0), EasingFunction::Linear),
                AutomationKeyframe::new(
                    MediaTime::new(1, 1).unwrap(),
                    number(1.0),
                    EasingFunction::Linear,
                ),
            ],
        },
    );
    for (frame, expected) in [(0, 0.0), (15, 0.5), (30, 1.0)] {
        assert_eq!(
            sample(&fixture, frame)[0].render_style,
            PointRenderStyle::Sprites {
                images: ImageCollectionValue::default(),
                selection: SpriteSelection::Value(OrderedFloat(expected))
            }
        );
    }
}

#[test]
fn random_selection_does_not_evaluate_the_unused_selection_property() {
    let mut fixture = particle_fixture(1);
    let renderer = renderer(&fixture);
    fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap()
        .graph
        .nodes
        .get_mut(&renderer)
        .unwrap()
        .set_property(
            "selection".into(),
            Property::expression(
                "missing_sprite_selection_symbol".into(),
                PropertyValue::Number(0.0.into()),
            ),
        )
        .unwrap();

    let scenes = sample(&fixture, 15);
    assert_eq!(scenes.len(), 1);
    assert_eq!(scenes[0].render_style, PointRenderStyle::default());
}

#[test]
fn collection_data_node_and_per_point_random_feed_the_shared_sprite_renderer() {
    let (mut fixture, nodes) = point_fixture(1);
    let collection = images(&mut fixture);
    let mode_parameter = parameter(&fixture, "selection_mode");
    fixture
        .project
        .module_instances
        .get_mut(&fixture.instance_ids[0])
        .unwrap()
        .parameter_overrides
        .insert(mode_parameter, PropertyValue::String("value".into()));
    let mut data = Node::new_catalog_node(DataContent::ImageCollection.catalog_id()).unwrap();
    data.set_property(
        "value".into(),
        Property::constant(PropertyValue::ImageCollection(collection.clone())),
    )
    .unwrap();
    let definition = fixture
        .project
        .module_definitions
        .get_mut(&fixture.definition_id)
        .unwrap();
    definition.interface.parameters.retain(|parameter| {
        !(parameter.target.node_id == nodes.renderer
            && matches!(parameter.target.port.as_str(), "selection" | "sprites"))
    });
    definition.graph.connections.push(connection(
        data.id,
        DATA_VALUE_OUTPUT_PORT,
        nodes.renderer,
        "sprites",
        0,
    ));
    definition.graph.connections.push(connection(
        nodes.info,
        "random",
        nodes.renderer,
        "selection",
        0,
    ));
    definition.graph.nodes.insert(data.id, data);
    let scenes = sample(&fixture, 15);
    assert_eq!(scenes[0].render_style.sprite_images(), Some(&collection));
    let program = scenes[0].point_program.as_ref().unwrap();
    let register = program
        .sprite_selection_register
        .expect("per-point selection register");
    assert!(matches!(
        program.instructions[usize::from(register)],
        crate::model::point::PointInstruction::Random { .. }
    ));
    program.validate().unwrap();
}
