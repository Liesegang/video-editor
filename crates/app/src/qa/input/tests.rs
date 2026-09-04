use super::*;
use std::sync::mpsc;
use std::sync::Arc;

fn pointer_request(x: f32, y: f32) -> PointerRequest {
    PointerRequest {
        x,
        y,
        coordinate_space: CoordinateSpace::Points,
        button: QaPointerButton::Primary,
        modifiers: QaModifiers::default(),
    }
}

fn shift_command_modifiers() -> QaModifiers {
    QaModifiers {
        shift: true,
        command: true,
        ..QaModifiers::default()
    }
}

#[test]
fn click_settles_pointer_then_splits_press_and_release_frames() {
    let steps = build_steps(
        InputCommand {
            id: 7,
            action: InputAction::Click(pointer_request(12.0, 34.0)),
        },
        2.0,
    );
    assert_eq!(steps.len(), 3);
    assert!(matches!(
        steps[0].events.as_slice(),
        [egui::Event::PointerMoved(_)]
    ));
    assert!(matches!(
        steps[1].events.as_slice(),
        [egui::Event::PointerButton { pressed: true, .. }]
    ));
    assert!(matches!(
        steps[2].events.as_slice(),
        [egui::Event::PointerButton { pressed: false, .. }]
    ));
    assert!(steps[2].final_step);
}

#[test]
fn double_click_uses_two_real_clicks_after_a_settle_frame() {
    let steps = build_steps(
        InputCommand {
            id: 27,
            action: InputAction::DoubleClick(pointer_request(12.0, 34.0)),
        },
        2.0,
    );
    assert_eq!(steps.len(), 2);
    assert!(matches!(
        steps[0].events.as_slice(),
        [egui::Event::PointerMoved(_)]
    ));
    for (event, pressed) in steps[1].events.iter().zip([true, false, true, false]) {
        assert!(matches!(event, egui::Event::PointerButton {
                pressed: event_pressed,
                ..
            } if *event_pressed == pressed));
    }
    assert!(steps[1].final_step);
}

#[test]
fn double_click_reaches_egui_double_clicked_without_wall_clock_timing() {
    let steps = build_steps(
        InputCommand {
            id: 28,
            action: InputAction::DoubleClick(pointer_request(24.0, 36.0)),
        },
        1.0,
    );
    let context = egui::Context::default();
    let mut observed_double_click = false;
    for (frame, step) in steps.into_iter().enumerate() {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(160.0, 120.0),
            )),
            time: Some(frame as f64 * 10.0),
            modifiers: step.modifiers,
            events: step.events,
            ..Default::default()
        };
        let _output = context.run(input, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let (_, response) =
                    ui.allocate_exact_size(egui::vec2(120.0, 80.0), egui::Sense::click());
                observed_double_click |= response.double_clicked();
            });
        });
    }
    assert!(observed_double_click);
}

#[test]
fn standalone_press_and_release_settle_pointer_on_a_prior_frame() {
    for (action, pressed) in [
        (InputAction::Press(pointer_request(12.0, 34.0)), true),
        (InputAction::Release(pointer_request(56.0, 78.0)), false),
    ] {
        let steps = build_steps(InputCommand { id: 17, action }, 1.0);
        assert_eq!(steps.len(), 2);
        assert!(matches!(
            steps[0].events.as_slice(),
            [egui::Event::PointerMoved(_)]
        ));
        assert!(matches!(
            steps[1].events.as_slice(),
            [egui::Event::PointerButton {
                pressed: event_pressed,
                ..
            }] if *event_pressed == pressed
        ));
        assert!(steps[1].final_step);
    }
}

#[test]
fn drag_moves_over_multiple_frames_before_release() {
    let steps = build_steps(
        InputCommand {
            id: 8,
            action: InputAction::Drag(DragRequest {
                from: QaPoint { x: 10.0, y: 20.0 },
                to: QaPoint { x: 50.0, y: 60.0 },
                coordinate_space: CoordinateSpace::Points,
                button: QaPointerButton::Primary,
                modifiers: QaModifiers::default(),
                steps: 4,
            }),
        },
        1.0,
    );
    assert_eq!(steps.len(), 7);
    assert!(matches!(
        steps.first().unwrap().events.as_slice(),
        [egui::Event::PointerMoved(_)]
    ));
    assert!(matches!(
        steps[1].events.as_slice(),
        [egui::Event::PointerButton { pressed: true, .. }]
    ));
    assert!(matches!(
        steps.last().unwrap().events.as_slice(),
        [egui::Event::PointerButton { pressed: false, .. }]
    ));
}

#[test]
fn modified_click_and_drag_hold_modifiers_for_every_gesture_frame() {
    let mut click = pointer_request(12.0, 34.0);
    click.modifiers = shift_command_modifiers();
    let click_steps = build_steps(
        InputCommand {
            id: 18,
            action: InputAction::Click(click),
        },
        1.0,
    );
    assert!(click_steps
        .iter()
        .all(|step| step.modifiers.shift && step.modifiers.command));

    let drag_steps = build_steps(
        InputCommand {
            id: 19,
            action: InputAction::Drag(DragRequest {
                from: QaPoint { x: 10.0, y: 20.0 },
                to: QaPoint { x: 50.0, y: 60.0 },
                coordinate_space: CoordinateSpace::Points,
                button: QaPointerButton::Primary,
                modifiers: shift_command_modifiers(),
                steps: 4,
            }),
        },
        1.0,
    );
    assert!(drag_steps
        .iter()
        .all(|step| step.modifiers.shift && step.modifiers.command));
}

#[test]
fn modified_click_is_visible_to_input_state_then_resets_after_release() {
    let (sender, receiver) = mpsc::channel();
    let tracker = Arc::new(ActionTracker::default());
    let mut sequencer = InputSequencer::new(receiver, Arc::clone(&tracker));
    let mut request = pointer_request(12.0, 34.0);
    request.modifiers = shift_command_modifiers();
    sender
        .send(InputCommand {
            id: 20,
            action: InputAction::Click(request),
        })
        .unwrap();

    let context = egui::Context::default();
    for _ in 0..3 {
        let mut raw_input = egui::RawInput::default();
        sequencer.inject_for_frame(&context, &mut raw_input, 1.0);
        let _output = context.run(raw_input, |context| {
            let modifiers = context.input(|input| input.modifiers);
            assert!(modifiers.shift);
            assert!(modifiers.command);
        });
    }
    assert_eq!(tracker.get(20).unwrap().phase, ActionPhase::Injected);

    let mut raw_input = egui::RawInput::default();
    sequencer.inject_for_frame(&context, &mut raw_input, 1.0);
    let _output = context.run(raw_input, |context| {
        let modifiers = context.input(|input| input.modifiers);
        assert!(!modifiers.shift);
        assert!(!modifiers.command);
    });
}

#[test]
fn sequencer_tracks_injected_action() {
    let (sender, receiver) = mpsc::channel();
    let tracker = Arc::new(ActionTracker::default());
    let mut sequencer = InputSequencer::new(receiver, Arc::clone(&tracker));
    sender
        .send(InputCommand {
            id: 9,
            action: InputAction::Move(pointer_request(1.0, 2.0)),
        })
        .unwrap();

    let context = egui::Context::default();
    let mut raw_input = egui::RawInput {
        focused: false,
        ..egui::RawInput::default()
    };
    sequencer.inject_for_frame(&context, &mut raw_input, 1.0);
    assert_eq!(raw_input.events.len(), 1);
    assert!(raw_input.focused);
    assert_eq!(tracker.get(9).unwrap().phase, ActionPhase::Injected);
}

#[test]
fn final_input_step_requests_a_follow_up_frame() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let (sender, receiver) = mpsc::channel();
    let tracker = Arc::new(ActionTracker::default());
    let mut sequencer = InputSequencer::new(receiver, tracker);
    sender
        .send(InputCommand {
            id: 22,
            action: InputAction::Move(pointer_request(1.0, 2.0)),
        })
        .unwrap();

    let context = egui::Context::default();
    let repaint_requests = Arc::new(AtomicUsize::new(0));
    let observed_requests = Arc::clone(&repaint_requests);
    context.set_request_repaint_callback(move |_| {
        observed_requests.fetch_add(1, Ordering::Relaxed);
    });
    // Drain egui's initial two-pass repaint request so the callback below
    // observes only the sequencer's explicit wake-up.
    for _ in 0..3 {
        drop(context.run(egui::RawInput::default(), |_| {}));
    }
    repaint_requests.store(0, Ordering::Relaxed);

    let mut raw_input = egui::RawInput::default();
    sequencer.inject_for_frame(&context, &mut raw_input, 1.0);

    assert_eq!(raw_input.events.len(), 1);
    assert!(repaint_requests.load(Ordering::Relaxed) > 0);
}

#[test]
fn pixel_coordinates_are_converted_to_egui_points() {
    let mut request = pointer_request(40.0, 60.0);
    request.coordinate_space = CoordinateSpace::Pixels;
    let steps = build_steps(
        InputCommand {
            id: 10,
            action: InputAction::Move(request),
        },
        2.0,
    );
    assert!(matches!(
        steps[0].events.as_slice(),
        [egui::Event::PointerMoved(pos)] if *pos == egui::pos2(20.0, 30.0)
    ));
}

#[test]
fn key_press_is_injected_as_a_real_egui_event() {
    let steps = build_steps(
        InputCommand {
            id: 9,
            action: InputAction::Key(KeyRequest {
                key: QaKey::Space,
                pressed: true,
                modifiers: QaModifiers::default(),
            }),
        },
        1.0,
    );
    assert_eq!(steps.len(), 1);
    assert!(matches!(
        steps[0].events.as_slice(),
        [egui::Event::Key {
            key: egui::Key::Space,
            pressed: true,
            ..
        }]
    ));
    assert!(steps[0].final_step);
}

#[test]
fn key_step_exposes_its_modifiers_through_raw_input() {
    let steps = build_steps(
        InputCommand {
            id: 21,
            action: InputAction::Key(KeyRequest {
                key: QaKey::A,
                pressed: true,
                modifiers: shift_command_modifiers(),
            }),
        },
        1.0,
    );
    assert_eq!(steps.len(), 1);
    assert!(steps[0].modifiers.shift);
    assert!(steps[0].modifiers.command);
}

#[test]
fn text_is_injected_as_a_real_egui_event() {
    let steps = build_steps(
        InputCommand {
            id: 11,
            action: InputAction::Text(TextRequest {
                text: "QA入力".to_string(),
            }),
        },
        1.0,
    );
    assert!(matches!(
        steps[0].events.as_slice(),
        [egui::Event::Text(text)] if text == "QA入力"
    ));
    assert!(steps[0].final_step);
}

#[test]
fn scroll_moves_the_pointer_and_injects_a_real_wheel_event() {
    let steps = build_steps(
        InputCommand {
            id: 12,
            action: InputAction::Scroll(ScrollRequest {
                x: 80.0,
                y: 60.0,
                delta_x: 0.0,
                delta_y: -240.0,
                coordinate_space: CoordinateSpace::Pixels,
                modifiers: shift_command_modifiers(),
            }),
        },
        2.0,
    );
    assert!(matches!(
        steps[0].events.as_slice(),
        [
            egui::Event::PointerMoved(pos),
            egui::Event::MouseWheel { delta, modifiers, .. }
        ] if *pos == egui::pos2(40.0, 30.0)
            && *delta == egui::vec2(0.0, -240.0)
            && modifiers.command
    ));
    assert!(steps[0].modifiers.command);
    assert!(steps[0].final_step);
}

#[test]
fn pinch_moves_the_pointer_and_injects_a_real_zoom_event() {
    let action = InputAction::Pinch(PinchRequest {
        x: 80.0,
        y: 60.0,
        factor: 1.25,
        coordinate_space: CoordinateSpace::Pixels,
        modifiers: shift_command_modifiers(),
    });
    assert!(action.validate().is_ok());
    let steps = build_steps(InputCommand { id: 13, action }, 2.0);
    assert!(matches!(
        steps[0].events.as_slice(),
        [egui::Event::PointerMoved(pos), egui::Event::Zoom(factor)]
            if *pos == egui::pos2(40.0, 30.0) && *factor == 1.25
    ));
    assert!(steps[0].modifiers.command);
    assert!(steps[0].final_step);
}

#[test]
fn pinch_rejects_non_finite_coordinates_and_unbounded_factors() {
    let request = |x, factor| {
        InputAction::Pinch(PinchRequest {
            x,
            y: 60.0,
            factor,
            coordinate_space: CoordinateSpace::Points,
            modifiers: QaModifiers::default(),
        })
    };
    assert_eq!(
        request(f32::NAN, 1.0).validate(),
        Err("pinch coordinates must be finite numbers")
    );
    for factor in [f32::NAN, 0.0, 0.009, 100.1] {
        assert_eq!(
            request(80.0, factor).validate(),
            Err("pinch factor must be between 0.01 and 100")
        );
    }
}

#[test]
fn close_request_injects_the_real_root_viewport_event() {
    let (sender, receiver) = mpsc::sync_channel(1);
    let tracker = Arc::new(ActionTracker::default());
    sender
        .send(InputCommand {
            id: 14,
            action: InputAction::CloseRequest,
        })
        .unwrap();
    let mut sequencer = InputSequencer::new(receiver, tracker);
    let context = egui::Context::default();
    let mut raw_input = egui::RawInput::default();

    sequencer.inject_for_frame(&context, &mut raw_input, 1.0);

    assert!(raw_input
        .viewports
        .get(&egui::ViewportId::ROOT)
        .is_some_and(egui::ViewportInfo::close_requested));
}
