use std::collections::HashMap;

use library::editor::{ModuleItemPlacement, TimelineEditorService};
use library::model::authoring::{
    InstancePath, MediaTime, ModuleDefinition, ModuleDefinitionSharing, ModuleInstanceId,
    ModuleTemplateOrigin, TimelineId, TimelineInterval, TimelineItemId,
};

use super::parameter::node_clip_parameter_item;
use crate::state::node_editor::ModuleEditorHost;

struct Fixture {
    project: std::sync::Arc<library::model::authoring::AuthoringProject>,
    root_timeline_id: TimelineId,
    other_timeline_id: TimelineId,
    item_id: TimelineItemId,
    instance_id: ModuleInstanceId,
}

fn fixture() -> Fixture {
    let service =
        TimelineEditorService::create_default("Node parameter scope").expect("authoring service");
    let initial = service.snapshot().expect("initial Project");
    let root_timeline_id = initial.root_timeline_id;
    let root_timeline = &initial.timelines[&root_timeline_id];
    let track_id = root_timeline.track_order[0];
    let frame_rate = root_timeline.fps;
    let duration = root_timeline.duration;
    drop(initial);

    let (definition, output_id) = ModuleDefinition::new_image(
        "Node Clip",
        ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
    );
    let definition_id = definition.id;
    service
        .add_module_definition(definition)
        .expect("Module definition");
    let (item_id, instance_id, _) = service
        .place_module_item(
            definition_id,
            ModuleItemPlacement {
                track_id,
                name: "Node Clip".to_string(),
                output_id,
                interval: TimelineInterval::new(
                    MediaTime::zero(),
                    MediaTime::from_whole_seconds(2),
                )
                .expect("fixture interval"),
                layer: 0,
                parameter_overrides: HashMap::new(),
                input_bindings: HashMap::new(),
            },
        )
        .expect("Node Clip placement");
    let (other_timeline_id, _, _) = service
        .add_timeline("Other".to_string(), 320, 180, frame_rate, duration)
        .expect("other Timeline");

    Fixture {
        project: service.snapshot().expect("fixture Project"),
        root_timeline_id,
        other_timeline_id,
        item_id,
        instance_id,
    }
}

fn host(
    fixture: &Fixture,
    instance_path: Option<InstancePath>,
    instance_id: ModuleInstanceId,
) -> ModuleEditorHost {
    ModuleEditorHost::NodeClip {
        timeline_item_id: fixture.item_id,
        instance_path,
        module_instance_id: instance_id,
    }
}

#[test]
fn root_path_and_absent_path_are_the_same_parameter_scope() {
    let fixture = fixture();
    let root_path = InstancePath::root(fixture.root_timeline_id);

    assert_eq!(
        node_clip_parameter_item(
            &fixture.project,
            fixture.root_timeline_id,
            Some(&root_path),
            &host(&fixture, None, fixture.instance_id),
        ),
        Ok(fixture.item_id)
    );
    assert_eq!(
        node_clip_parameter_item(
            &fixture.project,
            fixture.root_timeline_id,
            None,
            &host(&fixture, Some(root_path), fixture.instance_id),
        ),
        Ok(fixture.item_id)
    );
}

#[test]
fn a_different_active_timeline_cannot_edit_the_retained_document() {
    let fixture = fixture();

    let error = node_clip_parameter_item(
        &fixture.project,
        fixture.other_timeline_id,
        None,
        &host(&fixture, None, fixture.instance_id),
    )
    .expect_err("another Timeline must not own this Node Clip's keys");

    assert!(error.contains("Open the Node Clip's Timeline"));
}

#[test]
fn a_different_nested_placement_path_cannot_edit_the_document() {
    let fixture = fixture();
    let host_path = InstancePath::root(fixture.root_timeline_id).nested(TimelineItemId::new());
    let active_path = InstancePath::root(fixture.root_timeline_id).nested(TimelineItemId::new());

    let error = node_clip_parameter_item(
        &fixture.project,
        fixture.root_timeline_id,
        Some(&active_path),
        &host(&fixture, Some(host_path), fixture.instance_id),
    )
    .expect_err("another nested placement must not own this Node Clip's keys");

    assert!(error.contains("Composition placement"));
}

#[test]
fn a_replaced_module_instance_invalidates_the_retained_document() {
    let fixture = fixture();

    let error = node_clip_parameter_item(
        &fixture.project,
        fixture.root_timeline_id,
        None,
        &host(&fixture, None, ModuleInstanceId::new()),
    )
    .expect_err("a stale Module instance must not receive parameter edits");

    assert!(error.contains("Module instance has changed"));
}
