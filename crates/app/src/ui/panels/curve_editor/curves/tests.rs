use super::*;

use egui::{Pos2, Rect, Vec2};
use library::editor::AuthoringPropertyOwner;
use library::model::authoring::{MediaTime, TimelineId, TimelineItemId};
use library::model::property::{KeyframeId, PropertyValue, Vec2 as PropertyVec2};
use ordered_float::OrderedFloat;
use pan_zoom_ui::CanvasState;

use crate::state::authoring::{AuthoringUiState, AutomationLaneId, AutomationOwner, CurveKeyDrag};

fn time(seconds: i64) -> MediaTime {
    MediaTime::new(seconds, 1).expect("time")
}

fn vector(x: f64, y: f64) -> PropertyValue {
    PropertyValue::Vec2(PropertyVec2 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
    })
}

fn lane(item_id: TimelineItemId) -> AutomationLaneId {
    AutomationLaneId {
        owner: AutomationOwner::Item(item_id),
        target: crate::state::authoring::AutomationTarget::AuthoredProperty {
            owner: AuthoringPropertyOwner::Item(item_id),
            key: "position".to_string(),
        },
    }
}

fn point(
    id: KeyframeId,
    seconds: i64,
    x: f64,
    y: f64,
    component: crate::state::authoring::CurveValueComponent,
) -> automation_lanes::AutomationChannelPoint {
    let full_value = vector(x, y);
    automation_lanes::AutomationChannelPoint {
        id,
        time: time(seconds),
        value: component_value(&full_value, component).expect("numeric component"),
        full_value,
        easing: library::animation::EasingFunction::Linear,
    }
}

fn series(
    id: AutomationLaneId,
    component: crate::state::authoring::CurveValueComponent,
    points: Vec<automation_lanes::AutomationChannelPoint>,
) -> CurveSeries {
    CurveSeries {
        id,
        component,
        label: component_name(component).to_string(),
        points,
    }
}

fn transform() -> CurveTransform {
    CurveTransform::new(
        Rect::from_min_size(Pos2::new(10.0, 20.0), Vec2::new(200.0, 100.0)),
        10.0,
        0.0,
        100.0,
        CanvasState::uniform(Vec2::ZERO, 1.0),
    )
    .expect("transform")
}

fn dragging_state(
    lane: AutomationLaneId,
    keyframe_id: KeyframeId,
    component: crate::state::authoring::CurveValueComponent,
    projected_time: MediaTime,
    projected_value: PropertyValue,
) -> AuthoringUiState {
    let mut state = AuthoringUiState::new(TimelineId::new());
    state.curve_editor.drag = Some(CurveKeyDrag {
        lane,
        component,
        keyframe_id,
        original_time: time(2),
        original_value: vector(10.0, 20.0),
        pointer_origin: Pos2::ZERO,
        projected_time,
        projected_value,
    });
    state
}

#[test]
fn vector_components_share_projected_time_but_keep_their_own_values() {
    let item_id = TimelineItemId::new();
    let lane = lane(item_id);
    let moved_id = KeyframeId::new();
    let sibling_id = KeyframeId::new();
    let x = series(
        lane.clone(),
        crate::state::authoring::CurveValueComponent::X,
        vec![
            point(
                moved_id,
                2,
                10.0,
                20.0,
                crate::state::authoring::CurveValueComponent::X,
            ),
            point(
                sibling_id,
                6,
                60.0,
                70.0,
                crate::state::authoring::CurveValueComponent::X,
            ),
        ],
    );
    let y = series(
        lane.clone(),
        crate::state::authoring::CurveValueComponent::Y,
        vec![
            point(
                moved_id,
                2,
                10.0,
                20.0,
                crate::state::authoring::CurveValueComponent::Y,
            ),
            point(
                sibling_id,
                6,
                60.0,
                70.0,
                crate::state::authoring::CurveValueComponent::Y,
            ),
        ],
    );
    let state = dragging_state(
        lane,
        moved_id,
        crate::state::authoring::CurveValueComponent::X,
        time(4),
        vector(30.0, 20.0),
    );

    assert_eq!(projected_point(&state, &x, &x.points[0]), (time(4), 30.0));
    assert_eq!(projected_point(&state, &y, &y.points[0]), (time(4), 20.0));
    assert_eq!(projected_point(&state, &x, &x.points[1]), (time(6), 60.0));
    assert_eq!(projected_point(&state, &y, &y.points[1]), (time(6), 70.0));
}

#[test]
fn curve_samples_use_projected_endpoints_after_a_key_crosses_its_neighbour() {
    let item_id = TimelineItemId::new();
    let lane = lane(item_id);
    let moved_id = KeyframeId::new();
    let sibling_id = KeyframeId::new();
    let x = series(
        lane.clone(),
        crate::state::authoring::CurveValueComponent::X,
        vec![
            point(
                moved_id,
                2,
                10.0,
                20.0,
                crate::state::authoring::CurveValueComponent::X,
            ),
            point(
                sibling_id,
                6,
                60.0,
                70.0,
                crate::state::authoring::CurveValueComponent::X,
            ),
        ],
    );
    let y = series(
        lane.clone(),
        crate::state::authoring::CurveValueComponent::Y,
        vec![
            point(
                moved_id,
                2,
                10.0,
                20.0,
                crate::state::authoring::CurveValueComponent::Y,
            ),
            point(
                sibling_id,
                6,
                60.0,
                70.0,
                crate::state::authoring::CurveValueComponent::Y,
            ),
        ],
    );
    let state = dragging_state(
        lane,
        moved_id,
        crate::state::authoring::CurveValueComponent::X,
        time(8),
        vector(80.0, 20.0),
    );
    let transform = transform();

    let x_samples = curve_samples(&state, &x, transform);
    assert_eq!(x_samples.first(), Some(&transform.point(6.0, 60.0)));
    assert_eq!(x_samples.last(), Some(&transform.point(8.0, 80.0)));
    assert!(x_samples.windows(2).all(|pair| pair[0].x <= pair[1].x));

    let y_samples = curve_samples(&state, &y, transform);
    assert_eq!(y_samples.first(), Some(&transform.point(6.0, 70.0)));
    assert_eq!(y_samples.last(), Some(&transform.point(8.0, 20.0)));
    assert!(y_samples.windows(2).all(|pair| pair[0].x <= pair[1].x));
}
