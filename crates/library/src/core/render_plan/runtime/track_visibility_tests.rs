use super::*;
use crate::core::render_plan::RenderPlanCompiler;
use crate::model::authoring::{RationalRate, TRACK_VISIBILITY_PROPERTY, TimeMap, TimelineInterval};
use crate::model::frame::color::Color;
use crate::model::project::property::{Property, PropertyValue, Vec2};
use crate::plugin::PluginManager;

fn seconds(value: i64) -> MediaTime {
    MediaTime::new(value, 1).unwrap()
}

#[test]
fn hidden_visual_track_skips_children_before_their_evaluation() {
    let mut project = AuthoringProject::new(
        "Hidden Track runtime",
        320,
        180,
        RationalRate::new(30, 1).unwrap(),
        seconds(10),
    )
    .unwrap();
    let timeline_id = project.root_timeline_id;
    let track_id = project.timelines[&timeline_id].track_order[0];
    let item_id = TimelineItemId::new();
    let mut authored_properties = PropertyMap::new();
    authored_properties.set(
        "position".to_string(),
        Property::expression(
            "vec2(0, 0)".to_string(),
            PropertyValue::Vec2(Vec2 {
                x: 0.0.into(),
                y: 0.0.into(),
            }),
        ),
    );
    project.items.insert(
        item_id,
        TimelineItem {
            id: item_id,
            track_id,
            name: "Must not evaluate".to_string(),
            source: SourceRef::Solid {
                color: Color::white(),
            },
            interval: TimelineInterval::new(seconds(0), seconds(10)).unwrap(),
            time_map: TimeMap::default(),
            layer: 0,
            parent: None,
            blend_mode: BlendMode::Normal,
            authored_properties,
        },
    );
    project
        .tracks
        .get_mut(&track_id)
        .unwrap()
        .authored_properties
        .set(
            TRACK_VISIBILITY_PROPERTY.to_string(),
            Property::constant(PropertyValue::Boolean(false)),
        );
    project.validate().unwrap();
    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let plugins = PluginManager::default();

    let frame = evaluate_render_plan_frame(&project, &plan, &plugins, 0, 1.0, None)
        .expect("hidden child evaluator must not run");
    let [FrameItem::Group(composition)] = frame.items.as_slice() else {
        panic!("root composition")
    };
    assert!(composition.items.is_empty());

    project
        .tracks
        .get_mut(&track_id)
        .unwrap()
        .authored_properties
        .remove(TRACK_VISIBILITY_PROPERTY);
    let error = evaluate_render_plan_frame(&project, &plan, &plugins, 0, 1.0, None)
        .expect_err("enabled child proves its evaluator is reached")
        .to_string();
    assert!(error.contains("position"), "{error}");
}

#[test]
fn visual_visibility_does_not_change_audio_track_participation_contract() {
    let mut project = AuthoringProject::new(
        "Audio visibility",
        320,
        180,
        RationalRate::new(30, 1).unwrap(),
        seconds(10),
    )
    .unwrap();
    let timeline_id = project.root_timeline_id;
    let track_id = project.timelines[&timeline_id].track_order[0];
    let track = project.tracks.get_mut(&track_id).unwrap();
    track.kind = TimelineTrackKind::Audio;
    track.authored_properties.set(
        TRACK_VISIBILITY_PROPERTY.to_string(),
        Property::constant(PropertyValue::Boolean(false)),
    );
    project.validate().unwrap();
    let track = &project.tracks[&track_id];
    assert!(track.kind.supports_output(MediaOutputKind::Audio));
    assert!(!track.kind.supports_output(MediaOutputKind::Image));
    assert!(!track.is_visually_enabled().unwrap());
}
