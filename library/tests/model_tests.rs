use std::sync::{Arc, RwLock};

use library::editor::handlers::clip_handler::ClipHandler;
use library::model::project::{
    Composition, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, NodeContainer, PortAddress, PortOwner,
    Project, TIME_PORT,
};
use library::model::property::{Property, PropertyMap, PropertyValue};
use library::model::{
    Clip, GeneratorContent, MediaContent, Node, NodeContent, ReferenceContent, Track,
};
use ordered_float::OrderedFloat;
use uuid::Uuid;

fn add_composition(project: &mut Project, name: &str) -> (Uuid, Uuid) {
    let (composition, track) = Composition::new(name, 1920, 1080, 30.0, 10.0);
    let composition_id = composition.id;
    let track_id = track.id;
    project.add_track(track);
    project.add_composition(composition);
    (composition_id, track_id)
}

fn add_clip(project: &mut Project, track_id: Uuid, name: &str, start: f64) -> Uuid {
    let clip = Clip::new(name, start, 5.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id).unwrap();
    clip_id
}

fn solid(name: &str) -> Node {
    let mut node = Node::new(name, NodeContent::Generator(GeneratorContent::Solid));
    node.properties.set(
        "color".to_string(),
        Property::constant(PropertyValue::Color(Default::default())),
    );
    node
}

fn add_node(project: &mut Project, clip_id: Uuid, node: Node) -> Uuid {
    let node_id = node.id;
    project.add_node(node);
    project
        .attach_node_to_container(NodeContainer::Clip(clip_id), node_id)
        .unwrap();
    node_id
}

#[test]
fn property_serialization_roundtrip() {
    let mut properties = PropertyMap::new();
    properties.set(
        "opacity".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(0.5))),
    );

    let json = serde_json::to_string(&properties).expect("property map should serialize");
    let loaded: PropertyMap = serde_json::from_str(&json).expect("property map should deserialize");

    assert_eq!(
        loaded.get("opacity").and_then(Property::value),
        Some(&PropertyValue::Number(OrderedFloat(0.5)))
    );
}

#[test]
fn media_audio_stream_selection_is_required_pre_v1_state() {
    let asset_id = Uuid::new_v4();
    let missing_audio_stream = serde_json::json!({
        "asset_id": asset_id,
        "stream_index": 0
    });
    let error = serde_json::from_value::<MediaContent>(missing_audio_stream).unwrap_err();
    assert!(error.to_string().contains("audio_stream_index"));

    let media = serde_json::from_value::<MediaContent>(serde_json::json!({
        "asset_id": asset_id,
        "stream_index": 0,
        "audio_stream_index": 2
    }))
    .unwrap();
    assert_eq!(media.stream_index, Some(0));
    assert_eq!(media.audio_stream_index, Some(2));
}

#[test]
fn clip_timing_metadata_validates_freeze_and_never_duplicates_structural_values() {
    let definitions = Clip::timing_property_definitions();
    assert_eq!(definitions.len(), 4);
    assert_eq!(
        definitions
            .iter()
            .map(|definition| definition.name())
            .collect::<Vec<_>>(),
        vec!["start_time", "duration", "trim_in", "time_stretch"]
    );
    let stretch = Clip::timing_property_definition("time_stretch").unwrap();
    assert!(
        stretch
            .validate_value(&PropertyValue::Number(OrderedFloat(0.0)))
            .is_ok(),
        "zero is the canonical freeze value"
    );
    assert!(
        stretch
            .validate_value(&PropertyValue::Number(OrderedFloat(-0.001)))
            .is_err()
    );
    assert!(
        stretch
            .validate_value(&PropertyValue::Number(OrderedFloat(f64::NAN)))
            .is_err()
    );
    assert!(
        stretch
            .validate_value(&PropertyValue::Number(OrderedFloat(2_000.0)))
            .is_ok(),
        "the metadata max is soft guidance, not a mutation bound"
    );

    let mut project = Project::new("timing authority");
    let clip = Clip::new("freeze", 0.0, 5.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    let shared = Arc::new(RwLock::new(project));
    ClipHandler::update_property_or_keyframe(
        &shared,
        library::editor::handlers::property_ops::PropertyOwner::Clip(clip_id),
        "time_stretch",
        42.0,
        PropertyValue::Number(OrderedFloat(0.0)),
        None,
    )
    .unwrap();

    let read = shared.read().unwrap();
    let clip = read.get_clip(clip_id).unwrap();
    assert_eq!(clip.time_stretch, OrderedFloat(0.0));
    for definition in definitions {
        assert!(
            clip.properties.get(definition.name()).is_none(),
            "structural {} must not be copied into PropertyMap",
            definition.name()
        );
    }
    let saved = read.save().unwrap();
    drop(read);
    let loaded = Project::load(&saved).unwrap();
    let clip = loaded.get_clip(clip_id).unwrap();
    assert_eq!(clip.time_stretch, OrderedFloat(0.0));
    assert_eq!(clip.local_time(999.0), clip.trim_in.into_inner());
    assert!(
        definitions
            .iter()
            .all(|definition| clip.properties.get(definition.name()).is_none())
    );

    let before = shared.read().unwrap().clone();
    assert!(
        ClipHandler::update_property_or_keyframe(
            &shared,
            library::editor::handlers::property_ops::PropertyOwner::Clip(clip_id),
            "time_stretch",
            0.0,
            PropertyValue::Number(OrderedFloat(-1.0)),
            None,
        )
        .is_err()
    );
    assert_eq!(*shared.read().unwrap(), before);
}

#[test]
fn node_properties_are_the_only_generator_value_authority() {
    let mut node = Node::new("Text", NodeContent::Generator(GeneratorContent::Text));
    node.properties.set(
        "text".to_string(),
        Property::constant(PropertyValue::String("before".to_string())),
    );
    node.properties.set(
        "font_family".to_string(),
        Property::constant(PropertyValue::String("Arial".to_string())),
    );
    let mut clip = Clip::new("placement", 0.0, 5.0);

    assert!(node.update_property_or_keyframe(
        "text",
        0.0,
        PropertyValue::String("after".to_string()),
        None,
    ));
    assert!(clip.update_property_or_keyframe(
        "start_time",
        0.0,
        PropertyValue::Number(OrderedFloat(2.5)),
        None,
    ));

    assert_eq!(node.properties.get_string("text").as_deref(), Some("after"));
    assert_eq!(node.content, NodeContent::Generator(GeneratorContent::Text));
    assert_eq!(clip.start_time, OrderedFloat(2.5));
    let serialized = serde_json::to_value(node).unwrap();
    assert_eq!(serialized["content"]["data"], "Text");
    assert!(serialized.get("start_time").is_none());
    assert!(serialized.get("duration").is_none());
}

#[test]
fn explicit_keyframe_insert_promotes_a_constant_property() {
    let mut properties = PropertyMap::new();
    properties.set(
        "opacity".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(100.0))),
    );

    assert!(properties.upsert_keyframe(
        "opacity",
        1.25,
        PropertyValue::Number(OrderedFloat(75.0)),
        None,
    ));

    let property = properties.get("opacity").unwrap();
    assert_eq!(property.evaluator, "keyframe");
    assert_eq!(property.keyframes().len(), 1);
    assert_eq!(property.keyframes()[0].time, OrderedFloat(1.25));
}

#[test]
fn clip_move_updates_timing_and_parent_without_reordering_horizontal_drags() {
    let mut project = Project::new("Move Test");
    let (composition_id, first_track_id) = add_composition(&mut project, "Comp");
    let target = Track::new("Target");
    let target_id = target.id;
    project.add_track(target);
    project
        .attach_track_to_composition(composition_id, target_id)
        .unwrap();

    let first_id = add_clip(&mut project, first_track_id, "First", 1.0);
    let moving_id = add_clip(&mut project, first_track_id, "Moving", 1.0);
    let last_id = add_clip(&mut project, first_track_id, "Last", 1.0);
    let metadata_connection_id = project
        .connect_ports(
            PortAddress::new(PortOwner::Track(first_track_id), TIME_PORT),
            PortAddress::new(PortOwner::Clip(moving_id), TIME_PORT),
        )
        .unwrap();
    let project = Arc::new(RwLock::new(project));

    ClipHandler::move_clip_to_track_at_index(
        &project,
        composition_id,
        first_track_id,
        moving_id,
        first_track_id,
        3.0,
        None,
    )
    .unwrap();
    {
        let project = project.read().unwrap();
        assert_eq!(
            project.get_track(first_track_id).unwrap().clip_ids,
            vec![first_id, moving_id, last_id]
        );
        assert_eq!(
            project.get_clip(moving_id).unwrap().start_time,
            OrderedFloat(3.0)
        );
        assert_eq!(
            project
                .connections
                .iter()
                .find(|connection| connection.id == metadata_connection_id)
                .unwrap()
                .from,
            PortAddress::new(PortOwner::Track(first_track_id), TIME_PORT)
        );
    }

    ClipHandler::move_clip_to_track_at_index(
        &project,
        composition_id,
        first_track_id,
        moving_id,
        target_id,
        4.0,
        Some(0),
    )
    .unwrap();
    let project = project.read().unwrap();
    assert_eq!(
        project.get_track(first_track_id).unwrap().clip_ids,
        vec![first_id, last_id]
    );
    assert_eq!(
        project.get_track(target_id).unwrap().clip_ids,
        vec![moving_id]
    );
    assert_eq!(project.find_track_for_clip(moving_id), Some(target_id));
    let metadata_connection = project
        .connections
        .iter()
        .find(|connection| connection.id == metadata_connection_id)
        .unwrap();
    assert_eq!(metadata_connection.id, metadata_connection_id);
    assert_eq!(metadata_connection.order, 0);
    assert_eq!(
        metadata_connection.from,
        PortAddress::new(PortOwner::Track(target_id), TIME_PORT)
    );
    assert_eq!(
        metadata_connection.to,
        PortAddress::new(PortOwner::Clip(moving_id), TIME_PORT)
    );
    assert_eq!(
        project.get_clip(moving_id).unwrap().start_time,
        OrderedFloat(4.0)
    );
    assert_eq!(
        project.get_clip(moving_id).unwrap().duration,
        OrderedFloat(5.0)
    );
}

#[test]
fn removal_cleans_owned_registries_output_pointers_references_and_connections() {
    let mut project = Project::new("cleanup");
    let (first_composition_id, first_track_id) = add_composition(&mut project, "first");
    let first_clip_id = add_clip(&mut project, first_track_id, "source clip", 0.0);
    let source_id = add_node(&mut project, first_clip_id, solid("source"));
    project
        .set_output_node(NodeContainer::Clip(first_clip_id), Some(source_id))
        .unwrap();

    let (second_composition_id, second_track_id) = add_composition(&mut project, "second");
    let second_clip_id = add_clip(&mut project, second_track_id, "reference clip", 0.0);
    let reference_id = add_node(
        &mut project,
        second_clip_id,
        Node::new(
            "reference",
            NodeContent::Reference(ReferenceContent {
                target_id: first_composition_id,
                sync_global_time: false,
            }),
        ),
    );
    project
        .connect_ports(
            PortAddress::new(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(reference_id), IMAGE_INPUT_PORT),
        )
        .unwrap();

    project.remove_composition(first_composition_id).unwrap();
    assert!(project.get_composition(first_composition_id).is_none());
    assert!(project.get_track(first_track_id).is_none());
    assert!(project.get_clip(first_clip_id).is_none());
    assert!(project.get_node(source_id).is_none());
    assert!(project.get_node(reference_id).is_none());
    assert!(project.connections.is_empty());
    assert!(project.get_composition(second_composition_id).is_some());
    assert!(
        project
            .get_clip(second_clip_id)
            .unwrap()
            .node_ids
            .is_empty()
    );
    assert!(project.validate_containment().is_empty());
}
