use library::builtin::entity_converter::FrameEvaluationContext;
use library::builtin::entity_converter::VideoEntityConverterPlugin;
use library::builtin::properties::ConstantEvaluator;
use library::plugin::EntityConverterPlugin;
use library::plugin::PropertyEvaluatorRegistry;
use library::project::clip::TrackClip;
use library::project::project::Composition;
use library::runtime::entity::FrameContent;
use std::sync::Arc;

#[test]
fn test_video_converter_frame_calculation() {
    let comp_fps = 30.0;
    let (comp, _root_track) = Composition::new("Test Comp", 1920, 1080, comp_fps, 10.0);

    let mut registry = PropertyEvaluatorRegistry::new();
    registry.register("constant", Arc::new(ConstantEvaluator));
    let registry = Arc::new(registry);

    let plugin_manager = Arc::new(library::plugin::PluginManager::new());
    let project = library::project::project::Project::new("Test");
    let context = FrameEvaluationContext {
        composition: &comp,
        property_evaluators: &registry,
        plugin_manager: &plugin_manager,
        project: &project,
    };

    let video_fps = 60.0;
    let mut clip = TrackClip::new(
        uuid::Uuid::new_v4(),
        None,
        library::project::clip::TrackClipKind::Video,
        0,
        100,
        100,
        Some(100),
        video_fps,
        library::project::property::PropertyMap::new(),
    );
    clip.source_begin_frame = 100;
    clip.set_constant_property(
        "file_path",
        library::project::property::PropertyValue::String("test.mp4".to_string()),
    );

    println!("Clip FPS: {}", clip.fps);

    // Test Frame 0 (at 0 sec)
    // Expected: source_frame = 100 + (0/30 * 60) = 100
    let converter = VideoEntityConverterPlugin::new();
    let result = converter.convert_entity(&context, &clip, 0);
    assert!(result.is_some(), "Failed to convert frame 0");
    if let FrameContent::Video { frame_number, .. } = result.unwrap().content {
        assert_eq!(frame_number, 100, "Frame 0 calculation incorrect");
    } else {
        panic!("Result is not video content");
    }

    // Test Frame 30 (at 1 sec)
    // Expected: source_frame = 100 + (30/30 * 60) = 160
    let result = converter.convert_entity(&context, &clip, 30);
    assert!(result.is_some(), "Failed to convert frame 30");
    if let FrameContent::Video { frame_number, .. } = result.unwrap().content {
        assert_eq!(frame_number, 160, "Frame 30 calculation incorrect");
    } else {
        panic!("Result is not video content");
    }

    // Test Frame 15 (at 0.5 sec)
    // Expected: source_frame = 100 + (15/30 * 60) = 130
    let result = converter.convert_entity(&context, &clip, 15);
    assert!(result.is_some(), "Failed to convert frame 15");
    if let FrameContent::Video { frame_number, .. } = result.unwrap().content {
        assert_eq!(frame_number, 130, "Frame 15 calculation incorrect");
    } else {
        panic!("Result is not video content");
    }
}
