use library::model::asset::{Asset, AssetKind};
use library::model::frame::entity::FrameContent;
use library::model::project::Composition;
use library::model::property::{Property, PropertyValue};
use library::model::{MediaContent, Node, NodeContent, Project};
use library::plugin::entity_converter::{FrameEvaluationContext, VideoEntityConverterPlugin};
use library::plugin::properties::ConstantEvaluator;
use library::plugin::{EntityConverterPlugin, PropertyEvaluatorRegistry};
use std::sync::Arc;

#[test]
fn video_converter_preserves_clip_local_source_time_and_stream() {
    let (composition, _first_track) = Composition::new("Test Comp", 1920, 1080, 30.0, 10.0);
    let mut registry = PropertyEvaluatorRegistry::new();
    registry.register("constant", Arc::new(ConstantEvaluator));
    let registry = Arc::new(registry);

    let mut project = Project::new("converter test");
    let mut asset = Asset::new("source", "test.mp4", AssetKind::Video);
    asset.fps = Some(60.0);
    asset.stream_index = Some(2);
    let asset_id = asset.id;
    project.assets.push(asset);

    let mut node = Node::new(
        "Test Layer",
        NodeContent::Media(MediaContent {
            asset_id,
            stream_index: None,
            audio_stream_index: None,
        }),
    );
    node.properties.set(
        "file_path".to_string(),
        Property::constant(PropertyValue::String(
            "stale-path-is-not-used.mp4".to_string(),
        )),
    );

    let plugin_manager = Arc::new(library::plugin::PluginManager::new());
    let context = FrameEvaluationContext {
        project: &project,
        composition: &composition,
        property_evaluators: &registry,
        plugin_manager: &plugin_manager,
        resolved_inputs: None,
    };

    // Clip owns start/trim/stretch and passes the resulting source-local time
    // unchanged. The loader, not average FPS, maps it to stream PTS.
    let result = VideoEntityConverterPlugin::new()
        .convert_entity(&context, &node, 5.0)
        .expect("video converter should produce a frame");
    assert!(matches!(
        result.content,
        FrameContent::Video {
            source_time: 5.0,
            stream_index: Some(2),
            ref surface,
        } if surface.file_path == "test.mp4"
    ));
}
