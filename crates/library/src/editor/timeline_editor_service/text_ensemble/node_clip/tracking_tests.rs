//! Production-path preservation of Timeline-owned Tracking automation when a
//! direct Text item is explicitly converted to a Node Clip.

use std::sync::Arc;

use super::*;
use crate::animation::EasingFunction;
use crate::core::ensemble::{EffectorTarget, types::EffectorConfig};
use crate::core::render_plan::{RenderPlanCompiler, evaluate_render_plan_frame};
use crate::editor::AppearanceOperationFactory;
use crate::editor::timeline_editor_service::node_clip_conversion_tests::{
    rendered_pixels, small_service, time,
};
use crate::model::authoring::{
    AuthoringProject, ModuleDefinitionSharing, ProjectDocument, SourceRef, TimelineInterval,
};
use crate::model::frame::color::Color;
use crate::model::frame::entity::{FrameBounds, FrameContent, FrameItem};
use crate::model::project::property::KeyframeId;
use crate::rendering::renderer::Affine2D;

struct TrackingFixture {
    service: TimelineEditorService,
    plugins: Arc<PluginManager>,
    item_id: TimelineItemId,
    operation_id: uuid::Uuid,
    keyframe_ids: [KeyframeId; 2],
}

fn tracking_fixture() -> TrackingFixture {
    let plugins = Arc::new(PluginManager::default());
    let (service, track_id) = small_service("Tracking conversion");
    let fill = AppearanceOperationFactory::create(plugins.as_ref(), "fill").expect("Fill style");
    let (item_id, _) = service
        .add_item(
            track_id,
            "Tracked title".to_string(),
            SourceRef::Text {
                text: "ABCD".to_string(),
                appearance_operations: vec![fill],
                ensemble_operations: Vec::new(),
            },
            TimelineInterval::new(time(1), time(3)).expect("offset Text interval"),
            0,
        )
        .expect("add Text item");
    let (operation_id, _) = service
        .add_text_ensemble_operation_by_id(
            plugins.as_ref(),
            item_id,
            TextEnsembleOperationKind::Effector,
            "tracking",
        )
        .expect("add direct Tracking");
    let owner = AuthoringPropertyOwner::TextEnsemble {
        item_id,
        operation_id,
    };
    service
        .set_text_ensemble_property(
            plugins.as_ref(),
            item_id,
            operation_id,
            "target",
            MediaTime::zero(),
            PropertyValue::String("Block".to_string()),
        )
        .expect("non-default Tracking target");
    let (start_id, _) = service
        .set_authored_property_keyframe_mode(
            owner,
            "amount".to_string(),
            MediaTime::zero(),
            PropertyValue::from(0.0),
        )
        .expect("start Tracking keyframe");
    let (end_id, _) = service
        .upsert_authored_property_keyframe(
            owner,
            "amount".to_string(),
            time(1),
            PropertyValue::from(30.0),
            Some(EasingFunction::Linear),
        )
        .expect("end Tracking keyframe");
    TrackingFixture {
        service,
        plugins,
        item_id,
        operation_id,
        keyframe_ids: [start_id, end_id],
    }
}

fn tracking_amount(
    project: &AuthoringProject,
    plugins: &PluginManager,
    frame_number: u64,
) -> (f32, EffectorTarget) {
    let plan = RenderPlanCompiler::compile(project).expect("Tracking RenderPlan");
    let frame = evaluate_render_plan_frame(project, &plan, plugins, frame_number, 1.0, None)
        .expect("evaluate Tracking frame");
    let ensemble = first_ensemble(&frame.items).expect("Text Tracking must reach Frame Shape");
    let [EffectorConfig::Tracking { amount, target }] = ensemble.effector_configs.as_slice() else {
        panic!("Frame Shape must contain exactly one Tracking effector");
    };
    (*amount, *target)
}

fn first_ensemble(items: &[FrameItem]) -> Option<&crate::core::ensemble::EnsembleData> {
    for item in items {
        match item {
            FrameItem::Object(object) => match &object.content {
                FrameContent::Text {
                    ensemble: Some(ensemble),
                    ..
                }
                | FrameContent::Shape {
                    ensemble: Some(ensemble),
                    ..
                } => return Some(ensemble),
                _ => {}
            },
            FrameItem::Group(group) => {
                if let Some(ensemble) = first_ensemble(&group.items) {
                    return Some(ensemble);
                }
            }
            FrameItem::Transition(transition) => {
                if let Some(ensemble) = first_ensemble(std::slice::from_ref(&transition.from.item))
                    .or_else(|| first_ensemble(std::slice::from_ref(&transition.to.item)))
                {
                    return Some(ensemble);
                }
            }
        }
    }
    None
}

fn first_text_bounds(items: &[FrameItem]) -> Option<FrameBounds> {
    for item in items {
        match item {
            FrameItem::Object(object) => {
                if matches!(&object.content, FrameContent::Text { .. }) {
                    return object.content_bounds;
                }
            }
            FrameItem::Group(group) => {
                if let Some(bounds) = first_text_bounds(&group.items) {
                    return Some(bounds);
                }
            }
            FrameItem::Transition(transition) => {
                if let Some(bounds) = first_text_bounds(std::slice::from_ref(&transition.from.item))
                    .or_else(|| first_text_bounds(std::slice::from_ref(&transition.to.item)))
                {
                    return Some(bounds);
                }
            }
        }
    }
    None
}

fn text_bounds(
    project: &AuthoringProject,
    plugins: &PluginManager,
    frame_number: u64,
) -> FrameBounds {
    let plan = RenderPlanCompiler::compile(project).expect("Text bounds RenderPlan");
    let frame = evaluate_render_plan_frame(project, &plan, plugins, frame_number, 1.0, None)
        .expect("evaluate Text bounds frame");
    first_text_bounds(&frame.items).expect("Text FrameObject bounds")
}

fn first_text_canvas_bounds(items: &[FrameItem], parent: Affine2D) -> Option<(f64, f64, f64, f64)> {
    for item in items {
        match item {
            FrameItem::Object(object) => {
                if !matches!(&object.content, FrameContent::Text { .. }) {
                    continue;
                }
                let bounds = object.content_bounds?;
                let (x, y, width, height) = bounds.as_tuple();
                let transform = parent.compose(Affine2D::from(object.content.transform()));
                let points = [
                    transform.map_point(f64::from(x), f64::from(y)),
                    transform.map_point(f64::from(x + width), f64::from(y)),
                    transform.map_point(f64::from(x + width), f64::from(y + height)),
                    transform.map_point(f64::from(x), f64::from(y + height)),
                ];
                let left = points.iter().map(|point| point.0).reduce(f64::min)?;
                let top = points.iter().map(|point| point.1).reduce(f64::min)?;
                let right = points.iter().map(|point| point.0).reduce(f64::max)?;
                let bottom = points.iter().map(|point| point.1).reduce(f64::max)?;
                return Some((left, top, right, bottom));
            }
            FrameItem::Group(group) => {
                let transform = parent.compose(Affine2D::from(&group.transform));
                if let Some(bounds) = first_text_canvas_bounds(&group.items, transform) {
                    return Some(bounds);
                }
            }
            FrameItem::Transition(transition) => {
                if let Some(bounds) =
                    first_text_canvas_bounds(std::slice::from_ref(&transition.from.item), parent)
                        .or_else(|| {
                            first_text_canvas_bounds(
                                std::slice::from_ref(&transition.to.item),
                                parent,
                            )
                        })
                {
                    return Some(bounds);
                }
            }
        }
    }
    None
}

fn assert_visible_text_pixels_are_inside_bounds(
    project: &AuthoringProject,
    plugins: Arc<PluginManager>,
    frame_number: u64,
) {
    let mut transparent = project.clone();
    let width = {
        let timeline = transparent
            .timelines
            .get_mut(&transparent.root_timeline_id)
            .expect("root Timeline");
        timeline.background_color = Color {
            r: 0,
            g: 0,
            b: 0,
            a: 0,
        };
        usize::try_from(timeline.width).expect("canvas width")
    };
    let plan = RenderPlanCompiler::compile(&transparent).expect("transparent RenderPlan");
    let frame = evaluate_render_plan_frame(
        &transparent,
        &plan,
        plugins.as_ref(),
        frame_number,
        1.0,
        None,
    )
    .expect("transparent Text frame");
    let (left, top, right, bottom) =
        first_text_canvas_bounds(&frame.items, Affine2D::IDENTITY).expect("canvas Text bounds");
    let pixels = rendered_pixels(&transparent, plugins, frame_number);
    let mut visible_pixels = 0;
    for (index, pixel) in pixels.chunks_exact(4).enumerate() {
        if pixel[3] == 0 {
            continue;
        }
        visible_pixels += 1;
        let x = (index % width) as f64;
        let y = (index / width) as f64;
        assert!(
            x + 1.0 >= left && x <= right && y + 1.0 >= top && y <= bottom,
            "visible pixel ({x}, {y}) escaped evaluated canvas bounds ({left}, {top})..({right}, {bottom})"
        );
    }
    assert!(visible_pixels > 0, "Text fixture must paint visible pixels");
}

fn rendered_series(project: &AuthoringProject, plugins: Arc<PluginManager>) -> [Vec<u8>; 3] {
    [30, 45, 60].map(|frame| rendered_pixels(project, Arc::clone(&plugins), frame))
}

fn tracking_parameter(
    project: &AuthoringProject,
    item_id: TimelineItemId,
    operation_id: uuid::Uuid,
    property: &str,
) -> PublishedParameterId {
    let SourceRef::Module(invocation) = &project.items[&item_id].source else {
        panic!("converted Tracking source must be a Node Clip");
    };
    let instance = &project.module_instances[&invocation.instance_id];
    project.module_definitions[&instance.definition_id]
        .interface
        .parameters
        .iter()
        .find(|parameter| {
            parameter.target.node_id == operation_id
                && parameter.target.port == format!("{PROPERTY_PORT_PREFIX}{property}")
        })
        .map(|parameter| parameter.id)
        .unwrap_or_else(|| panic!("published Tracking {property}"))
}

fn invocation(
    project: &AuthoringProject,
    item_id: TimelineItemId,
) -> &crate::model::authoring::ModuleInvocation {
    let SourceRef::Module(invocation) = &project.items[&item_id].source else {
        panic!("Timeline item must be a Node Clip");
    };
    invocation
}

#[test]
fn keyframed_tracking_survives_node_clip_conversion_local_time_pixels_and_persistence() {
    let TrackingFixture {
        service,
        plugins,
        item_id,
        operation_id,
        keyframe_ids,
    } = tracking_fixture();
    let before = service.snapshot().expect("direct Text snapshot");
    let before_pixels = rendered_series(&before, Arc::clone(&plugins));
    let before_bounds = [30, 45, 60].map(|frame| text_bounds(&before, plugins.as_ref(), frame));
    assert_eq!(
        [30, 45, 60].map(|frame| tracking_amount(&before, plugins.as_ref(), frame).0),
        [0.0, 15.0, 30.0],
        "offset placement must evaluate Tracking in clip-local time"
    );
    assert_eq!(
        [30, 45, 60].map(|frame| tracking_amount(&before, plugins.as_ref(), frame).1),
        [EffectorTarget::Block; 3],
        "non-default Tracking target must reach production evaluation"
    );
    assert!(before_pixels[0] != before_pixels[1]);
    assert!(before_pixels[1] != before_pixels[2]);
    let right_edges = before_bounds.map(|bounds| {
        let (x, _, width, _) = bounds.as_tuple();
        x + width
    });
    assert!(
        right_edges[0] < right_edges[1] && right_edges[1] < right_edges[2],
        "keyframed Tracking must expand the evaluated Text bounds: {right_edges:?}"
    );
    for frame in [30, 45, 60] {
        assert_visible_text_pixels_are_inside_bounds(&before, Arc::clone(&plugins), frame);
    }

    let conversion = service
        .convert_source_to_node_clip(plugins.as_ref(), item_id)
        .expect("explicit Node Clip conversion");
    let after = service.snapshot().expect("converted snapshot");
    assert_eq!(
        after.module_definitions[&conversion.definition_id].sharing,
        ModuleDefinitionSharing::Private
    );
    let parameter_id = tracking_parameter(&after, item_id, operation_id, "amount");
    let target_parameter_id = tracking_parameter(&after, item_id, operation_id, "target");
    assert_eq!(
        after.module_instances[&conversion.instance_id].parameter_overrides[&target_parameter_id],
        PropertyValue::String("Block".to_string()),
        "constant non-default target must remain an instance value"
    );
    let track = &invocation(&after, item_id).automation_tracks[&parameter_id];
    assert_eq!(
        track
            .keyframes
            .iter()
            .map(|keyframe| keyframe.id)
            .collect::<Vec<_>>(),
        keyframe_ids.to_vec()
    );
    assert_eq!(
        track
            .keyframes
            .iter()
            .map(|keyframe| keyframe.time)
            .collect::<Vec<_>>(),
        vec![MediaTime::zero(), time(1)]
    );
    assert_eq!(
        track
            .keyframes
            .iter()
            .map(|keyframe| keyframe.easing.clone())
            .collect::<Vec<_>>(),
        vec![EasingFunction::Linear, EasingFunction::Linear]
    );
    assert_eq!(
        [30, 45, 60].map(|frame| tracking_amount(&after, plugins.as_ref(), frame)),
        [
            (0.0, EffectorTarget::Block),
            (15.0, EffectorTarget::Block),
            (30.0, EffectorTarget::Block),
        ]
    );
    assert_eq!(
        rendered_series(&after, Arc::clone(&plugins)),
        before_pixels,
        "conversion changed Tracking Preview pixels"
    );
    assert_eq!(
        [30, 45, 60].map(|frame| text_bounds(&after, plugins.as_ref(), frame)),
        before_bounds,
        "conversion changed the evaluated Text bounds used by Preview Gizmos"
    );
    for frame in [30, 45, 60] {
        assert_visible_text_pixels_are_inside_bounds(&after, Arc::clone(&plugins), frame);
    }

    let encoded = ProjectDocument::new(after.as_ref().clone())
        .to_json()
        .expect("save converted Tracking");
    let loaded = ProjectDocument::from_json(&encoded)
        .expect("load converted Tracking")
        .project;
    assert_eq!(&loaded, after.as_ref());
    assert_eq!(
        rendered_series(&loaded, Arc::clone(&plugins)),
        before_pixels,
        "save/load changed converted Tracking pixels"
    );

    service.undo().expect("undo conversion").expect("one undo");
    assert_eq!(
        service.snapshot().expect("undo snapshot").as_ref(),
        before.as_ref()
    );
    service.redo().expect("redo conversion").expect("one redo");
    assert_eq!(
        service.snapshot().expect("redo snapshot").as_ref(),
        after.as_ref()
    );
}

#[test]
fn direct_text_without_ensemble_uses_the_same_styled_bounds_after_conversion() {
    let plugins = Arc::new(PluginManager::default());
    let (service, track_id) = small_service("Plain Text bounds conversion");
    let fill = AppearanceOperationFactory::create(plugins.as_ref(), "fill").expect("Fill style");
    let stroke =
        AppearanceOperationFactory::create(plugins.as_ref(), "stroke").expect("Stroke style");
    let (item_id, _) = service
        .add_item(
            track_id,
            "Plain styled title".to_string(),
            SourceRef::Text {
                text: "Plain styled title".to_string(),
                appearance_operations: vec![fill, stroke],
                ensemble_operations: Vec::new(),
            },
            TimelineInterval::new(MediaTime::zero(), time(2)).expect("Text interval"),
            0,
        )
        .expect("add plain Text item");
    let before = service.snapshot().expect("direct Text snapshot");
    let direct_bounds = text_bounds(&before, plugins.as_ref(), 0);
    let (x, y, _, _) = direct_bounds.as_tuple();
    assert!(
        x < 0.0 && y < 0.0,
        "Stroke outset must be represented in direct Text bounds: {direct_bounds:?}"
    );
    assert_visible_text_pixels_are_inside_bounds(&before, Arc::clone(&plugins), 0);

    service
        .convert_source_to_node_clip(plugins.as_ref(), item_id)
        .expect("convert plain styled Text");
    let converted = service.snapshot().expect("converted Text snapshot");
    assert_eq!(
        text_bounds(&converted, plugins.as_ref(), 0),
        direct_bounds,
        "plain styled Text bounds changed across explicit Node Clip conversion"
    );
    assert_visible_text_pixels_are_inside_bounds(&converted, Arc::clone(&plugins), 0);
}

#[test]
fn converted_tracking_automation_is_instance_local_for_duplicated_siblings() {
    let fixture = tracking_fixture();
    fixture
        .service
        .convert_source_to_node_clip(fixture.plugins.as_ref(), fixture.item_id)
        .expect("convert tracked Text");
    let (sibling_id, _) = fixture
        .service
        .duplicate_item(fixture.item_id, time(1), 1)
        .expect("duplicate tracked Node Clip");
    let before_edit = fixture.service.snapshot().expect("duplicate snapshot");
    let parameter_id = tracking_parameter(
        &before_edit,
        fixture.item_id,
        fixture.operation_id,
        "amount",
    );
    let original_invocation = invocation(&before_edit, fixture.item_id);
    let sibling_invocation = invocation(&before_edit, sibling_id);
    assert_ne!(
        original_invocation.instance_id,
        sibling_invocation.instance_id
    );
    assert_eq!(
        before_edit.module_instances[&original_invocation.instance_id].definition_id,
        before_edit.module_instances[&sibling_invocation.instance_id].definition_id
    );
    assert_eq!(
        original_invocation.automation_tracks[&parameter_id],
        sibling_invocation.automation_tracks[&parameter_id]
    );
    let definition_before = before_edit.module_definitions
        [&before_edit.module_instances[&original_invocation.instance_id].definition_id]
        .clone();

    fixture
        .service
        .upsert_module_parameter_keyframe(
            fixture.item_id,
            parameter_id,
            time(1),
            PropertyValue::from(48.0),
            Some(EasingFunction::Linear),
        )
        .expect("edit original Tracking automation");
    let after_edit = fixture.service.snapshot().expect("edited snapshot");
    let original = &invocation(&after_edit, fixture.item_id).automation_tracks[&parameter_id];
    let sibling = &invocation(&after_edit, sibling_id).automation_tracks[&parameter_id];
    assert_ne!(original, sibling);
    assert_eq!(
        original.evaluate_at(time(1)).expect("original amount"),
        PropertyValue::from(48.0)
    );
    assert_eq!(
        sibling.evaluate_at(time(1)).expect("sibling amount"),
        PropertyValue::from(30.0)
    );
    assert_eq!(
        after_edit.module_definitions[&definition_before.id], definition_before,
        "Timeline automation edit must not copy or mutate Module topology"
    );

    fixture
        .service
        .undo()
        .expect("undo automation edit")
        .expect("one automation undo");
    assert_eq!(
        fixture.service.snapshot().expect("undo snapshot").as_ref(),
        before_edit.as_ref()
    );
}

#[test]
fn tracking_keyframe_projection_matches_commit_without_mutating_service_or_existing_keys() {
    let fixture = tracking_fixture();
    fixture
        .service
        .convert_source_to_node_clip(fixture.plugins.as_ref(), fixture.item_id)
        .expect("convert tracked Text");
    let before = fixture.service.snapshot().expect("converted snapshot");
    let revision = fixture.service.revision().expect("service revision");
    let instance_id = invocation(&before, fixture.item_id).instance_id;
    let parameter_id = tracking_parameter(&before, fixture.item_id, fixture.operation_id, "amount");
    let keyframes_before = invocation(&before, fixture.item_id).automation_tracks[&parameter_id]
        .keyframes
        .clone();

    let projected = TimelineEditorService::project_module_parameter_value(
        &before,
        fixture.item_id,
        instance_id,
        parameter_id,
        PropertyValue::from(42.0),
        AuthoringPropertyValueTarget::Keyframe {
            local_time: time(1),
        },
    )
    .expect("project Tracking keyframe drag");
    assert_eq!(
        fixture.service.revision().expect("unchanged revision"),
        revision
    );
    assert_eq!(
        fixture
            .service
            .snapshot()
            .expect("unchanged project")
            .as_ref(),
        before.as_ref()
    );
    let projected_keys =
        &invocation(&projected, fixture.item_id).automation_tracks[&parameter_id].keyframes;
    assert_eq!(
        projected_keys
            .iter()
            .map(|keyframe| (keyframe.id, keyframe.time, keyframe.easing.clone()))
            .collect::<Vec<_>>(),
        keyframes_before
            .iter()
            .map(|keyframe| (keyframe.id, keyframe.time, keyframe.easing.clone()))
            .collect::<Vec<_>>(),
        "projection must preserve authored key identities, times, and interpolation"
    );
    assert_eq!(
        tracking_amount(&projected, fixture.plugins.as_ref(), 60),
        (42.0, EffectorTarget::Block)
    );
    assert_ne!(
        rendered_pixels(&projected, Arc::clone(&fixture.plugins), 60),
        rendered_pixels(&before, Arc::clone(&fixture.plugins), 60),
        "projected Tracking value must affect production Preview pixels"
    );
    assert!(
        TimelineEditorService::project_module_parameter_value(
            &before,
            fixture.item_id,
            ModuleInstanceId::new(),
            parameter_id,
            PropertyValue::from(42.0),
            AuthoringPropertyValueTarget::Keyframe {
                local_time: time(1),
            },
        )
        .unwrap_err()
        .to_string()
        .contains("changed Module instance")
    );

    fixture
        .service
        .upsert_module_parameter_keyframe(
            fixture.item_id,
            parameter_id,
            time(1),
            PropertyValue::from(42.0),
            None,
        )
        .expect("commit Tracking keyframe drag");
    assert_eq!(
        fixture
            .service
            .snapshot()
            .expect("committed snapshot")
            .as_ref(),
        &projected,
        "gesture projection and release command must produce the same Project"
    );
}

#[test]
fn tracking_constant_projection_is_instance_local_and_rejects_invalid_targets() {
    let fixture = tracking_fixture();
    fixture
        .service
        .convert_source_to_node_clip(fixture.plugins.as_ref(), fixture.item_id)
        .expect("convert tracked Text");
    let before = fixture.service.snapshot().expect("converted snapshot");
    let revision = fixture.service.revision().expect("service revision");
    let instance_id = invocation(&before, fixture.item_id).instance_id;
    let amount_id = tracking_parameter(&before, fixture.item_id, fixture.operation_id, "amount");
    let target_id = tracking_parameter(&before, fixture.item_id, fixture.operation_id, "target");
    let projected = TimelineEditorService::project_module_parameter_value(
        &before,
        fixture.item_id,
        instance_id,
        target_id,
        PropertyValue::String("Line".to_string()),
        AuthoringPropertyValueTarget::Constant,
    )
    .expect("project constant Tracking target");
    assert_eq!(
        projected.module_instances[&instance_id].parameter_overrides[&target_id],
        PropertyValue::String("Line".to_string())
    );
    assert_eq!(
        before.module_instances[&instance_id].parameter_overrides[&target_id],
        PropertyValue::String("Block".to_string())
    );
    assert_eq!(
        fixture.service.revision().expect("unchanged revision"),
        revision
    );
    assert_eq!(
        fixture
            .service
            .snapshot()
            .expect("unchanged project")
            .as_ref(),
        before.as_ref()
    );
    assert!(
        TimelineEditorService::project_module_parameter_value(
            &before,
            fixture.item_id,
            instance_id,
            amount_id,
            PropertyValue::from(42.0),
            AuthoringPropertyValueTarget::Constant,
        )
        .unwrap_err()
        .to_string()
        .contains("controlled by Timeline automation")
    );
    assert!(
        TimelineEditorService::project_module_parameter_value(
            &before,
            fixture.item_id,
            instance_id,
            PublishedParameterId::new(),
            PropertyValue::from(42.0),
            AuthoringPropertyValueTarget::Constant,
        )
        .is_err()
    );

    fixture
        .service
        .set_module_parameter(
            instance_id,
            target_id,
            PropertyValue::String("Line".to_string()),
        )
        .expect("commit constant Tracking target");
    assert_eq!(
        fixture
            .service
            .snapshot()
            .expect("committed snapshot")
            .as_ref(),
        &projected,
        "constant projection and release command must produce the same Project"
    );
}
