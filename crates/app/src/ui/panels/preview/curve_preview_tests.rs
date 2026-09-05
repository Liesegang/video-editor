//! Curve drag projection through the production Preview runtime.

use std::sync::Arc;

use super::plan_tests::{
    assert_compiled_topology_shared, color, invocation, request_key, solid_node_clip_fixture,
};
use super::*;
use crate::state::authoring::{
    AuthoringUiState, AutomationLaneId, AutomationOwner, AutomationTarget, CurveKeyDrag,
    CurveValueComponent, TransientPropertyEdit,
};
use library::animation::EasingFunction;
use library::editor::{
    AuthoringKeyframeTarget, AuthoringKeyframeUpdate, AuthoringPropertyOwner,
    AuthoringPropertyValueTarget, AuthoringPropertyValueUpdate,
};
use library::model::authoring::{MediaTime, SourceRef, TimelineInterval};
use library::model::frame::color::Color;
use library::model::property::PropertyValue;

fn time(numerator: i64, denominator: u32) -> MediaTime {
    MediaTime::new(numerator, denominator).expect("valid test time")
}

#[test]
fn direct_curve_drag_projects_once_and_cancel_restores_the_source_arc() {
    let service = TimelineEditorService::create_default("Direct Curve preview").expect("service");
    let project = service.snapshot().expect("default Project");
    let track_id = project.timelines[&project.root_timeline_id].track_order[0];
    drop(project);
    let (item_id, _) = service
        .add_item(
            track_id,
            "Animated Solid".to_string(),
            SourceRef::Solid {
                color: Color::white(),
            },
            TimelineInterval::new(MediaTime::zero(), time(3, 1)).expect("interval"),
            0,
        )
        .expect("item");
    service
        .set_authored_property_keyframe_mode(
            AuthoringPropertyOwner::Item(item_id),
            "opacity".to_string(),
            MediaTime::zero(),
            PropertyValue::from(1.0),
        )
        .expect("initial Keyframe");
    let (keyframe_id, _) = service
        .upsert_authored_property_keyframe(
            AuthoringPropertyOwner::Item(item_id),
            "opacity".to_string(),
            time(1, 1),
            PropertyValue::from(0.75),
            Some(EasingFunction::EaseOutQuad),
        )
        .expect("second Keyframe");

    let mut runtime = AuthoringPreviewRuntime::default();
    let (revision, source, _) = runtime.snapshot_and_plan(&service).expect("stable plan");
    let lane = AutomationLaneId {
        owner: AutomationOwner::Item(item_id),
        target: AutomationTarget::AuthoredProperty {
            owner: AuthoringPropertyOwner::Item(item_id),
            key: "opacity".to_string(),
        },
    };
    let mut state = AuthoringUiState::new(source.root_timeline_id);
    state.curve_editor.drag = Some(CurveKeyDrag {
        source_revision: revision,
        lane,
        component: CurveValueComponent::Scalar,
        keyframe_id,
        original_time: time(1, 1),
        original_value: PropertyValue::from(0.75),
        pointer_origin: egui::pos2(20.0, 30.0),
        projected_time: time(3, 2),
        projected_value: PropertyValue::from(0.4),
    });

    let (projected, first_digest) = runtime
        .project_for_preview(&source, revision, &state)
        .expect("Curve projection");
    let keyframe = projected.items[&item_id]
        .authored_properties
        .get("opacity")
        .expect("Opacity")
        .keyframe_by_id(keyframe_id)
        .expect("same Keyframe");
    assert_eq!(keyframe.time.into_inner(), 1.5);
    assert_eq!(keyframe.value, PropertyValue::from(0.4));
    assert_eq!(keyframe.easing, EasingFunction::EaseOutQuad);
    assert_eq!(service.revision().expect("unchanged revision"), revision);
    assert_eq!(service.snapshot().expect("unchanged Project"), source);

    let (reused, second_digest) = runtime
        .project_for_preview(&source, revision, &state)
        .expect("cached Curve projection");
    assert_eq!(second_digest, first_digest);
    assert!(Arc::ptr_eq(&reused, &projected));

    state.curve_editor.drag = None;
    let (cancelled, cancelled_digest) = runtime
        .project_for_preview(&source, revision, &state)
        .expect("cancel Curve projection");
    assert!(cancelled_digest.is_none());
    assert!(Arc::ptr_eq(&cancelled, &source));
}

#[test]
fn module_curve_drag_compiles_projected_automation_without_recompiling_topology() {
    let fixture = solid_node_clip_fixture();
    let (keyframe_id, _) = fixture
        .service
        .upsert_module_parameter_keyframe(
            fixture.item_id,
            fixture.color_parameter_id,
            MediaTime::zero(),
            color(12, 24, 36),
            Some(EasingFunction::EaseInOutQuad),
        )
        .expect("Color Keyframe");
    let mut runtime = AuthoringPreviewRuntime::default();
    let (revision, source, stable_plan) = runtime
        .snapshot_and_plan(&fixture.service)
        .expect("stable keyed plan");
    let timeline_id = source.root_timeline_id;
    let projected_color = color(210, 45, 90);
    let lane = AutomationLaneId {
        owner: AutomationOwner::Item(fixture.item_id),
        target: AutomationTarget::ModuleParameter(fixture.color_parameter_id),
    };
    let mut state = AuthoringUiState::new(source.root_timeline_id);
    state.curve_editor.drag = Some(CurveKeyDrag {
        source_revision: revision,
        lane,
        component: CurveValueComponent::Scalar,
        keyframe_id,
        original_time: MediaTime::zero(),
        original_value: color(12, 24, 36),
        pointer_origin: egui::pos2(20.0, 30.0),
        projected_time: time(1, 2),
        projected_value: projected_color.clone(),
    });

    let (projected, transient_edit) = runtime
        .project_for_preview(&source, revision, &state)
        .expect("Node Clip Curve projection");
    let transient_edit = transient_edit.expect("non-neutral edit digest");
    let SourceRef::Module(projected_invocation) = &projected.items[&fixture.item_id].source else {
        panic!("projected item must remain a Node Clip")
    };
    let projected_track = &projected_invocation.automation_tracks[&fixture.color_parameter_id];
    let projected_key = projected_track
        .keyframes
        .iter()
        .find(|keyframe| keyframe.id == keyframe_id)
        .expect("same compiled Keyframe");
    assert_eq!(projected_key.time, time(1, 2));
    assert_eq!(projected_key.value, projected_color);
    assert_eq!(projected_key.easing, EasingFunction::EaseInOutQuad);

    runtime
        .request(
            request_key(revision, timeline_id, 0, None, Some(transient_edit)),
            None,
            Arc::clone(&projected),
            Arc::clone(&stable_plan),
        )
        .expect("queue matching transient plan");
    let desired = runtime.desired.take().expect("transient render request");
    let desired_track = &invocation(&desired.plan, timeline_id, fixture.item_id).automation_tracks
        [&fixture.color_parameter_id];
    assert_eq!(desired_track, projected_track);
    assert_compiled_topology_shared(
        &stable_plan,
        &desired.plan,
        timeline_id,
        fixture.definition_id,
    );
    assert_eq!(fixture.service.revision().expect("pure revision"), revision);
    assert_eq!(fixture.service.snapshot().expect("pure Project"), source);

    let target = AuthoringKeyframeTarget::ModuleParameter {
        item_id: fixture.item_id,
        parameter_id: fixture.color_parameter_id,
    };
    fixture
        .service
        .update_keyframe(
            &target,
            keyframe_id,
            AuthoringKeyframeUpdate {
                time: Some(time(1, 2)),
                value: Some(projected_color),
                easing: None,
            },
        )
        .expect("release Curve drag");
    assert_eq!(
        fixture.service.snapshot().expect("committed").as_ref(),
        projected.as_ref()
    );
    fixture.service.undo().expect("Undo").expect("one command");
    assert_eq!(fixture.service.snapshot().expect("Undo Project"), source);
}

#[test]
fn stale_curve_drag_falls_through_to_current_inspector_projection() {
    let fixture = solid_node_clip_fixture();
    let (keyframe_id, _) = fixture
        .service
        .upsert_module_parameter_keyframe(
            fixture.item_id,
            fixture.color_parameter_id,
            MediaTime::zero(),
            color(12, 24, 36),
            None,
        )
        .expect("Color Keyframe");
    fixture
        .service
        .set_authored_property_constant(
            AuthoringPropertyOwner::Item(fixture.item_id),
            "opacity".to_string(),
            PropertyValue::from(1.0),
        )
        .expect("Opacity");
    let mut runtime = AuthoringPreviewRuntime::default();
    let (revision, source, _) = runtime
        .snapshot_and_plan(&fixture.service)
        .expect("stable keyed plan");
    let mut state = AuthoringUiState::new(source.root_timeline_id);
    state.curve_editor.drag = Some(CurveKeyDrag {
        source_revision: library::model::authoring::ProjectRevision::initial(),
        lane: AutomationLaneId {
            owner: AutomationOwner::Item(fixture.item_id),
            target: AutomationTarget::ModuleParameter(fixture.color_parameter_id),
        },
        component: CurveValueComponent::Scalar,
        keyframe_id,
        original_time: MediaTime::zero(),
        original_value: color(12, 24, 36),
        pointer_origin: egui::pos2(20.0, 30.0),
        projected_time: time(1, 2),
        projected_value: color(200, 40, 80),
    });
    state.inspector.transient_property_edit = Some(TransientPropertyEdit::authored(
        revision,
        AuthoringPropertyOwner::Item(fixture.item_id),
        AuthoringPropertyValueUpdate {
            key: "opacity".to_string(),
            value: PropertyValue::from(0.35),
            target: AuthoringPropertyValueTarget::Constant,
        },
    ));
    assert_ne!(
        revision,
        library::model::authoring::ProjectRevision::initial()
    );
    let (projected, edit) = runtime
        .project_for_preview(&source, revision, &state)
        .expect("project current Inspector edit");
    assert!(edit.is_some());
    assert_eq!(
        projected.items[&fixture.item_id]
            .authored_properties
            .get("opacity")
            .and_then(|property| property.get_static_value()),
        Some(&PropertyValue::from(0.35))
    );
    assert_eq!(
        fixture.service.snapshot().expect("unchanged Project"),
        source
    );
    assert_eq!(
        fixture.service.revision().expect("unchanged revision"),
        revision
    );
}

#[test]
fn missing_curve_keyframe_projection_errors_without_mutating_the_source() {
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
        .expect("Color Keyframe");
    let mut runtime = AuthoringPreviewRuntime::default();
    let (revision, source, _) = runtime
        .snapshot_and_plan(&fixture.service)
        .expect("stable keyed plan");
    let mut state = AuthoringUiState::new(source.root_timeline_id);
    state.curve_editor.drag = Some(CurveKeyDrag {
        source_revision: revision,
        lane: AutomationLaneId {
            owner: AutomationOwner::Item(fixture.item_id),
            target: AutomationTarget::ModuleParameter(fixture.color_parameter_id),
        },
        component: CurveValueComponent::Scalar,
        keyframe_id: library::model::property::KeyframeId::new(),
        original_time: MediaTime::zero(),
        original_value: color(12, 24, 36),
        pointer_origin: egui::pos2(20.0, 30.0),
        projected_time: time(1, 2),
        projected_value: color(200, 40, 80),
    });

    let error = runtime
        .project_for_preview(&source, revision, &state)
        .expect_err("missing Keyframe must reject projection");
    assert!(error.contains("Missing Automation Keyframe"), "{error}");
    assert_eq!(
        fixture.service.snapshot().expect("unchanged Project"),
        source
    );
    assert_eq!(
        fixture.service.revision().expect("unchanged revision"),
        revision
    );
}
