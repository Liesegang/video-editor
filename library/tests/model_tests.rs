mod support;

use anyhow::{Context, Result, anyhow, bail};
use std::sync::{Arc, RwLock};

use library::editor::handlers::clip_handler::ClipHandler;
use library::editor::project_service::GeneratorNodeRequest;
use library::model::frame::color::Color;
use library::model::project::{
    Composition, NodeContainer, PortAddress, PortOwner, Project, TIME_PORT,
};
use library::model::property::{Property, PropertyMap, PropertyValue, Vec2, Vec3, Vec4};
use library::model::{
    Clip, CompositionInstanceContent, GeneratorContent, MediaContent, Node, NodeContent, Track,
};
use ordered_float::OrderedFloat;
use uuid::Uuid;

use support::generator_node;

fn add_composition(project: &mut Project, name: &str) -> (Uuid, Uuid) {
    let (composition, track) = Composition::new(name, 1920, 1080, 30.0, 10.0);
    let composition_id = composition.id;
    let track_id = track.id;
    assert!(
        project.add_track(track).is_ok(),
        "container structural Merge insertion must succeed"
    );
    assert!(
        project.add_composition(composition).is_ok(),
        "container structural Merge insertion must succeed"
    );
    (composition_id, track_id)
}

fn add_clip(project: &mut Project, track_id: Uuid, name: &str, start: f64) -> Result<Uuid> {
    let clip = Clip::new(name, start, 5.0);
    let clip_id = clip.id;
    project.add_clip(clip);
    project.attach_clip_to_track(track_id, clip_id)?;
    Ok(clip_id)
}

fn solid(name: &str) -> Node {
    generator_node(
        name,
        GeneratorNodeRequest::Solid {
            color: Color::default(),
        },
    )
}

fn add_node(project: &mut Project, clip_id: Uuid, node: Node) -> Result<Uuid> {
    let node_id = node.id;
    project.add_node(node);
    project.attach_node_to_container(NodeContainer::Clip(clip_id), node_id)?;
    Ok(node_id)
}

fn read_project(project: &RwLock<Project>) -> Result<std::sync::RwLockReadGuard<'_, Project>> {
    project
        .read()
        .map_err(|error| anyhow!("project lock poisoned: {error}"))
}

#[test]
fn property_serialization_roundtrip() -> Result<()> {
    let mut properties = PropertyMap::new();
    properties.set(
        "opacity".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(0.5))),
    );

    let json = serde_json::to_string(&properties).context("property map should serialize")?;
    let loaded: PropertyMap =
        serde_json::from_str(&json).context("property map should deserialize")?;

    assert_eq!(
        loaded.get("opacity").and_then(Property::value),
        Some(&PropertyValue::Number(OrderedFloat(0.5)))
    );
    Ok(())
}

#[test]
fn every_vector_arity_roundtrips_without_collapsing_to_vec2() -> Result<()> {
    let values = [
        PropertyValue::Vec2(Vec2 {
            x: OrderedFloat(1.0),
            y: OrderedFloat(2.0),
        }),
        PropertyValue::Vec3(Vec3 {
            x: OrderedFloat(1.0),
            y: OrderedFloat(2.0),
            z: OrderedFloat(3.0),
        }),
        PropertyValue::Vec4(Vec4 {
            x: OrderedFloat(1.0),
            y: OrderedFloat(2.0),
            z: OrderedFloat(3.0),
            w: OrderedFloat(4.0),
        }),
    ];

    for value in values {
        let json = serde_json::to_string(&value)?;
        let loaded: PropertyValue = serde_json::from_str(&json)?;
        assert_eq!(loaded, value);
    }
    Ok(())
}

#[test]
fn media_audio_stream_selection_is_required_pre_v1_state() -> Result<()> {
    let asset_id = Uuid::new_v4();
    let missing_audio_stream = serde_json::json!({
        "asset_id": asset_id,
        "stream_index": 0
    });
    let error = match serde_json::from_value::<MediaContent>(missing_audio_stream) {
        Ok(_) => bail!("MediaContent without audio_stream_index unexpectedly decoded"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("audio_stream_index"));

    let media = serde_json::from_value::<MediaContent>(serde_json::json!({
        "asset_id": asset_id,
        "stream_index": 0,
        "audio_stream_index": 2
    }))?;
    assert_eq!(media.stream_index, Some(0));
    assert_eq!(media.audio_stream_index, Some(2));
    Ok(())
}

#[test]
fn clip_timing_metadata_validates_freeze_and_never_duplicates_structural_values() -> Result<()> {
    let definitions = Clip::timing_property_definitions();
    assert_eq!(definitions.len(), 4);
    assert_eq!(
        definitions
            .iter()
            .map(|definition| definition.name())
            .collect::<Vec<_>>(),
        vec!["start_time", "duration", "trim_in", "time_stretch"]
    );
    let stretch = Clip::timing_property_definition("time_stretch")
        .context("time_stretch definition must exist")?;
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
        library::PropertyOwner::Clip(clip_id),
        "time_stretch",
        42.0,
        PropertyValue::Number(OrderedFloat(0.0)),
        None,
    )?;

    let read = read_project(&shared)?;
    let clip = read.get_clip(clip_id).context("freeze Clip must exist")?;
    assert_eq!(clip.time_stretch, OrderedFloat(0.0));
    for definition in definitions {
        assert!(
            clip.properties.get(definition.name()).is_none(),
            "structural {} must not be copied into PropertyMap",
            definition.name()
        );
    }
    let saved = read.save()?;
    drop(read);
    let loaded = Project::load(&saved)?;
    let clip = loaded
        .get_clip(clip_id)
        .context("round-tripped freeze Clip must exist")?;
    assert_eq!(clip.time_stretch, OrderedFloat(0.0));
    assert_eq!(clip.local_time(999.0), clip.trim_in.into_inner());
    assert!(
        definitions
            .iter()
            .all(|definition| clip.properties.get(definition.name()).is_none())
    );

    let before = read_project(&shared)?.clone();
    assert!(
        ClipHandler::update_property_or_keyframe(
            &shared,
            library::PropertyOwner::Clip(clip_id),
            "time_stretch",
            0.0,
            PropertyValue::Number(OrderedFloat(-1.0)),
            None,
        )
        .is_err()
    );
    assert_eq!(*read_project(&shared)?, before);
    Ok(())
}

#[test]
fn node_properties_are_the_only_generator_value_authority() -> Result<()> {
    let mut node = generator_node(
        "Text",
        GeneratorNodeRequest::Text {
            text: "before".to_string(),
            font: "Arial".to_string(),
        },
    );
    node.set_property(
        "font_family".to_string(),
        Property::constant(PropertyValue::String("Arial".to_string())),
    )
    .map_err(|error| anyhow!(error))?;
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

    assert_eq!(
        node.properties().get_string("text").as_deref(),
        Some("after")
    );
    assert_eq!(
        node.content(),
        &NodeContent::Generator(GeneratorContent::Text)
    );
    assert_eq!(clip.start_time, OrderedFloat(2.5));
    let serialized = serde_json::to_value(node)?;
    assert_eq!(serialized["content"]["data"], "Text");
    assert!(serialized.get("start_time").is_none());
    assert!(serialized.get("duration").is_none());
    Ok(())
}

#[test]
fn explicit_keyframe_insert_promotes_a_constant_property() -> Result<()> {
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

    let property = properties
        .get("opacity")
        .context("opacity property must remain present")?;
    assert_eq!(property.evaluator, "keyframe");
    assert_eq!(property.keyframes().len(), 1);
    assert_eq!(property.keyframes()[0].time, OrderedFloat(1.25));
    Ok(())
}

#[test]
fn clip_move_updates_timing_and_parent_without_reordering_horizontal_drags() -> Result<()> {
    let mut project = Project::new("Move Test");
    let (composition_id, first_track_id) = add_composition(&mut project, "Comp");
    let target = Track::new("Target");
    let target_id = target.id;
    assert!(
        project.add_track(target).is_ok(),
        "container structural Merge insertion must succeed"
    );
    project.attach_track_to_composition(composition_id, target_id)?;

    let first_id = add_clip(&mut project, first_track_id, "First", 1.0)?;
    let moving_id = add_clip(&mut project, first_track_id, "Moving", 1.0)?;
    let last_id = add_clip(&mut project, first_track_id, "Last", 1.0)?;
    let metadata_connection_id = project.connect_ports(
        PortAddress::new(PortOwner::Track(first_track_id), TIME_PORT),
        PortAddress::new(PortOwner::Clip(moving_id), TIME_PORT),
    )?;
    let project = Arc::new(RwLock::new(project));

    ClipHandler::move_clip_to_track_at_index(
        &project,
        composition_id,
        first_track_id,
        moving_id,
        first_track_id,
        3.0,
        None,
    )?;
    {
        let project = read_project(&project)?;
        assert_eq!(
            project
                .get_track(first_track_id)
                .context("source Track must exist")?
                .clip_ids,
            vec![first_id, moving_id, last_id]
        );
        assert_eq!(
            project
                .get_clip(moving_id)
                .context("moving Clip must exist")?
                .start_time,
            OrderedFloat(3.0)
        );
        assert_eq!(
            project
                .connections
                .iter()
                .find(|connection| connection.id == metadata_connection_id)
                .context("metadata connection must survive horizontal move")?
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
    )?;
    let project = read_project(&project)?;
    assert_eq!(
        project
            .get_track(first_track_id)
            .context("source Track must exist after move")?
            .clip_ids,
        vec![first_id, last_id]
    );
    assert_eq!(
        project
            .get_track(target_id)
            .context("target Track must exist after move")?
            .clip_ids,
        vec![moving_id]
    );
    assert_eq!(project.find_track_for_clip(moving_id), Some(target_id));
    let metadata_connection = project
        .connections
        .iter()
        .find(|connection| connection.id == metadata_connection_id)
        .context("metadata connection must survive cross-Track move")?;
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
        project
            .get_clip(moving_id)
            .context("moved Clip must exist")?
            .start_time,
        OrderedFloat(4.0)
    );
    assert_eq!(
        project
            .get_clip(moving_id)
            .context("moved Clip must exist")?
            .duration,
        OrderedFloat(5.0)
    );
    Ok(())
}

#[test]
fn removal_cleans_owned_registries_output_pointers_instances_and_connections() -> Result<()> {
    let mut project = Project::new("cleanup");
    let (first_composition_id, first_track_id) = add_composition(&mut project, "first");
    let first_clip_id = add_clip(&mut project, first_track_id, "source clip", 0.0)?;
    let source_id = add_node(&mut project, first_clip_id, solid("source"))?;
    project.set_output_node(NodeContainer::Clip(first_clip_id), Some(source_id))?;

    let (second_composition_id, second_track_id) = add_composition(&mut project, "second");
    let second_clip_id = add_clip(
        &mut project,
        second_track_id,
        "composition instance clip",
        0.0,
    )?;
    let instance_id = add_node(
        &mut project,
        second_clip_id,
        Node::new_composition_instance(
            "composition instance",
            CompositionInstanceContent {
                composition_id: first_composition_id,
            },
        ),
    )?;
    project
        .remove_composition(first_composition_id)
        .context("first Composition must be removable")?;
    assert!(project.get_composition(first_composition_id).is_none());
    assert!(project.get_track(first_track_id).is_none());
    assert!(project.get_clip(first_clip_id).is_none());
    assert!(project.get_node(source_id).is_none());
    assert!(project.get_node(instance_id).is_none());
    assert!(project.connections.iter().all(|connection| {
        !matches!(
            connection.from.owner,
            PortOwner::Composition(id) if id == first_composition_id
        ) && !matches!(connection.from.owner, PortOwner::Track(id) if id == first_track_id)
            && !matches!(connection.from.owner, PortOwner::Clip(id) if id == first_clip_id)
            && !matches!(connection.from.owner, PortOwner::Node(id) if id == source_id || id == instance_id)
            && !matches!(connection.to.owner, PortOwner::Node(id) if id == source_id || id == instance_id)
    }));
    assert!(project.get_composition(second_composition_id).is_some());
    assert!(
        project
            .get_clip(second_clip_id)
            .context("unrelated Clip must remain")?
            .node_ids
            .is_empty()
    );
    assert!(project.validate_containment().is_empty());
    Ok(())
}
