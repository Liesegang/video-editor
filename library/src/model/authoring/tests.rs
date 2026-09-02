use std::collections::HashMap;

use ordered_float::OrderedFloat;

use crate::model::frame::color::Color;
use crate::model::project::property::PropertyMap;

use super::*;

fn empty_project() -> AuthoringProject {
    let timeline_id = TimelineId::new();
    let timeline = Timeline {
        id: timeline_id,
        name: "Main".to_string(),
        width: 1920,
        height: 1080,
        fps: OrderedFloat(30.0),
        duration: OrderedFloat(10.0),
        background_color: Color::black(),
        color_profile: "sRGB".to_string(),
        track_order: Vec::new(),
        authored_properties: PropertyMap::new(),
    };
    AuthoringProject {
        name: "Project".to_string(),
        root_timeline_id: timeline_id,
        timelines: HashMap::from([(timeline_id, timeline)]),
        tracks: HashMap::new(),
        items: HashMap::new(),
        module_definitions: HashMap::new(),
        module_instances: HashMap::new(),
        attachments: HashMap::new(),
        signal_bindings: HashMap::new(),
        event_bindings: HashMap::new(),
        data_sources: HashMap::new(),
        generated_items: HashMap::new(),
        overrides: HashMap::new(),
        masks: HashMap::new(),
        transitions: HashMap::new(),
        transcript_documents: HashMap::new(),
        transcript_links: HashMap::new(),
        assets: Vec::new(),
        color_management: Default::default(),
        export: Default::default(),
    }
}

#[test]
fn versionless_pre_v1_project_is_rejected() {
    let error = ProjectDocument::from_json(r#"{"name":"old","compositions":[]}"#)
        .expect_err("versionless Project must not load");
    assert!(error.contains("Unsupported Project format"));
}

#[test]
fn project_document_round_trips_strict_schema() {
    let document = ProjectDocument::new(empty_project());
    let json = document.to_json().expect("Project must serialize");
    let loaded = ProjectDocument::from_json(&json).expect("Project must load");
    assert_eq!(loaded, document);
}

#[test]
fn instance_path_contains_only_authored_placement_ids() {
    let root = TimelineId::new();
    let first = TimelineItemId::new();
    let second = TimelineItemId::new();
    let path = InstancePath::root(root).nested(first).nested(second);
    assert_eq!(path.root_timeline_id, root);
    assert_eq!(path.composition_items, vec![first, second]);
}

#[test]
fn timeline_intervals_are_half_open() {
    let interval = TimelineInterval::new(2.0, 3.0).expect("valid interval");
    assert!(interval.contains(2.0));
    assert!(interval.contains(4.999));
    assert!(!interval.contains(5.0));
}

#[test]
fn generated_identity_ignores_generator_version() {
    let generator = ModuleInstanceId::new();
    let before = GeneratedItem::stable_id(generator, "row-42");
    let after = GeneratedItem::stable_id(generator, "row-42");
    let other = GeneratedItem::stable_id(generator, "row-43");
    assert_eq!(before, after);
    assert_ne!(before, other);
}

#[test]
fn direct_item_edit_invalidates_only_its_timeline_property_target() {
    let project =
        AuthoringProject::new("Edit", 1920, 1080, 30.0, 10.0).expect("Project must be valid");
    let track_id = *project.tracks.keys().next().expect("default Track");
    let mut session = AuthoringSession::new(project).expect("session must open");
    let (item_id, _) = session
        .add_item(
            track_id,
            "Title".to_string(),
            SourceRef::Text {
                text: "Hello".to_string(),
            },
            TimelineInterval::new(0.0, 3.0).expect("valid interval"),
            0,
        )
        .expect("item must be added");
    let change = session
        .set_item_property(
            item_id,
            "opacity".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(0.82)),
            ),
        )
        .expect("property must change");
    assert_eq!(change.invalidations.len(), 1);
    assert!(matches!(
        change.invalidations[0],
        ProjectInvalidation::ItemProperties { item_id: changed, .. } if changed == item_id
    ));
}

#[test]
fn project_file_store_rejects_versionless_input_and_round_trips_atomically() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("project.ruvie");
    let document = ProjectDocument::new(
        AuthoringProject::new("Save", 1280, 720, 24.0, 5.0).expect("Project must be valid"),
    );
    ProjectFileStore::save(&path, &document).expect("Project must save");
    assert_eq!(
        ProjectFileStore::load(&path).expect("Project must load"),
        document
    );
    std::fs::write(&path, r#"{"name":"old","compositions":[]}"#).expect("test fixture must write");
    assert!(ProjectFileStore::load(&path).is_err());
}

#[test]
fn parent_cycles_are_rejected_by_the_timeline_model() {
    let project =
        AuthoringProject::new("Parents", 1920, 1080, 30.0, 10.0).expect("Project must be valid");
    let track_id = *project.tracks.keys().next().expect("default Track");
    let mut session = AuthoringSession::new(project).expect("session must open");
    let (first, _) = session
        .add_item(
            track_id,
            "First".to_string(),
            SourceRef::Text {
                text: "First".to_string(),
            },
            TimelineInterval::new(0.0, 1.0).expect("valid interval"),
            0,
        )
        .expect("first item");
    let (second, _) = session
        .add_item(
            track_id,
            "Second".to_string(),
            SourceRef::Text {
                text: "Second".to_string(),
            },
            TimelineInterval::new(1.0, 1.0).expect("valid interval"),
            0,
        )
        .expect("second item");
    let mut project = session.into_project();
    project.items.get_mut(&first).expect("first").parent = Some(second);
    project.items.get_mut(&second).expect("second").parent = Some(first);
    assert!(project.validate().is_err());
}

#[test]
fn split_asset_item_preserves_continuous_source_time() {
    let mut project =
        AuthoringProject::new("Split", 1920, 1080, 30.0, 10.0).expect("Project must be valid");
    let track_id = *project.tracks.keys().next().expect("default Track");
    let asset = crate::model::asset::Asset::new(
        "source",
        "source.mp4",
        crate::model::asset::AssetKind::Video,
    );
    let asset_id = asset.id;
    project.assets.push(asset);
    let mut session = AuthoringSession::new(project).expect("session must open");
    let (left_id, _) = session
        .add_item(
            track_id,
            "Video".to_string(),
            SourceRef::Asset {
                asset_id,
                time_map: TimeMap {
                    source_start: OrderedFloat(5.0),
                    playback_rate: OrderedFloat(2.0),
                },
            },
            TimelineInterval::new(1.0, 5.0).expect("interval"),
            0,
        )
        .expect("item must be added");
    let (right_id, _) = session.split_item(left_id, 3.0).expect("split");
    let project = session.into_project();

    assert_eq!(project.items[&left_id].interval.duration, OrderedFloat(2.0));
    assert_eq!(project.items[&right_id].interval.start, OrderedFloat(3.0));
    let SourceRef::Asset { time_map, .. } = &project.items[&right_id].source else {
        panic!("Asset source expected");
    };
    assert_eq!(time_map.source_start, OrderedFloat(9.0));
}

#[test]
fn signal_bindings_target_only_published_parameters() {
    let mut project = empty_project();
    let definition_id = ModuleDefinitionId::new();
    let parameter_id = PublishedParameterId::new();
    let node = crate::model::node::Node::new_merge("Internal");
    let node_id = node.id;
    project.module_definitions.insert(
        definition_id,
        ModuleDefinition {
            id: definition_id,
            name: "Effect".to_string(),
            role: ModuleRole::Effect,
            graph: ModuleGraph {
                nodes: HashMap::from([(node_id, node)]),
                connections: Vec::new(),
            },
            output_node_id: Some(node_id),
            published_parameters: vec![PublishedParameter {
                id: parameter_id,
                name: "Amount".to_string(),
                data_type: crate::model::project::connection::PortDataType::Numeric,
                default_value: crate::model::project::property::PropertyValue::Number(
                    OrderedFloat(1.0),
                ),
                target: ModulePortAddress {
                    node_id,
                    port: "amount".to_string(),
                },
            }],
            published_signals: Vec::new(),
            published_actions: Vec::new(),
            version: 1,
        },
    );
    let binding_id = SignalBindingId::new();
    project.signal_bindings.insert(
        binding_id,
        SignalBinding {
            id: binding_id,
            source: SignalSource::AudioEnvelope {
                channel: "music".to_string(),
            },
            scope: BindingScope::Definition { definition_id },
            target_parameter_id: PublishedParameterId::new(),
            mapping: SignalMapping {
                input_min: OrderedFloat(0.0),
                input_max: OrderedFloat(1.0),
                output_min: OrderedFloat(0.0),
                output_max: OrderedFloat(2.0),
                clamp: true,
            },
            operator: BindingOperator::Multiply,
            smoothing_seconds: OrderedFloat(0.05),
            priority: 0,
        },
    );

    assert!(
        project
            .validate()
            .unwrap_err()
            .contains("PublishedParameter")
    );
    project
        .signal_bindings
        .get_mut(&binding_id)
        .unwrap()
        .target_parameter_id = parameter_id;
    project
        .validate()
        .expect("Published parameter target is stable");
}

#[test]
fn ripple_delete_closes_only_the_removed_track_gap() {
    let project = AuthoringProject::new("Ripple", 1920, 1080, 30.0, 20.0).unwrap();
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    let mut session = AuthoringSession::new(project).unwrap();
    let first = session
        .add_item(
            track_id,
            "First".to_string(),
            SourceRef::Text {
                text: "First".to_string(),
            },
            TimelineInterval::new(0.0, 2.0).unwrap(),
            0,
        )
        .unwrap()
        .0;
    let middle = session
        .add_item(
            track_id,
            "Middle".to_string(),
            SourceRef::Text {
                text: "Middle".to_string(),
            },
            TimelineInterval::new(2.0, 2.0).unwrap(),
            0,
        )
        .unwrap()
        .0;
    let last = session
        .add_item(
            track_id,
            "Last".to_string(),
            SourceRef::Text {
                text: "Last".to_string(),
            },
            TimelineInterval::new(5.0, 2.0).unwrap(),
            0,
        )
        .unwrap()
        .0;

    session.delete_item(middle, true).unwrap();
    let project = session.into_project();

    assert!(project.items.contains_key(&first));
    assert!(!project.items.contains_key(&middle));
    assert_eq!(project.items[&last].interval.start, OrderedFloat(3.0));
}
