use std::collections::HashMap;

use crate::model::BlendMode;
use crate::model::authoring::{
    AuthoringProject, MediaTime, RationalRate, SourceRef, TimeMap, TimelineInterval, TimelineItem,
    TimelineItemId, Transition, TransitionAlignment, TransitionId, TransitionProcessor,
};
use crate::model::frame::color::Color;
use crate::model::project::property::PropertyMap;

use super::RenderPlanCompiler;

fn seconds(value: i64) -> MediaTime {
    MediaTime::new(value, 1).expect("whole seconds")
}

#[test]
fn compiler_keeps_transition_hierarchical_with_two_schedule_invocations() {
    let mut project = AuthoringProject::new(
        "compiled transition",
        320,
        180,
        RationalRate::new(30, 1).unwrap(),
        seconds(20),
    )
    .unwrap();
    let timeline_id = project.root_timeline_id;
    let track_id = project.timelines[&timeline_id].track_order[0];
    let from = TimelineItemId::new();
    let to = TimelineItemId::new();
    for (item_id, layer, red, start, duration) in [
        (from, 0, 32, seconds(0), seconds(5)),
        (to, 1, 224, seconds(5), seconds(5)),
    ] {
        project.items.insert(
            item_id,
            TimelineItem {
                id: item_id,
                track_id,
                name: format!("Clip {layer}"),
                source: SourceRef::Solid {
                    color: Color {
                        r: red,
                        g: 0,
                        b: 0,
                        a: 255,
                    },
                },
                interval: TimelineInterval::new(start, duration).unwrap(),
                time_map: TimeMap::default(),
                layer,
                parent: None,
                blend_mode: BlendMode::Normal,
                authored_properties: PropertyMap::new(),
            },
        );
    }
    let transition_id = TransitionId::new();
    project.transitions.insert(
        transition_id,
        Transition {
            id: transition_id,
            timeline_id,
            from_item_id: from,
            to_item_id: to,
            edit_point: seconds(5),
            duration: seconds(4),
            alignment: TransitionAlignment::CenteredOnEdit,
            processor: TransitionProcessor::cross_dissolve(),
            parameters: HashMap::new(),
        },
    );

    let plan = RenderPlanCompiler::compile(&project).unwrap();
    let compiled = &plan.timelines[&timeline_id];
    assert_eq!(compiled.transitions.len(), 1);
    let transition = &compiled.transitions[0];
    assert_eq!(transition.id, transition_id);
    assert_eq!(
        compiled.schedule[transition.from.schedule_index].item_id,
        from
    );
    assert_eq!(compiled.schedule[transition.to.schedule_index].item_id, to);
    assert_eq!(
        transition.output_schedule_index,
        transition.to.schedule_index
    );
    assert_eq!(transition.progress.interval().start, seconds(3));
    assert_eq!(transition.progress.sample_at(seconds(2)).unwrap(), 0.0);
    assert_eq!(transition.progress.sample_at(seconds(3)).unwrap(), 0.0);
    assert_eq!(transition.progress.sample_at(seconds(5)).unwrap(), 0.5);
    assert_eq!(transition.progress.sample_at(seconds(7)).unwrap(), 1.0);
    assert_eq!(transition.progress.sample_at(seconds(8)).unwrap(), 1.0);
    assert_eq!(transition.from.required_hidden_handle.before, seconds(0));
    assert_eq!(transition.from.required_hidden_handle.after, seconds(2));
    assert_eq!(transition.to.required_hidden_handle.before, seconds(2));
    assert_eq!(transition.to.required_hidden_handle.after, seconds(0));
    assert!(plan.module_definitions.is_empty());
    assert!(plan.module_invocations.is_empty());
}

#[test]
fn transition_changes_invalidate_only_its_timeline_schedule_cache() {
    let mut project = AuthoringProject::new(
        "transition fingerprint",
        320,
        180,
        RationalRate::new(30, 1).unwrap(),
        seconds(20),
    )
    .unwrap();
    let timeline_id = project.root_timeline_id;
    let track_id = project.timelines[&timeline_id].track_order[0];
    let ids = [TimelineItemId::new(), TimelineItemId::new()];
    for (layer, item_id, start, duration) in [
        (0, ids[0], seconds(0), seconds(6)),
        (1, ids[1], seconds(4), seconds(6)),
    ] {
        project.items.insert(
            item_id,
            TimelineItem {
                id: item_id,
                track_id,
                name: format!("Clip {layer}"),
                source: SourceRef::Solid {
                    color: Color::white(),
                },
                interval: TimelineInterval::new(start, duration).unwrap(),
                time_map: TimeMap::default(),
                layer,
                parent: None,
                blend_mode: BlendMode::Normal,
                authored_properties: PropertyMap::new(),
            },
        );
    }
    let mut cache = super::RenderPlanCache::default();
    let (_, initial) = cache.compile(&project).unwrap();
    assert_eq!(initial.compiled_timelines, 1);

    let id = TransitionId::new();
    project.transitions.insert(
        id,
        Transition {
            id,
            timeline_id,
            from_item_id: ids[0],
            to_item_id: ids[1],
            edit_point: seconds(5),
            duration: seconds(2),
            alignment: TransitionAlignment::CenteredOnEdit,
            processor: TransitionProcessor::cross_dissolve(),
            parameters: HashMap::new(),
        },
    );
    let (changed_plan, changed) = cache.compile(&project).unwrap();
    assert_eq!(changed.compiled_timelines, 1);
    assert_eq!(changed.reused_timelines, 0);
    let transition = &changed_plan.timelines[&timeline_id].transitions[0];
    assert!(transition.from.required_hidden_handle.is_empty());
    assert!(transition.to.required_hidden_handle.is_empty());
}
