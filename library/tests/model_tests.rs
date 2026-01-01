use library::model::project::asset::{Asset, AssetKind};
use library::model::project::project::{Composition, Project};
use library::model::project::property::PropertyMap;
use library::model::project::{Node, TrackClip, TrackData};

use ordered_float::OrderedFloat;
use uuid::Uuid;

#[test]
fn test_project_serialization_roundtrip() {
    let mut project = Project::new("Test Project");

    // Add Export Config
    project.export.container = Some("mp4".to_string());

    // Add Asset
    let asset_id = Uuid::new_v4();
    let mut asset = Asset::new("My Video", "/path/to/video.mp4", AssetKind::Video);
    asset.id = asset_id;
    asset.fps = Some(60.0);
    project.assets.push(asset);

    // Add Composition with root track
    let (comp, root_track) = Composition::new("My Comp", 1920, 1080, 30.0, 10.0);
    let root_id = comp.root_track_id;
    project.add_node(Node::Track(root_track));

    // Create a clip and add to root track
    let mut clip = TrackClip::new(
        Uuid::new_v4(),
        Some(asset_id),
        library::model::project::TrackClipKind::Video,
        0,
        100,
        100,
        Some(100),
        60.0,
        PropertyMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    clip.source_begin_frame = 0;
    let clip_id = clip.id;
    project.add_node(Node::Clip(clip));
    project.get_track_mut(root_id).unwrap().add_child(clip_id);

    project.add_composition(comp);

    // Serialize
    let json = project.save().expect("Failed to serialize project");
    println!("Serialized JSON: {}", json);

    // Deserialize
    let loaded_project = Project::load(&json).expect("Failed to deserialize project");

    // Assert
    assert_eq!(
        project, loaded_project,
        "Roundtrip failed: Projects are not equal"
    );
    assert_eq!(loaded_project.assets.len(), 1);
    assert_eq!(loaded_project.assets[0].fps, Some(60.0));
    assert_eq!(loaded_project.compositions.len(), 1);

    // Check nodes registry has the clip
    assert!(
        loaded_project.get_clip(clip_id).is_some(),
        "Clip should be in nodes registry"
    );
}

#[test]
fn test_property_serialization() {
    let mut props = PropertyMap::new();
    props.set(
        "opacity".to_string(),
        library::model::project::property::Property::constant(
            library::model::project::property::PropertyValue::Number(OrderedFloat(0.5)),
        ),
    );

    let json = serde_json::to_string(&props).expect("Failed to serialize props");
    let loaded_props: PropertyMap =
        serde_json::from_str(&json).expect("Failed to deserialize props");

    let val = loaded_props.get("opacity").expect("Missing opacity");
    if let library::model::project::property::PropertyValue::Number(n) = val.value().unwrap() {
        assert_eq!(*n, OrderedFloat(0.5));
    } else {
        panic!("Wrong value type");
    }
}

#[test]
fn test_node_based_structure() {
    let mut project = Project::new("Node Test");

    // Create composition with root track
    let (comp, root_track) = Composition::new("Test Comp", 1920, 1080, 30.0, 10.0);
    let root_id = comp.root_track_id;
    project.add_node(Node::Track(root_track));
    project.add_composition(comp);

    // Add a child track
    let child_track = TrackData::new("Child Track");
    let child_id = child_track.id;
    project.add_node(Node::Track(child_track));
    project.get_track_mut(root_id).unwrap().add_child(child_id);

    // Add clips to child track
    let mut clip1 = TrackClip::new(
        Uuid::new_v4(),
        None,
        library::model::project::TrackClipKind::Image,
        0,
        50,
        100,
        Some(50),
        30.0,
        PropertyMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    clip1.set_constant_property(
        "file_path",
        library::model::project::property::PropertyValue::String("/path/to/image.png".to_string()),
    );
    clip1.source_begin_frame = 0;
    let mut clip2 = TrackClip::new(
        Uuid::new_v4(),
        None,
        library::model::project::TrackClipKind::Image,
        51,
        100,
        100,
        Some(50),
        30.0,
        PropertyMap::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );
    clip2.set_constant_property(
        "file_path",
        library::model::project::property::PropertyValue::String("/path/to/image2.png".to_string()),
    );
    clip2.source_begin_frame = 0;
    let clip1_id = clip1.id;
    let clip2_id = clip2.id;
    project.add_node(Node::Clip(clip1));
    project.add_node(Node::Clip(clip2));
    project.get_track_mut(child_id).unwrap().add_child(clip1_id);
    project.get_track_mut(child_id).unwrap().add_child(clip2_id);

    // Verify structure
    assert_eq!(project.all_tracks().count(), 2, "Should have 2 tracks");
    assert_eq!(project.all_clips().count(), 2, "Should have 2 clips");

    // Verify hierarchy
    let root_track = project.get_track(root_id).unwrap();
    assert_eq!(root_track.child_ids.len(), 1, "Root should have 1 child");
    assert_eq!(
        root_track.child_ids[0], child_id,
        "Child should be the child track"
    );

    let child_track = project.get_track(child_id).unwrap();
    assert_eq!(
        child_track.child_ids.len(),
        2,
        "Child track should have 2 children (clips)"
    );

    // Test O(1) clip lookup
    assert!(project.get_clip(clip1_id).is_some());
    assert!(project.get_clip(clip2_id).is_some());
}
