use library::model::project::project::{Project, Composition, ExportConfig};
use library::model::project::asset::{Asset, AssetKind};
use library::model::project::{Track, TrackClip, TrackClipKind, EffectConfig};
use library::model::project::property::PropertyMap;

use uuid::Uuid;
use ordered_float::OrderedFloat;

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
    
    // Add Composition
    let mut comp = Composition::new("My Comp", 1920, 1080, 30.0, 10.0);
    // Comp ID is random, let's keep it
    
    // Add Track
    let mut track = Track::new("Video Track");
    
    // Add Clip
    let clip = TrackClip::create_video(
        Some(asset_id),
        "/path/to/video.mp4",
        0,
        100,
        0,
        100,
        60.0
    );
    track.clips.push(clip);
    comp.add_track(track);
    
    project.add_composition(comp);
    
    // Serialize
    let json = project.save().expect("Failed to serialize project");
    println!("Serialized JSON: {}", json);
    
    // Deserialize
    let loaded_project = Project::load(&json).expect("Failed to deserialize project");
    
    // Assert
    assert_eq!(project, loaded_project, "Roundtrip failed: Projects are not equal");
    assert_eq!(loaded_project.assets.len(), 1);
    assert_eq!(loaded_project.assets[0].fps, Some(60.0));
    assert_eq!(loaded_project.compositions.len(), 1);
    assert_eq!(loaded_project.compositions[0].tracks.len(), 1);
}

#[test]
fn test_property_serialization() {
    let mut props = PropertyMap::new();
    props.set(
        "opacity".to_string(), 
        library::model::project::property::Property::constant(
            library::model::project::property::PropertyValue::Number(OrderedFloat(0.5))
        )
    );
    
    let json = serde_json::to_string(&props).expect("Failed to serialize props");
    let loaded_props: PropertyMap = serde_json::from_str(&json).expect("Failed to deserialize props");
    
    // Manual equality check effectively provided by PropertyMap derivation? 
    // PropertyMap might not derive PartialEq correctly if HashMap order differs, 
    // but serde_json::Value comparison handles map order?
    // Actually PropertyMap wraps HashMap, creating identical HashMaps should be Equal.
    
    // We can't easily assert_eq check PropertyMap directly unless we can access inner map or check specific keys
    // Let's check keys
    let val = loaded_props.get("opacity").expect("Missing opacity");
    if let library::model::project::property::PropertyValue::Number(n) = val.value().unwrap() {
        assert_eq!(*n, OrderedFloat(0.5));
    } else {
        panic!("Wrong value type");
    }
}
