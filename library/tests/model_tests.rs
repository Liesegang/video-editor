use library::model::asset::{Asset, AssetKind};
use library::model::project::{Composite, Project};
use library::model::property::{Property, PropertyMap, PropertyValue};
use library::model::{Layer, LayerContent, MediaContent, Node, Track};

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
    let (comp, root_track) = Composite::new("My Comp", 1920, 1080, 30.0, 10.0);
    let root_id = comp.root_track_id;
    project.add_node(Node::Track(root_track));

    // Create a clip and add to root track
    let content = LayerContent::Media(MediaContent {
        asset_id,
        stream_index: None,
    });

    let mut layer = Layer::new("My Clip", 0.0, 30.0, content);
    layer.trim_in = OrderedFloat(0.0);

    let layer_id = layer.id;
    project.add_node(Node::Layer(layer));
    project
        .get_track_mut(root_id)
        .unwrap()
        .children
        .push(layer_id);

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
        loaded_project.get_layer(layer_id).is_some(),
        "Layer should be in nodes registry"
    );
}

#[test]
fn test_property_serialization() {
    let mut props = PropertyMap::new();
    props.set(
        "opacity".to_string(),
        Property::constant(PropertyValue::Number(OrderedFloat(0.5))),
    );

    let json = serde_json::to_string(&props).expect("Failed to serialize props");
    let loaded_props: PropertyMap =
        serde_json::from_str(&json).expect("Failed to deserialize props");

    let val = loaded_props.get("opacity").expect("Missing opacity");
    if let PropertyValue::Number(n) = val.value().unwrap() {
        assert_eq!(*n, OrderedFloat(0.5));
    } else {
        panic!("Wrong value type");
    }
}

#[test]
fn test_node_based_structure() {
    let mut project = Project::new("Node Test");

    // Create composition with root track
    let (comp, root_track) = Composite::new("Test Comp", 1920, 1080, 30.0, 10.0);
    let root_id = comp.root_track_id;
    project.add_node(Node::Track(root_track));
    project.add_composition(comp);

    // Add a child track
    let child_track = Track::new("Child Track");
    let child_id = child_track.id;
    project.add_node(Node::Track(child_track));
    project
        .get_track_mut(root_id)
        .unwrap()
        .children
        .push(child_id);

    // Add clips to child track
    // Mock generic content for test
    let content1 = LayerContent::Generator(library::model::GeneratorContent::Solid {
        color: library::model::frame::color::Color::default(),
    });

    let mut layer1 = Layer::new("Layer 1", 0.0, 5.0, content1);
    layer1.properties.set(
        "file_path".to_string(),
        Property::constant(PropertyValue::String("/path/to/image.png".to_string())),
    );

    let content2 = LayerContent::Generator(library::model::GeneratorContent::Solid {
        color: library::model::frame::color::Color::default(),
    });

    let mut layer2 = Layer::new("Layer 2", 5.0, 5.0, content2);
    layer2.properties.set(
        "file_path".to_string(),
        Property::constant(PropertyValue::String("/path/to/image2.png".to_string())),
    );

    let layer1_id = layer1.id;
    let layer2_id = layer2.id;

    project.add_node(Node::Layer(layer1));
    project.add_node(Node::Layer(layer2));

    project
        .get_track_mut(child_id)
        .unwrap()
        .children
        .push(layer1_id);
    project
        .get_track_mut(child_id)
        .unwrap()
        .children
        .push(layer2_id);

    // Verify structure
    let track_count = project
        .nodes
        .values()
        .filter(|n| matches!(n, Node::Track(_)))
        .count();
    assert_eq!(track_count, 2, "Should have 2 tracks");

    let layer_count = project
        .nodes
        .values()
        .filter(|n| matches!(n, Node::Layer(_)))
        .count();
    assert_eq!(layer_count, 2, "Should have 2 layers");

    // Verify hierarchy
    let root_track = project.get_track(root_id).unwrap();
    assert_eq!(root_track.children.len(), 1, "Root should have 1 child");
    assert_eq!(
        root_track.children[0], child_id,
        "Child should be the child track"
    );

    let child_track = project.get_track(child_id).unwrap();
    assert_eq!(
        child_track.children.len(),
        2,
        "Child track should have 2 children (layers)"
    );

    // Test O(1) clip lookup
    assert!(project.get_layer(layer1_id).is_some());
    assert!(project.get_layer(layer2_id).is_some());
}
