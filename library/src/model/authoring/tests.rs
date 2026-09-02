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
        assets: Vec::new(),
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
