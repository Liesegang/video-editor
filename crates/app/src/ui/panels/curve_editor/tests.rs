use super::*;

fn assert_near(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() <= 1.0e-4,
        "{actual} was not near {expected}"
    );
}

fn test_transform(state: CanvasState) -> CurveTransform {
    CurveTransform::new(
        Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(100.0, 50.0)),
        10.0,
        -1.0,
        3.0,
        state,
    )
    .expect("valid curve transform")
}

#[test]
fn curve_projection_and_edit_deltas_use_the_canvas_transform() {
    let transform = test_transform(CanvasState::new(
        egui::vec2(30.0, -10.0),
        egui::vec2(2.0, 4.0),
    ));

    let point = transform.point(2.5, 1.0);
    assert_near(point.x, 90.0);
    assert_near(point.y, 110.0);
    assert_near(transform.delta_time(20.0) as f32, 1.0);
    assert_near(transform.delta_value(20.0) as f32, -0.4);
    assert_near(transform.time_at_screen_x(90.0).unwrap() as f32, 2.5);
}

#[test]
fn curve_content_and_grid_share_pan_zoom_and_origin() {
    let transform = test_transform(CanvasState::new(
        egui::vec2(17.0, 13.0),
        egui::vec2(1.5, 0.75),
    ));
    let content_origin = transform.point(0.0, transform.max);
    let lines = pan_zoom_ui::grid_lines(transform.rect, transform.canvas, transform.grid_config());
    let x_origin = lines
        .iter()
        .find(|line| line.axis == GridAxis::X && line.kind == GridLineKind::Origin)
        .expect("visible x origin");
    let y_origin = lines
        .iter()
        .find(|line| line.axis == GridAxis::Y && line.kind == GridLineKind::Origin)
        .expect("visible y origin");

    assert_near(x_origin.screen_position, content_origin.x);
    assert_near(y_origin.screen_position, content_origin.y);
    assert_near(
        transform.time_at_world_x(x_origin.world_position) as f32,
        0.0,
    );
    assert_near(
        transform.value_at_world_y(y_origin.world_position) as f32,
        transform.max as f32,
    );
}

#[test]
fn curve_navigation_uses_the_shared_independent_axis_policy() {
    let config = curve_navigation_config();

    assert_eq!(config.input_policy, ViewportInputPolicy::AxisModifiers);
    assert_eq!(config.zoom_policy, ZoomPolicy::IndependentXY);
    assert_eq!(config.pan_axes, AxisMask::BOTH);
    assert_eq!(config.zoom_axes, AxisMask::BOTH);
    assert_eq!(config.min_zoom, Vec2::new(0.01, 1.0e-6));
    assert_eq!(config.max_zoom, Vec2::new(20.0, 100_000.0));
    assert_eq!(config.wheel_zoom_sensitivity, 0.2);
}

#[test]
fn minimum_vertical_zoom_can_show_more_than_two_hundred_thousand_values() {
    let transform = test_transform(CanvasState::new(
        Vec2::ZERO,
        Vec2::new(1.0, curve_navigation_config().min_zoom.y),
    ));
    let (minimum, maximum) = transform.visible_value_range().expect("finite value range");

    assert!(maximum - minimum >= 200_000.0);
    assert!(minimum.is_finite() && maximum.is_finite());
}

#[test]
fn extreme_grid_labels_use_compact_finite_notation() {
    assert_eq!(format_curve_value(42.25), "42.25");
    assert_eq!(format_curve_value(12_345.0), "12345");
    assert!(format_curve_value(12_345_678.0).contains('e'));
    assert!(format_curve_value(0.000_01).contains('e'));
}

#[test]
fn fit_state_is_identity_for_the_normalized_curve_world() {
    let transform = test_transform(CanvasState::uniform(Vec2::ZERO, 1.0));

    assert_eq!(
        transform.point(0.0, transform.max),
        transform.rect.left_top()
    );
    assert_eq!(
        transform.point(transform.duration, transform.min),
        transform.rect.right_bottom()
    );
}

#[test]
fn playhead_is_hidden_outside_the_visible_curve_plot() {
    let transform = test_transform(CanvasState::uniform(Vec2::ZERO, 1.0));

    assert_eq!(visible_playhead_x(transform, -0.01), None);
    assert_eq!(visible_playhead_x(transform, 10.01), None);
    assert_eq!(
        visible_playhead_x(transform, 0.0),
        Some(transform.rect.left())
    );
    assert_eq!(
        visible_playhead_x(transform, 10.0),
        Some(transform.rect.right())
    );
    assert_eq!(visible_playhead_x(transform, f64::NAN), None);
}

fn test_series(
    owner: AutomationOwner,
    target: crate::state::authoring::AutomationTarget,
) -> CurveSeries {
    CurveSeries {
        id: AutomationLaneId { owner, target },
        component: crate::state::authoring::CurveValueComponent::Scalar,
        label: "Value".to_string(),
        points: Vec::new(),
    }
}

#[test]
fn new_and_promoted_lanes_are_visible_without_forgetting_explicitly_hidden_lanes() {
    let item_id = library::model::authoring::TimelineItemId::new();
    let owner = AutomationOwner::Item(item_id);
    let position = test_series(
        owner.clone(),
        crate::state::authoring::AutomationTarget::AuthoredProperty {
            owner: library::editor::AuthoringPropertyOwner::Item(item_id),
            key: "position".to_string(),
        },
    );
    let direct_tracking = test_series(
        owner.clone(),
        crate::state::authoring::AutomationTarget::AuthoredProperty {
            owner: library::editor::AuthoringPropertyOwner::TextEnsemble {
                item_id,
                operation_id: uuid::Uuid::new_v4(),
            },
            key: "amount".to_string(),
        },
    );
    let promoted_tracking = test_series(
        owner.clone(),
        crate::state::authoring::AutomationTarget::ModuleParameter(
            library::model::authoring::PublishedParameterId::new(),
        ),
    );
    let mut state = AuthoringUiState::new(library::model::authoring::TimelineId::new());

    sync_visibility(&mut state, &owner, std::slice::from_ref(&position));
    assert!(series_visible(&state, &position));
    state.curve_editor.hidden_lanes.insert(position.id.clone());

    sync_visibility(
        &mut state,
        &owner,
        &[position.clone(), direct_tracking.clone()],
    );
    assert!(!series_visible(&state, &position));
    assert!(series_visible(&state, &direct_tracking));

    state
        .curve_editor
        .hidden_lanes
        .insert(direct_tracking.id.clone());
    sync_visibility(
        &mut state,
        &owner,
        &[position.clone(), promoted_tracking.clone()],
    );
    assert!(!series_visible(&state, &position));
    assert!(series_visible(&state, &promoted_tracking));
    assert!(!state
        .curve_editor
        .hidden_lanes
        .contains(&direct_tracking.id));

    sync_visibility(&mut state, &owner, std::slice::from_ref(&promoted_tracking));
    sync_visibility(&mut state, &owner, &[position.clone(), promoted_tracking]);
    assert!(
        series_visible(&state, &position),
        "a removed and re-authored lane returns with the visible default"
    );
}

#[test]
fn changing_curve_owner_resets_only_owner_local_visibility_and_navigation() {
    let first_item = library::model::authoring::TimelineItemId::new();
    let first_owner = AutomationOwner::Item(first_item);
    let first = test_series(
        first_owner.clone(),
        crate::state::authoring::AutomationTarget::AuthoredProperty {
            owner: library::editor::AuthoringPropertyOwner::Item(first_item),
            key: "opacity".to_string(),
        },
    );
    let second_item = library::model::authoring::TimelineItemId::new();
    let second_owner = AutomationOwner::Item(second_item);
    let second = test_series(
        second_owner.clone(),
        crate::state::authoring::AutomationTarget::AuthoredProperty {
            owner: library::editor::AuthoringPropertyOwner::Item(second_item),
            key: "opacity".to_string(),
        },
    );
    let mut state = AuthoringUiState::new(library::model::authoring::TimelineId::new());
    sync_visibility(&mut state, &first_owner, std::slice::from_ref(&first));
    state.curve_editor.hidden_lanes.insert(first.id);
    state.curve_editor.canvas = CanvasState::new(Vec2::splat(20.0), Vec2::splat(3.0));
    state.curve_editor.value_range = Some((-100.0, 100.0));

    sync_visibility(&mut state, &second_owner, std::slice::from_ref(&second));

    assert!(state.curve_editor.hidden_lanes.is_empty());
    assert_eq!(state.curve_editor.value_range, None);
    assert!(series_visible(&state, &second));
    assert_eq!(
        state.curve_editor.canvas,
        CanvasState::uniform(Vec2::ZERO, 1.0)
    );
}
