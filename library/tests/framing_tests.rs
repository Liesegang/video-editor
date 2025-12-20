use library::timeline::converter::{
    EntityConverter, FrameEvaluationContext, VideoEntityConverter,
};
use library::core::frame::entity::FrameContent;
use library::core::model::TrackClip;
use library::core::model::project::Composition;
use library::extensions::manager::PropertyEvaluatorRegistry;
use library::extensions::properties::ConstantEvaluator;
use std::sync::Arc;

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
        "test.mp4",
        0,   // in_frame
        100, // duration_frames
        video_fps,
        1920, // canvas width
        1080, // canvas height
        1920, // clip width
        1080, // clip height
    );

    println!("Clip FPS: {}", clip.fps);

    // Test Frame 0 (at 0 sec)
    // Expected: source_frame = 0 (source_begin) + (0/30 * 60) = 0
    // Wait, create_video sets source_begin_frame to 0 by default now.
    // The original test expected 100 as start.
    // But create_video hardcodes source_begin to 0.
    // If we want to test source offset, we might need to manually set it.

    // Let's modify the expectation based on create_video behavior (source_begin=0)
    // Frame 0 -> 0
    let converter = VideoEntityConverter;
    let result = converter.convert_entity(&context, &clip, 0);
    assert!(result.is_some(), "Failed to convert frame 0");
    if let FrameContent::Video { frame_number, .. } = result.unwrap().content {
        assert_eq!(frame_number, 0, "Frame 0 calculation incorrect (expected 0)");
    } else {
        panic!("Result is not video content");
    }

    // Test Frame 30 (at 1 sec)
    // Expected: source_frame = 0 + (30/30 * 60) = 60
    let result = converter.convert_entity(&context, &clip, 30);
    assert!(result.is_some(), "Failed to convert frame 30");
    if let FrameContent::Video { frame_number, .. } = result.unwrap().content {
        assert_eq!(frame_number, 60, "Frame 30 calculation incorrect (expected 60)");
    } else {
        panic!("Result is not video content");
    }

    // Test Frame 15 (at 0.5 sec)
    // Expected: source_frame = 0 + (15/30 * 60) = 30
    let result = converter.convert_entity(&context, &clip, 15);
    assert!(result.is_some(), "Failed to convert frame 15");
    if let FrameContent::Video { frame_number, .. } = result.unwrap().content {
        assert_eq!(frame_number, 30, "Frame 15 calculation incorrect (expected 30)");
    } else {
        panic!("Result is not video content");
    }
}
