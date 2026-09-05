//! Real egui pointer sequences for Curve Editor keyframe dragging.

use super::*;
use library::editor::AuthoringPropertyOwner;
use library::model::authoring::{SourceRef, TimelineInterval, TimelineItemId};
use library::model::frame::color::Color;
use library::model::property::{PropertyValue, Vec2 as PropertyVec2};
use ordered_float::OrderedFloat;

const SCREEN_SIZE: Vec2 = Vec2::new(640.0, 480.0);
const DRAG_DELTA: Vec2 = Vec2::new(160.0, -40.0);

struct CurveDragFixture {
    service: TimelineEditorService,
    state: AuthoringUiState,
    curve: CurveSeries,
    transform: CurveTransform,
    item_id: TimelineItemId,
    keyframe_id: KeyframeId,
    original_time: MediaTime,
    original_value: PropertyValue,
}

fn property_vec2(x: f64, y: f64) -> PropertyValue {
    PropertyValue::Vec2(PropertyVec2 {
        x: OrderedFloat(x),
        y: OrderedFloat(y),
    })
}

fn fixture() -> CurveDragFixture {
    let service = TimelineEditorService::create_default("Curve key drag").expect("service");
    let project = service.snapshot().expect("default Project");
    let timeline_id = project.root_timeline_id;
    let track_id = project.timelines[&timeline_id].track_order[0];
    let (item_id, _) = service
        .add_item(
            track_id,
            "Vector key".to_string(),
            SourceRef::Solid {
                color: Color::black(),
            },
            TimelineInterval::new(MediaTime::zero(), MediaTime::new(10, 1).expect("duration"))
                .expect("interval"),
            0,
        )
        .expect("item");
    let original_time = MediaTime::new(2, 1).expect("key time");
    let original_value = property_vec2(10.0, 20.0);
    let (keyframe_id, _) = service
        .set_authored_property_keyframe_mode(
            AuthoringPropertyOwner::Item(item_id),
            "position".to_string(),
            original_time,
            original_value.clone(),
        )
        .expect("Position keyframe");
    let project = service.snapshot().expect("keyframed Project");
    let curve = automation_lanes::numeric_channels(&automation_lanes::collect_item_lanes(
        &project, item_id,
    ))
    .into_iter()
    .find(|curve| {
        curve.component == crate::state::authoring::CurveValueComponent::X
            && curve.points.iter().any(|point| point.id == keyframe_id)
    })
    .expect("Position X curve");
    let transform = CurveTransform::new(
        Rect::from_min_size(Pos2::new(100.0, 100.0), Vec2::new(400.0, 200.0)),
        10.0,
        0.0,
        100.0,
        CanvasState::uniform(Vec2::ZERO, 1.0),
    )
    .expect("curve transform");
    CurveDragFixture {
        service,
        state: AuthoringUiState::new(timeline_id),
        curve,
        transform,
        item_id,
        keyframe_id,
        original_time,
        original_value,
    }
}

fn pointer_button(position: Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos: position,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn escape_pressed() -> egui::Event {
    egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: Some(egui::Key::Escape),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

fn render_frame(
    context: &egui::Context,
    fixture: &mut CurveDragFixture,
    frame: usize,
    events: Vec<egui::Event>,
) {
    drop(context.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN_SIZE)),
            time: Some(frame as f64 / 60.0),
            events,
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default().show(context, |ui| {
                update_key_drag(ui, &mut fixture.state, fixture.transform);
                paint_curve(
                    ui,
                    &mut fixture.state,
                    &fixture.service,
                    &fixture.curve,
                    Color32::LIGHT_BLUE,
                    fixture.transform,
                );
                finish_key_drag(ui, &mut fixture.state, &fixture.service);
            });
        },
    ));
}

fn render_curve_canvas(
    context: &egui::Context,
    fixture: &mut CurveDragFixture,
    frame: usize,
    series: &[CurveSeries],
) {
    let project = fixture.service.snapshot().expect("Project snapshot");
    let owner = AutomationOwner::Item(fixture.item_id);
    drop(context.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN_SIZE)),
            time: Some(frame as f64 / 60.0),
            ..Default::default()
        },
        |context| {
            egui::CentralPanel::default().show(context, |ui| {
                curve_canvas(
                    ui,
                    &project,
                    &mut fixture.state,
                    &fixture.service,
                    &owner,
                    series,
                    Rect::from_min_size(Pos2::new(20.0, 20.0), Vec2::new(560.0, 360.0)),
                );
            });
        },
    ));
}

fn key_value(fixture: &CurveDragFixture) -> (f64, PropertyValue) {
    let project = fixture.service.snapshot().expect("Project snapshot");
    let property = project.items[&fixture.item_id]
        .authored_properties
        .get("position")
        .expect("Position property");
    let keyframe = property
        .keyframes()
        .into_iter()
        .find(|keyframe| keyframe.id == fixture.keyframe_id)
        .expect("Position keyframe");
    (keyframe.time.into_inner(), keyframe.value)
}

fn assert_key_value(
    fixture: &CurveDragFixture,
    expected_time: f64,
    expected_x: f64,
    expected_y: f64,
) {
    let (time, value) = key_value(fixture);
    assert!((time - expected_time).abs() <= 1.0e-6);
    let PropertyValue::Vec2(value) = value else {
        panic!("Position keyframe must remain Vec2");
    };
    assert!((value.x.into_inner() - expected_x).abs() <= 1.0e-6);
    assert_eq!(value.y.into_inner(), expected_y);
}

fn assert_projected_delta(fixture: &CurveDragFixture, delta: Vec2) {
    let drag = fixture
        .state
        .curve_editor
        .drag
        .as_ref()
        .expect("active drag");
    let expected_time =
        fixture.original_time.to_seconds_f64() + fixture.transform.delta_time(delta.x);
    assert!(
        (drag.projected_time.to_seconds_f64() - expected_time).abs() <= 1.0e-6,
        "projected time did not use the total drag from its immutable origin"
    );
    let PropertyValue::Vec2(value) = &drag.projected_value else {
        panic!("Position drag must retain its Vec2 value");
    };
    assert!(
        (value.x.into_inner() - (10.0 + fixture.transform.delta_value(delta.y))).abs() <= 1.0e-6
    );
    assert_eq!(
        value.y.into_inner(),
        20.0,
        "dragging X must not modify the sibling Y channel"
    );
}

fn begin_drag(context: &egui::Context, fixture: &mut CurveDragFixture, frame: &mut usize) -> Pos2 {
    let start = fixture
        .transform
        .point(fixture.original_time.to_seconds_f64(), 10.0);
    render_frame(context, fixture, *frame, Vec::new());
    *frame += 1;
    render_frame(
        context,
        fixture,
        *frame,
        vec![
            egui::Event::PointerMoved(start),
            pointer_button(start, true),
        ],
    );
    *frame += 1;
    start
}

#[test]
fn changed_project_cancels_held_key_before_release_can_overwrite_it() {
    let context = egui::Context::default();
    let mut fixture = fixture();
    let mut frame = 0;
    let start = begin_drag(&context, &mut fixture, &mut frame);
    let endpoint = start + DRAG_DELTA;
    render_frame(
        &context,
        &mut fixture,
        frame,
        vec![egui::Event::PointerMoved(endpoint)],
    );
    assert!(fixture.state.curve_editor.drag.is_some());
    fixture
        .service
        .set_authored_property_constant(
            AuthoringPropertyOwner::Item(fixture.item_id),
            "opacity".into(),
            PropertyValue::from(0.5),
        )
        .unwrap();
    let (changed, revision) = fixture.service.snapshot_with_revision().unwrap();
    render_frame(
        &context,
        &mut fixture,
        frame + 1,
        vec![pointer_button(endpoint, false)],
    );
    assert!(fixture.state.curve_editor.drag.is_none());
    assert_eq!(fixture.service.revision().unwrap(), revision);
    assert_eq!(
        fixture.service.snapshot().unwrap().as_ref(),
        changed.as_ref()
    );
    assert_key_value(&fixture, 2.0, 10.0, 20.0);
}

#[test]
fn total_drag_is_step_independent_and_commits_once_with_release_frame_motion() {
    for steps in [2_usize, 16] {
        let context = egui::Context::default();
        let mut fixture = fixture();
        let before = fixture.service.snapshot().expect("before drag");
        let before_revision = fixture.service.revision().expect("before revision");
        let mut frame = 0;
        let start = begin_drag(&context, &mut fixture, &mut frame);

        for step in 1..steps {
            let delta = DRAG_DELTA * (step as f32 / steps as f32);
            render_frame(
                &context,
                &mut fixture,
                frame,
                vec![egui::Event::PointerMoved(start + delta)],
            );
            frame += 1;
            assert_projected_delta(&fixture, delta);
            let projected = fixture
                .state
                .curve_editor
                .drag
                .as_ref()
                .expect("held drag")
                .projected_value
                .clone();
            render_frame(&context, &mut fixture, frame, Vec::new());
            frame += 1;
            assert_eq!(
                fixture
                    .state
                    .curve_editor
                    .drag
                    .as_ref()
                    .expect("idle held drag")
                    .projected_value,
                projected,
                "an idle held frame must not snap back"
            );
            assert_eq!(
                fixture.service.revision().expect("held revision"),
                before_revision,
                "drag previews must not write history"
            );
        }

        let endpoint = start + DRAG_DELTA;
        render_frame(
            &context,
            &mut fixture,
            frame,
            vec![
                egui::Event::PointerMoved(endpoint),
                pointer_button(endpoint, false),
            ],
        );
        assert!(fixture.state.curve_editor.drag.is_none());
        assert_eq!(
            fixture.service.revision().expect("release revision").get(),
            before_revision.get() + 1,
            "one pointer gesture must create one command"
        );
        assert_key_value(&fixture, 6.0, 30.0, 20.0);

        fixture
            .service
            .undo()
            .expect("Undo drag")
            .expect("one drag command");
        assert_eq!(
            fixture.service.snapshot().expect("undo snapshot").as_ref(),
            before.as_ref()
        );
    }
}

#[test]
fn active_drag_survives_off_plot_excursion_and_uses_reentry_position() {
    let context = egui::Context::default();
    let mut fixture = fixture();
    let before_revision = fixture.service.revision().expect("before revision");
    let mut frame = 0;
    let start = begin_drag(&context, &mut fixture, &mut frame);
    let outside = fixture.transform.rect.right_top() + Vec2::new(90.0, -70.0);
    render_frame(
        &context,
        &mut fixture,
        frame,
        vec![egui::Event::PointerMoved(outside)],
    );
    frame += 1;
    assert!(fixture.state.curve_editor.drag.is_some());
    render_frame(&context, &mut fixture, frame, Vec::new());
    frame += 1;
    assert!(fixture.state.curve_editor.drag.is_some());

    let reentry = start + DRAG_DELTA * 0.75;
    render_frame(
        &context,
        &mut fixture,
        frame,
        vec![egui::Event::PointerMoved(reentry)],
    );
    frame += 1;
    assert_projected_delta(&fixture, DRAG_DELTA * 0.75);
    let endpoint = start + DRAG_DELTA;
    render_frame(
        &context,
        &mut fixture,
        frame,
        vec![
            egui::Event::PointerMoved(endpoint),
            pointer_button(endpoint, false),
        ],
    );

    assert_eq!(
        fixture.service.revision().expect("release revision").get(),
        before_revision.get() + 1
    );
    assert_key_value(&fixture, 6.0, 30.0, 20.0);
}

#[test]
fn escape_cancels_drag_without_writing_project_or_history() {
    let context = egui::Context::default();
    let mut fixture = fixture();
    let before = fixture.service.snapshot().expect("before drag");
    let before_revision = fixture.service.revision().expect("before revision");
    let mut frame = 0;
    let start = begin_drag(&context, &mut fixture, &mut frame);
    render_frame(
        &context,
        &mut fixture,
        frame,
        vec![egui::Event::PointerMoved(start + DRAG_DELTA * 0.5)],
    );
    frame += 1;
    assert_projected_delta(&fixture, DRAG_DELTA * 0.5);
    render_frame(&context, &mut fixture, frame, vec![escape_pressed()]);

    assert!(fixture.state.curve_editor.drag.is_none());
    assert_eq!(
        fixture.service.revision().expect("cancel revision"),
        before_revision
    );
    assert_eq!(
        fixture
            .service
            .snapshot()
            .expect("cancel snapshot")
            .as_ref(),
        before.as_ref()
    );
    assert_eq!(
        key_value(&fixture),
        (
            fixture.original_time.to_seconds_f64(),
            fixture.original_value
        )
    );
}

#[test]
fn curve_canvas_waits_for_first_key_before_fitting_and_keeps_the_view_domain_stable() {
    let context = egui::Context::default();
    let mut fixture = fixture();
    let mut empty = fixture.curve.clone();
    empty.points.clear();

    render_curve_canvas(&context, &mut fixture, 0, &[empty]);
    assert_eq!(
        fixture.state.curve_editor.value_range, None,
        "a numeric constant channel must not persist the temporary empty-state range"
    );

    let initial_curve = fixture.curve.clone();
    render_curve_canvas(
        &context,
        &mut fixture,
        1,
        std::slice::from_ref(&initial_curve),
    );
    let fitted = fixture
        .state
        .curve_editor
        .value_range
        .expect("first key fits the value domain");
    assert_eq!(fitted, value_extent(std::slice::from_ref(&initial_curve)));

    let mut edited = initial_curve;
    edited.points[0].value = 100_000.0;
    edited.points[0].full_value = property_vec2(100_000.0, 20.0);
    assert_ne!(value_extent(std::slice::from_ref(&edited)), fitted);
    render_curve_canvas(&context, &mut fixture, 2, std::slice::from_ref(&edited));
    assert_eq!(
        fixture.state.curve_editor.value_range,
        Some(fitted),
        "editing an extremum must not rescale the view underneath the pointer"
    );
}
