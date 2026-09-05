//! Matching Project/RenderPlan pairs for immutable Inspector preview gestures.

use super::*;
use crate::state::authoring::TransientPropertyEdit;
use library::core::render_plan::ModuleHost;
use library::editor::{AuthoringPropertyOwner, AuthoringPropertyValueTarget};
use library::model::authoring::{
    MediaTime, ModuleDefinitionId, ModuleInstanceId, PublishedParameterId, SourceRef,
    TimelineInterval, TimelineItemId,
};
use library::model::frame::color::Color;
use library::model::property::{ColorValue, PropertyValue};
use library::plugin::PluginManager;

struct SolidNodeClipFixture {
    service: TimelineEditorService,
    item_id: TimelineItemId,
    instance_id: ModuleInstanceId,
    definition_id: ModuleDefinitionId,
    color_parameter_id: PublishedParameterId,
}

fn color(r: u8, g: u8, b: u8) -> PropertyValue {
    PropertyValue::ColorValue(ColorValue::from_straight_srgba8(&Color { r, g, b, a: 255 }))
}

fn solid_node_clip_fixture() -> SolidNodeClipFixture {
    let plugins = PluginManager::default();
    let service = TimelineEditorService::create_default("Transient Node Clip plan")
        .expect("authoring service");
    let project = service.snapshot().expect("default Project");
    let timeline_id = project.root_timeline_id;
    let track_id = project.timelines[&timeline_id].track_order[0];
    let (item_id, _) = service
        .add_item(
            track_id,
            "Solid Node Clip".to_string(),
            SourceRef::Solid {
                color: Color::black(),
            },
            TimelineInterval::new(MediaTime::zero(), MediaTime::new(2, 1).expect("duration"))
                .expect("interval"),
            0,
        )
        .expect("Solid item");
    service
        .set_authored_property_constant(
            AuthoringPropertyOwner::Item(item_id),
            "color".to_string(),
            color(12, 24, 36),
        )
        .expect("authored Solid color");
    let conversion = service
        .convert_source_to_node_clip(&plugins, item_id)
        .expect("promote Solid to Node Clip");
    let project = service.snapshot().expect("converted Project");
    let SourceRef::Module(invocation) = &project.items[&item_id].source else {
        panic!("converted Solid must be a Node Clip");
    };
    let instance_id = invocation.instance_id;
    let definition = &project.module_definitions[&conversion.definition_id];
    let color_parameter_id = definition
        .interface
        .parameters
        .iter()
        .find(|parameter| parameter.target.port == "color")
        .map(|parameter| parameter.id)
        .expect("published Solid Color");
    SolidNodeClipFixture {
        service,
        item_id,
        instance_id,
        definition_id: conversion.definition_id,
        color_parameter_id,
    }
}

fn request_key(
    revision: ProjectRevision,
    timeline_id: TimelineId,
    frame_number: i64,
    region: Option<Region>,
    transient_edit: Option<u64>,
) -> PreviewRequestKey {
    PreviewRequestKey {
        revision,
        timeline_id,
        instance_path: None,
        frame_number,
        render_scale: OrderedFloat(1.0),
        region,
        transient_edit,
    }
}

fn invocation(
    plan: &RenderPlan,
    timeline_id: TimelineId,
    item_id: TimelineItemId,
) -> &library::core::render_plan::CompiledModuleInvocation {
    plan.invocation(ModuleHost::TimelineItem {
        timeline_id,
        item_id,
    })
    .expect("compiled Node Clip invocation")
}

fn assert_compiled_topology_shared(
    stable: &RenderPlan,
    transient: &RenderPlan,
    timeline_id: TimelineId,
    definition_id: ModuleDefinitionId,
) {
    assert!(Arc::ptr_eq(
        &stable.module_definitions[&definition_id],
        &transient.module_definitions[&definition_id]
    ));
    assert!(Arc::ptr_eq(
        &stable.timelines[&timeline_id],
        &transient.timelines[&timeline_id]
    ));
}

#[test]
fn constant_module_preview_request_compiles_matching_plan_and_reuses_it_for_navigation() {
    let fixture = solid_node_clip_fixture();
    let mut runtime = AuthoringPreviewRuntime::default();
    let (revision, source, stable_plan) = runtime
        .snapshot_and_plan(&fixture.service)
        .expect("warm stable plan");
    let timeline_id = source.root_timeline_id;
    let transient_color = color(220, 40, 70);
    let edit = TransientPropertyEdit::module_parameter(
        revision,
        fixture.item_id,
        fixture.instance_id,
        fixture.color_parameter_id,
        transient_color.clone(),
        AuthoringPropertyValueTarget::Constant,
    );
    let digest = edit.digest();
    let projected = Arc::new(edit.project(&source).expect("project module Color"));

    runtime
        .request(
            request_key(revision, timeline_id, 0, None, Some(digest)),
            None,
            Arc::clone(&projected),
            Arc::clone(&stable_plan),
        )
        .expect("queue transient request");
    let first = runtime.desired.take().expect("transient desired render");
    assert_ne!(
        invocation(&stable_plan, timeline_id, fixture.item_id).parameter_overrides
            [&fixture.color_parameter_id],
        transient_color,
        "the stable immutable plan must retain its authored value"
    );
    assert_eq!(
        invocation(&first.plan, timeline_id, fixture.item_id).parameter_overrides
            [&fixture.color_parameter_id],
        transient_color,
        "the queued plan must match the projected Project"
    );
    assert_compiled_topology_shared(
        &stable_plan,
        &first.plan,
        timeline_id,
        fixture.definition_id,
    );
    assert_eq!(
        fixture.service.revision().expect("unchanged revision"),
        revision
    );
    assert_eq!(
        fixture
            .service
            .snapshot()
            .expect("unchanged Project")
            .as_ref(),
        source.as_ref()
    );

    runtime
        .request(
            request_key(
                revision,
                timeline_id,
                1,
                Some(Region {
                    x: 4.0,
                    y: 6.0,
                    width: 32.0,
                    height: 18.0,
                }),
                Some(digest),
            ),
            None,
            Arc::clone(&projected),
            Arc::clone(&stable_plan),
        )
        .expect("queue navigation request for same edit");
    let navigated = runtime.desired.take().expect("navigated desired render");
    assert!(Arc::ptr_eq(&first.plan, &navigated.plan));

    runtime
        .request(
            request_key(revision, timeline_id, 2, None, None),
            None,
            Arc::clone(&source),
            Arc::clone(&stable_plan),
        )
        .expect("queue stable request");
    let stable = runtime.desired.take().expect("stable desired render");
    assert!(Arc::ptr_eq(&stable.plan, &stable_plan));
}

#[test]
fn keyframed_module_preview_plan_matches_projection_and_release_commit() {
    let fixture = solid_node_clip_fixture();
    fixture
        .service
        .upsert_module_parameter_keyframe(
            fixture.item_id,
            fixture.color_parameter_id,
            MediaTime::zero(),
            color(12, 24, 36),
            None,
        )
        .expect("initial Color keyframe");
    let mut runtime = AuthoringPreviewRuntime::default();
    let (revision, source, stable_plan) = runtime
        .snapshot_and_plan(&fixture.service)
        .expect("warm keyed plan");
    let timeline_id = source.root_timeline_id;
    let transient_color = color(40, 190, 230);
    let edit = TransientPropertyEdit::module_parameter(
        revision,
        fixture.item_id,
        fixture.instance_id,
        fixture.color_parameter_id,
        transient_color.clone(),
        AuthoringPropertyValueTarget::Keyframe {
            local_time: MediaTime::zero(),
        },
    );
    let projected = Arc::new(edit.project(&source).expect("project Color keyframe"));
    runtime
        .request(
            request_key(revision, timeline_id, 0, None, Some(edit.digest())),
            None,
            Arc::clone(&projected),
            Arc::clone(&stable_plan),
        )
        .expect("queue keyed transient request");
    let transient = runtime.desired.take().expect("keyed desired render");
    assert_ne!(
        invocation(&stable_plan, timeline_id, fixture.item_id).automation_tracks
            [&fixture.color_parameter_id]
            .evaluate_at(MediaTime::zero())
            .expect("stable Color"),
        transient_color
    );
    assert_eq!(
        invocation(&transient.plan, timeline_id, fixture.item_id).automation_tracks
            [&fixture.color_parameter_id]
            .evaluate_at(MediaTime::zero())
            .expect("transient Color"),
        transient_color
    );
    assert_compiled_topology_shared(
        &stable_plan,
        &transient.plan,
        timeline_id,
        fixture.definition_id,
    );
    assert_eq!(
        fixture.service.revision().expect("unchanged revision"),
        revision
    );

    fixture
        .service
        .upsert_module_parameter_keyframe(
            fixture.item_id,
            fixture.color_parameter_id,
            MediaTime::zero(),
            transient_color,
            None,
        )
        .expect("commit Color keyframe");
    assert_eq!(
        fixture
            .service
            .snapshot()
            .expect("committed Project")
            .as_ref(),
        projected.as_ref(),
        "projection and release must produce the same Project"
    );
    let (_, _, committed_plan) = runtime
        .snapshot_and_plan(&fixture.service)
        .expect("compile committed plan");
    assert_eq!(
        invocation(&committed_plan, timeline_id, fixture.item_id).automation_tracks
            [&fixture.color_parameter_id],
        invocation(&transient.plan, timeline_id, fixture.item_id).automation_tracks
            [&fixture.color_parameter_id]
    );
}

#[test]
fn invalid_transient_plan_clears_desired_without_poisoning_stable_snapshot() {
    let fixture = solid_node_clip_fixture();
    let mut runtime = AuthoringPreviewRuntime::default();
    let (revision, source, stable_plan) = runtime
        .snapshot_and_plan(&fixture.service)
        .expect("warm stable plan");
    let timeline_id = source.root_timeline_id;
    let mut invalid = source.as_ref().clone();
    invalid.module_instances.remove(&fixture.instance_id);

    runtime
        .request(
            request_key(revision, timeline_id, 0, None, Some(98)),
            None,
            Arc::clone(&source),
            Arc::clone(&stable_plan),
        )
        .expect("pending valid transient request");
    let prior_intent = runtime
        .desired
        .as_ref()
        .expect("pending render")
        .intent
        .clone();
    assert!(runtime.transient_plan.is_some());

    let error = runtime
        .request(
            request_key(revision, timeline_id, 0, None, Some(99)),
            None,
            Arc::new(invalid),
            Arc::clone(&stable_plan),
        )
        .expect_err("invalid transient Project must not queue");
    assert!(
        error.contains("Invocation has missing Module instance"),
        "unexpected validation error: {error}"
    );
    assert!(runtime.desired.is_none());
    assert!(runtime.transient_plan.is_none());
    assert!(!completion_is_publishable(
        &prior_intent,
        runtime.latest.as_ref(),
        None,
        true
    ));
    assert!(runtime.plan_error.is_none());

    let (stable_revision, stable_project, recovered_plan) = runtime
        .snapshot_and_plan(&fixture.service)
        .expect("stable snapshot remains usable");
    assert_eq!(stable_revision, revision);
    assert!(Arc::ptr_eq(&stable_project, &source));
    assert!(Arc::ptr_eq(&recovered_plan, &stable_plan));
}
