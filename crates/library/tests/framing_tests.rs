use anyhow::{Context, Result};
use library::model::Project;
use library::model::asset::{Asset, AssetKind};
use library::model::frame::entity::FrameContent;
use library::model::project::Composition;
use library::model::property::{Property, PropertyValue};
use library::plugin::entity_converter::{FrameEvaluationContext, VideoEntityConverterPlugin};
use library::plugin::properties::ConstantEvaluator;
use library::plugin::{EntityConverterPlugin, PropertyEvaluatorRegistry};
use std::sync::Arc;

use support::media_node_for_canvas;

#[test]
fn video_converter_preserves_clip_local_source_time_and_stream() -> Result<()> {
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

    let node = media_node_for_canvas(
        "Test Layer",
        MediaNodeRequest::Video {
            asset_id,
            file_path: "test.mp4".to_string(),
            stream_index: None,
            audio_stream_index: None,
        },
        1920,
        1080,
        1920,
        1080,
    );
    let mut persisted = serde_json::to_value(node)?;
    let properties = persisted
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
        .context("serialized media Node must contain a property map")?;
    properties.insert(
        "file_path".to_string(),
        serde_json::to_value(Property::constant(PropertyValue::String(
            "stale-path-is-not-used.mp4".to_string(),
        )))?,
    );
    let node = serde_json::from_value(persisted)?;

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
        .context("video converter should produce a frame")?;
    assert!(
        VideoEntityConverterPlugin::new()
            .get_property_definitions(1920, 1080, 1920, 1080)
            .is_empty(),
        "video color interpretation must come from Asset + Project, not Node properties"
    );
    assert!(matches!(
        result.content,
        FrameContent::Video {
            source_time: 5.0,
            stream_index: Some(2),
            ref surface,
        } if surface.file_path == "test.mp4"
            && surface.input_color_space.is_none()
            && surface.output_color_space.is_none()
    ));

    let mut legacy = serde_json::to_value(&node)?;
    let legacy_properties = legacy
        .get_mut("properties")
        .and_then(serde_json::Value::as_object_mut)
        .context("serialized Media Node must contain properties")?;
    for (key, value) in [
        ("input_color_space", "Configless Input"),
        ("output_color_space", "Configless Output"),
    ] {
        legacy_properties.insert(
            key.to_string(),
            serde_json::to_value(Property::constant(PropertyValue::String(value.to_string())))?,
        );
    }
    let legacy_node = serde_json::from_value(legacy)?;
    assert!(
        VideoEntityConverterPlugin::new()
            .convert_entity(&context, &legacy_node, 5.0)
            .is_none(),
        "non-empty config-less legacy color fields must fail closed until explicitly repaired"
    );
    Ok(())
}
mod support;

use library::editor::project_service::MediaNodeRequest;
