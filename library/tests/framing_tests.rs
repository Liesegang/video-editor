use std::sync::Arc;
use library::model::project::project::Composition;
use library::model::project::TrackClip;
use library::framing::entity_converters::{VideoEntityConverter, EntityConverter, FrameEvaluationContext};
use library::plugin::PropertyEvaluatorRegistry;
use library::plugin::properties::ConstantEvaluator;
use library::model::frame::entity::FrameContent;

#[test]
fn test_video_converter_frame_calculation() {
    let comp_fps = 30.0;
    let comp = Composition::new("Test Comp", 1920, 1080, comp_fps, 10.0);
    
    let mut registry = PropertyEvaluatorRegistry::new();
    registry.register("constant", Arc::new(ConstantEvaluator));
    let registry = Arc::new(registry);
    
    let context = FrameEvaluationContext {
        composition: &comp,
        property_evaluators: &registry,
    };
    
    let video_fps = 60.0;
    let clip = TrackClip::create_video(
        None,
        "test.mp4",
        0, // in_frame
        100, // out_frame
        100, // source_begin_frame
        100, // duration_frame
        video_fps
    );
    
    println!("Clip FPS: {}", clip.fps);

    // Test Frame 0 (at 0 sec)
    // Expected: source_frame = 100 + (0/30 * 60) = 100
    let converter = VideoEntityConverter;
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
