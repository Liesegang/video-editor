use std::collections::HashMap;

use crate::editor::{TimelineEditorService, TransitionPlacement};
use crate::model::authoring::{
    MediaTime, ModuleDefinition, ModuleDefinitionSharing, ModuleHostContract, ModuleTemplateOrigin,
    SourceRef, TimelineInterval, TimelineTrackKind, TransitionAlignment, TransitionMediaType,
    TransitionProcessor,
};
use crate::model::frame::color::Color;

use super::{ModuleHost, PlannedSource, RenderPlanCompiler};

fn seconds(value: i64) -> MediaTime {
    MediaTime::new(value, 1).unwrap()
}

fn add_transition(
    service: &TimelineEditorService,
    track_id: crate::model::authoring::TimelineTrackId,
    name: &str,
    start: i64,
) -> crate::model::authoring::TransitionId {
    let source = |red| SourceRef::Solid {
        color: Color {
            r: red,
            g: 0,
            b: 0,
            a: 255,
        },
    };
    let (from, _) = service
        .add_item(
            track_id,
            format!("{name} A"),
            source(24),
            TimelineInterval::new(seconds(start), seconds(7)).unwrap(),
            0,
        )
        .unwrap();
    let (to, _) = service
        .add_item(
            track_id,
            format!("{name} B"),
            source(232),
            TimelineInterval::new(seconds(start + 3), seconds(7)).unwrap(),
            1,
        )
        .unwrap();
    service
        .add_transition(TransitionPlacement {
            from_item_id: from,
            to_item_id: to,
            edit_point: seconds(start + 5),
            duration: seconds(4),
            alignment: TransitionAlignment::CenteredOnEdit,
            processor: TransitionProcessor::cross_dissolve(),
            parameters: HashMap::new(),
        })
        .unwrap()
        .0
}

#[test]
fn compiler_shares_one_transition_definition_across_lightweight_invocations() {
    let service = TimelineEditorService::create_default("Compiled Transition Modules").unwrap();
    let project = service.snapshot().unwrap();
    let timeline_id = project.root_timeline_id;
    let first_track = project.timelines[&timeline_id].track_order[0];
    drop(project);
    let second_track = service
        .add_track(
            timeline_id,
            "Video 2".to_string(),
            TimelineTrackKind::Visual,
        )
        .unwrap()
        .0;
    let first_transition = add_transition(&service, first_track, "First", 0);
    let second_transition = add_transition(&service, second_track, "Second", 10);
    let (definition, contract) = ModuleDefinition::new_transition(
        "Shared Dissolve",
        ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
        TransitionMediaType::Image,
    )
    .unwrap();
    let definition_id = definition.id;
    service.add_module_definition(definition).unwrap();
    let first_instance = service
        .assign_transition_module(first_transition, definition_id)
        .unwrap()
        .0;
    let second_instance = service
        .assign_transition_module(second_transition, definition_id)
        .unwrap()
        .0;
    let project = service.snapshot().unwrap();

    let plan = RenderPlanCompiler::compile(project.as_ref()).expect("RenderPlan");

    assert_eq!(plan.module_definitions.len(), 1);
    assert_eq!(plan.module_invocations.len(), 2);
    let compiled_definition = &plan.module_definitions[&definition_id];
    assert_eq!(
        compiled_definition.host_contract,
        ModuleHostContract::Transition(contract.clone())
    );
    assert!(plan.module_invocations.iter().all(|invocation| {
        invocation.definition_id == definition_id
            && invocation.output_id == contract.output_id
            && invocation.input_bindings.is_empty()
    }));
    for transition_id in [first_transition, second_transition] {
        let host = ModuleHost::Transition {
            timeline_id,
            transition_id,
        };
        let invocation = plan.invocation(host).expect("Transition invocation");
        assert_eq!(invocation.definition_id, definition_id);
        let compiled_transition = plan.timelines[&timeline_id]
            .transitions
            .iter()
            .find(|transition| transition.id == transition_id)
            .unwrap();
        assert_eq!(compiled_transition.module_host, Some(host));
    }
    assert!(
        plan.timelines[&timeline_id]
            .schedule
            .iter()
            .all(|item| item.source == PlannedSource::Solid)
    );
    assert_eq!(
        plan.dependencies
            .definition_invocations
            .get(&definition_id)
            .map(Vec::len),
        Some(2)
    );
    let definition_invalidation = plan.dependencies.affected_by_definition(definition_id);
    assert_eq!(definition_invalidation.timelines.len(), 1);
    assert_eq!(definition_invalidation.ranges.len(), 2);
    let first_invalidation = plan.dependencies.affected_by_instance(first_instance);
    assert_eq!(first_invalidation.timelines.len(), 1);
    assert!(first_invalidation.timelines.contains(&timeline_id));
    assert_eq!(first_invalidation.ranges.len(), 1);
    let first_range = first_invalidation.ranges.iter().next().unwrap();
    assert_eq!(first_range.start, seconds(3));
    assert_eq!(first_range.duration, seconds(4));
    let second_invalidation = plan.dependencies.affected_by_instance(second_instance);
    assert_eq!(second_invalidation.ranges.len(), 1);
    assert_eq!(
        second_invalidation.ranges.iter().next().unwrap().start,
        seconds(13)
    );
}
