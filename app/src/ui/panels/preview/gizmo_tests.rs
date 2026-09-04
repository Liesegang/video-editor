use super::*;
use library::model::authoring::{RationalRate, SourceRef, TimelineInterval};
use library::model::frame::color::Color;
use library::model::frame::transform::{Position, Scale, Transform};
use library::model::property::Property;
use pan_zoom_ui::CanvasState;

fn gesture(handle: Option<GizmoHandle>) -> PreviewTransformGesture {
    let original_position = property_vec2(0.0, 0.0);
    let original_scale = property_vec2(1.0, 1.0);
    let visual = Transform {
        position: Position { x: 0.0, y: 0.0 },
        scale: Scale { x: 1.0, y: 1.0 },
        anchor: Position { x: 50.0, y: 25.0 },
        ..Transform::default()
    };
    PreviewTransformGesture {
        item_id: TimelineItemId::new(),
        handle,
        pointer_origin: Pos2::ZERO,
        canvas_origin: CanvasTransform::new(Pos2::ZERO, CanvasState::uniform(Vec2::ZERO, 1.0)),
        original_position,
        projected_position: original_position,
        original_scale,
        projected_scale: original_scale,
        original_rotation: 0.0,
        projected_rotation: 0.0,
        original_visual_transform: visual.clone(),
        projected_visual_transform: visual,
        parent_transform: Affine2D::IDENTITY,
        local_bounds: Rect::from_min_size(Pos2::ZERO, egui::vec2(100.0, 50.0)),
        local_time: MediaTime::zero(),
        position_keyframed: false,
        scale_keyframed: false,
        rotation_keyframed: false,
        project_revision: ProjectRevision::initial(),
    }
}

#[test]
fn right_handle_scales_and_keeps_the_opposite_edge_fixed() {
    let mut gesture = gesture(Some(GizmoHandle::Right));
    project_gesture(
        &mut gesture,
        egui::pos2(50.0, 0.0),
        egui::Modifiers::default(),
    )
    .unwrap();

    assert_eq!(gesture.projected_scale, property_vec2(1.5, 1.0));
    assert_eq!(gesture.projected_position, property_vec2(25.0, 0.0));
}

#[test]
fn shift_side_resize_preserves_aspect_ratio() {
    let mut gesture = gesture(Some(GizmoHandle::Right));
    project_gesture(
        &mut gesture,
        egui::pos2(50.0, 0.0),
        egui::Modifiers {
            shift: true,
            ..egui::Modifiers::default()
        },
    )
    .unwrap();

    assert_eq!(gesture.projected_scale, property_vec2(1.5, 1.5));
}

#[test]
fn alt_resize_scales_about_the_visual_centre() {
    let mut gesture = gesture(Some(GizmoHandle::Right));
    project_gesture(
        &mut gesture,
        egui::pos2(25.0, 0.0),
        egui::Modifiers {
            alt: true,
            ..egui::Modifiers::default()
        },
    )
    .unwrap();

    assert_eq!(gesture.projected_scale, property_vec2(1.5, 1.0));
    assert_eq!(gesture.projected_position, property_vec2(0.0, 0.0));
}

#[test]
fn rotation_uses_the_visual_anchor_in_world_space() {
    let mut gesture = gesture(Some(GizmoHandle::Rotation));
    gesture.pointer_origin = egui::pos2(100.0, 0.0);
    project_gesture(
        &mut gesture,
        egui::pos2(0.0, 100.0),
        egui::Modifiers::default(),
    )
    .unwrap();

    assert!((gesture.projected_rotation - 90.0).abs() < 0.001);
}

#[test]
fn translation_is_mapped_through_the_parent_transform() {
    let mut gesture = gesture(None);
    gesture.parent_transform = Affine2D::scale(2.0, 4.0);
    project_gesture(
        &mut gesture,
        egui::pos2(20.0, 20.0),
        egui::Modifiers::default(),
    )
    .unwrap();

    assert_eq!(gesture.projected_position, property_vec2(10.0, 5.0));
}

fn editable_item() -> (TimelineEditorService, AuthoringUiState, TimelineItemId) {
    let project = AuthoringProject::new(
        "Gizmo",
        640,
        360,
        RationalRate::new(30, 1).unwrap(),
        MediaTime::new(10, 1).unwrap(),
    )
    .unwrap();
    let timeline_id = project.root_timeline_id;
    let track_id = project.timelines[&timeline_id].track_order[0];
    let service = TimelineEditorService::new(project).unwrap();
    let (item_id, _) = service
        .add_item(
            track_id,
            "Solid".to_string(),
            SourceRef::Solid {
                color: Color::black(),
            },
            TimelineInterval::new(MediaTime::zero(), MediaTime::new(5, 1).unwrap()).unwrap(),
            0,
        )
        .unwrap();
    service
        .set_authored_property_constant(
            AuthoringPropertyOwner::Item(item_id),
            "position".to_string(),
            PropertyValue::Vec2(property_vec2(0.0, 0.0)),
        )
        .unwrap();
    service
        .set_authored_property_constant(
            AuthoringPropertyOwner::Item(item_id),
            "scale".to_string(),
            PropertyValue::Vec2(property_vec2(1.0, 1.0)),
        )
        .unwrap();
    let mut state = AuthoringUiState::new(timeline_id);
    state.selection.replace(AuthoringSelection::Item(item_id));
    (service, state, item_id)
}

#[test]
fn resize_position_and_scale_commit_as_one_revision() {
    let (service, mut state, item_id) = editable_item();
    let mut resize = gesture(Some(GizmoHandle::Right));
    resize.item_id = item_id;
    resize.project_revision = service.revision().unwrap();
    resize.projected_position = property_vec2(25.0, 0.0);
    resize.projected_scale = property_vec2(1.5, 1.0);
    let before = service.revision().unwrap();

    commit_gesture(&mut state, &service, resize);

    assert_eq!(service.revision().unwrap().get(), before.get() + 1);
    let project = service.snapshot().unwrap();
    let item = &project.items[&item_id];
    assert_eq!(
        item.authored_properties
            .get("position")
            .unwrap()
            .evaluate_at(0.0)
            .unwrap(),
        PropertyValue::Vec2(property_vec2(25.0, 0.0))
    );
    assert_eq!(
        item.authored_properties
            .get("scale")
            .unwrap()
            .evaluate_at(0.0)
            .unwrap(),
        PropertyValue::Vec2(property_vec2(1.5, 1.0))
    );
}

#[test]
fn transform_gesture_renders_from_a_transient_project_without_mutating_source() {
    let (service, mut state, item_id) = editable_item();
    let source = service.snapshot().unwrap();
    let mut resize = gesture(Some(GizmoHandle::Right));
    resize.item_id = item_id;
    resize.project_revision = service.revision().unwrap();
    resize.projected_position = property_vec2(25.0, 0.0);
    resize.projected_scale = property_vec2(1.5, 1.0);
    state.preview.transform_gesture = Some(resize);

    let (projected, digest) = transient_render_project(&source, &state);

    assert!(digest.is_some());
    assert_eq!(
        projected.items[&item_id]
            .authored_properties
            .get("position")
            .unwrap()
            .evaluate_at(0.0)
            .unwrap(),
        PropertyValue::Vec2(property_vec2(25.0, 0.0))
    );
    assert_eq!(
        projected.items[&item_id]
            .authored_properties
            .get("scale")
            .unwrap()
            .evaluate_at(0.0)
            .unwrap(),
        PropertyValue::Vec2(property_vec2(1.5, 1.0))
    );
    assert_eq!(
        source.items[&item_id]
            .authored_properties
            .get("position")
            .unwrap()
            .evaluate_at(0.0)
            .unwrap(),
        PropertyValue::Vec2(property_vec2(0.0, 0.0))
    );
    assert_eq!(service.revision().unwrap(), resize_project_revision(&state));
}

#[test]
fn unchanged_transform_projection_reuses_the_same_project_arc() {
    let (service, mut state, item_id) = editable_item();
    let source = service.snapshot().unwrap();
    let mut resize = gesture(Some(GizmoHandle::Right));
    resize.item_id = item_id;
    resize.project_revision = service.revision().unwrap();
    resize.projected_scale = property_vec2(1.5, 1.0);
    state.preview.transform_gesture = Some(resize);
    let mut runtime = super::super::AuthoringPreviewRuntime::default();

    let revision = service.revision().unwrap();
    let digest = transient_edit_digest(&state);
    let (first, first_edit) = runtime.project_transient_edit(
        super::super::TransientProjectionStage::Transform,
        revision,
        None,
        digest,
        &source,
        |source| transient_render_project(source, &state),
    );
    let (second, second_edit) = runtime.project_transient_edit(
        super::super::TransientProjectionStage::Transform,
        revision,
        None,
        digest,
        &source,
        |source| transient_render_project(source, &state),
    );

    assert_eq!(first_edit, second_edit);
    assert!(Arc::ptr_eq(&first, &second));
    assert!(!Arc::ptr_eq(&source, &first));
}

#[test]
fn resize_preserves_keyframe_ownership_for_both_changed_properties() {
    let (service, mut state, item_id) = editable_item();
    for (key, value) in [
        ("position", property_vec2(0.0, 0.0)),
        ("scale", property_vec2(1.0, 1.0)),
    ] {
        service
            .upsert_authored_property_keyframe(
                AuthoringPropertyOwner::Item(item_id),
                key.to_string(),
                MediaTime::zero(),
                PropertyValue::Vec2(value),
                None,
            )
            .unwrap();
    }
    let mut resize = gesture(Some(GizmoHandle::Right));
    resize.item_id = item_id;
    resize.project_revision = service.revision().unwrap();
    resize.local_time = MediaTime::new(1, 1).unwrap();
    resize.position_keyframed = true;
    resize.scale_keyframed = true;
    resize.projected_position = property_vec2(25.0, 0.0);
    resize.projected_scale = property_vec2(1.5, 1.0);
    let before = service.revision().unwrap();

    commit_gesture(&mut state, &service, resize);

    assert_eq!(service.revision().unwrap().get(), before.get() + 1);
    let project = service.snapshot().unwrap();
    for key in ["position", "scale"] {
        assert_eq!(
            project.items[&item_id]
                .authored_properties
                .get(key)
                .unwrap()
                .evaluator,
            "keyframe"
        );
    }
    assert_eq!(
        project.items[&item_id]
            .authored_properties
            .get("position")
            .unwrap()
            .evaluate_at(1.0)
            .unwrap(),
        PropertyValue::Vec2(property_vec2(25.0, 0.0))
    );
    assert_eq!(
        project.items[&item_id]
            .authored_properties
            .get("scale")
            .unwrap()
            .evaluate_at(1.0)
            .unwrap(),
        PropertyValue::Vec2(property_vec2(1.5, 1.0))
    );
}

fn resize_project_revision(state: &AuthoringUiState) -> ProjectRevision {
    state
        .preview
        .transform_gesture
        .as_ref()
        .unwrap()
        .project_revision
}

#[test]
fn expression_controlled_scale_is_refused() {
    let (_, _, item_id) = editable_item();
    let mut item = TimelineItem {
        id: item_id,
        track_id: library::model::authoring::TimelineTrackId::new(),
        name: "Bound".to_string(),
        source: SourceRef::Solid {
            color: Color::black(),
        },
        interval: TimelineInterval::new(MediaTime::zero(), MediaTime::new(1, 1).unwrap()).unwrap(),
        time_map: library::model::authoring::TimeMap::default(),
        layer: 0,
        parent: None,
        blend_mode: library::model::BlendMode::Normal,
        authored_properties: library::model::property::PropertyMap::new(),
    };
    item.authored_properties.set(
        "scale".to_string(),
        Property::expression(
            "signal".to_string(),
            PropertyValue::Vec2(property_vec2(1.0, 1.0)),
        ),
    );

    let error = authored_vec2(
        &item,
        "scale",
        MediaTime::zero(),
        property_vec2(1.0, 1.0),
        true,
    )
    .unwrap_err();
    assert!(error.contains("would disconnect that control"));
    assert_eq!(
        item.authored_properties.get("scale").unwrap().evaluator,
        "expression"
    );
}

#[test]
fn all_production_handle_ids_are_stable() {
    let names = [
        GizmoHandle::TopLeft,
        GizmoHandle::Top,
        GizmoHandle::TopRight,
        GizmoHandle::Left,
        GizmoHandle::Right,
        GizmoHandle::BottomLeft,
        GizmoHandle::Bottom,
        GizmoHandle::BottomRight,
        GizmoHandle::Rotation,
    ]
    .map(handle_name);
    assert_eq!(
        names,
        [
            "top_left",
            "top",
            "top_right",
            "left",
            "right",
            "bottom_left",
            "bottom",
            "bottom_right",
            "rotation",
        ]
    );
}
