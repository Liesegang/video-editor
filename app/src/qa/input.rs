use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::sync::mpsc::Receiver;
use std::sync::Mutex;

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoordinateSpace {
    #[default]
    Points,
    Pixels,
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QaPointerButton {
    #[default]
    Primary,
    Secondary,
    Middle,
    Extra1,
    Extra2,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QaKey {
    A,
    L,
    T,
    Z,
    Space,
    Escape,
    Enter,
    Backspace,
}

impl From<QaKey> for egui::Key {
    fn from(value: QaKey) -> Self {
        match value {
            QaKey::A => Self::A,
            QaKey::L => Self::L,
            QaKey::T => Self::T,
            QaKey::Z => Self::Z,
            QaKey::Space => Self::Space,
            QaKey::Escape => Self::Escape,
            QaKey::Enter => Self::Enter,
            QaKey::Backspace => Self::Backspace,
        }
    }
}

impl From<QaPointerButton> for egui::PointerButton {
    fn from(value: QaPointerButton) -> Self {
        match value {
            QaPointerButton::Primary => Self::Primary,
            QaPointerButton::Secondary => Self::Secondary,
            QaPointerButton::Middle => Self::Middle,
            QaPointerButton::Extra1 => Self::Extra1,
            QaPointerButton::Extra2 => Self::Extra2,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
pub struct QaModifiers {
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub command: bool,
}

impl From<QaModifiers> for egui::Modifiers {
    fn from(value: QaModifiers) -> Self {
        let mac_cmd = cfg!(target_os = "macos") && value.command;
        Self {
            alt: value.alt,
            ctrl: value.ctrl,
            shift: value.shift,
            mac_cmd,
            command: value.command,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct QaPoint {
    pub x: f32,
    pub y: f32,
}

impl QaPoint {
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }

    fn to_points(self, space: CoordinateSpace, pixels_per_point: f32) -> egui::Pos2 {
        let divisor = match space {
            CoordinateSpace::Points => 1.0,
            CoordinateSpace::Pixels => pixels_per_point.max(f32::EPSILON),
        };
        egui::pos2(self.x / divisor, self.y / divisor)
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct PointerRequest {
    pub x: f32,
    pub y: f32,
    #[serde(default)]
    pub coordinate_space: CoordinateSpace,
    #[serde(default)]
    pub button: QaPointerButton,
    #[serde(default)]
    pub modifiers: QaModifiers,
}

impl PointerRequest {
    pub fn point(&self) -> QaPoint {
        QaPoint {
            x: self.x,
            y: self.y,
        }
    }
}

fn default_drag_steps() -> u16 {
    8
}

#[derive(Clone, Debug, Deserialize)]
pub struct DragRequest {
    pub from: QaPoint,
    pub to: QaPoint,
    #[serde(default)]
    pub coordinate_space: CoordinateSpace,
    #[serde(default)]
    pub button: QaPointerButton,
    #[serde(default)]
    pub modifiers: QaModifiers,
    #[serde(default = "default_drag_steps")]
    pub steps: u16,
}

#[derive(Clone, Debug, Deserialize)]
pub struct KeyRequest {
    pub key: QaKey,
    pub pressed: bool,
    #[serde(default)]
    pub modifiers: QaModifiers,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TextRequest {
    pub text: String,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct ScrollRequest {
    pub x: f32,
    pub y: f32,
    pub delta_x: f32,
    pub delta_y: f32,
    #[serde(default)]
    pub coordinate_space: CoordinateSpace,
    #[serde(default)]
    pub modifiers: QaModifiers,
}

#[derive(Clone, Copy, Debug, Deserialize)]
pub struct PinchRequest {
    pub x: f32,
    pub y: f32,
    /// Multiplicative native zoom factor (`1.0` means no change).
    pub factor: f32,
    #[serde(default)]
    pub coordinate_space: CoordinateSpace,
    #[serde(default)]
    pub modifiers: QaModifiers,
}

#[derive(Clone, Debug)]
pub enum InputAction {
    Move(PointerRequest),
    Press(PointerRequest),
    Release(PointerRequest),
    Click(PointerRequest),
    DoubleClick(PointerRequest),
    Drag(DragRequest),
    Key(KeyRequest),
    Text(TextRequest),
    Scroll(ScrollRequest),
    Pinch(PinchRequest),
}

impl InputAction {
    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Move(request)
            | Self::Press(request)
            | Self::Release(request)
            | Self::Click(request)
            | Self::DoubleClick(request) => {
                if request.point().is_finite() {
                    Ok(())
                } else {
                    Err("coordinates must be finite numbers")
                }
            }
            Self::Drag(request) => {
                if !request.from.is_finite() || !request.to.is_finite() {
                    return Err("coordinates must be finite numbers");
                }
                if !(1..=120).contains(&request.steps) {
                    return Err("drag steps must be between 1 and 120");
                }
                Ok(())
            }
            Self::Key(_) => Ok(()),
            Self::Text(request) => {
                if request.text.len() > 4096 {
                    Err("text input must be at most 4096 UTF-8 bytes")
                } else {
                    Ok(())
                }
            }
            Self::Scroll(request) => {
                if request.x.is_finite()
                    && request.y.is_finite()
                    && request.delta_x.is_finite()
                    && request.delta_y.is_finite()
                {
                    Ok(())
                } else {
                    Err("scroll coordinates and deltas must be finite numbers")
                }
            }
            Self::Pinch(request) => {
                if !request.x.is_finite() || !request.y.is_finite() {
                    return Err("pinch coordinates must be finite numbers");
                }
                if !request.factor.is_finite() || !(0.01..=100.0).contains(&request.factor) {
                    return Err("pinch factor must be between 0.01 and 100");
                }
                Ok(())
            }
        }
    }
}

#[derive(Debug)]
pub struct InputCommand {
    pub id: u64,
    pub action: InputAction,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionPhase {
    Queued,
    Injecting,
    Injected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct ActionStatus {
    pub id: u64,
    pub phase: ActionPhase,
}

#[derive(Default)]
pub struct ActionTracker {
    statuses: Mutex<BTreeMap<u64, ActionPhase>>,
}

impl ActionTracker {
    const MAX_STATUSES: usize = 512;

    pub fn set(&self, id: u64, phase: ActionPhase) {
        let Ok(mut statuses) = self.statuses.lock() else {
            return;
        };
        statuses.insert(id, phase);
        while statuses.len() > Self::MAX_STATUSES {
            let Some(first) = statuses.keys().next().copied() else {
                break;
            };
            statuses.remove(&first);
        }
    }

    pub fn get(&self, id: u64) -> Option<ActionStatus> {
        self.statuses.lock().ok().and_then(|statuses| {
            statuses
                .get(&id)
                .copied()
                .map(|phase| ActionStatus { id, phase })
        })
    }

    pub fn remove(&self, id: u64) {
        if let Ok(mut statuses) = self.statuses.lock() {
            statuses.remove(&id);
        }
    }
}

struct FrameStep {
    action_id: u64,
    events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
    final_step: bool,
}

pub struct InputSequencer {
    receiver: Receiver<InputCommand>,
    tracker: std::sync::Arc<ActionTracker>,
    commands: VecDeque<InputCommand>,
    steps: VecDeque<FrameStep>,
}

impl InputSequencer {
    pub fn new(receiver: Receiver<InputCommand>, tracker: std::sync::Arc<ActionTracker>) -> Self {
        Self {
            receiver,
            tracker,
            commands: VecDeque::new(),
            steps: VecDeque::new(),
        }
    }

    pub fn inject_for_frame(
        &mut self,
        ctx: &egui::Context,
        raw_input: &mut egui::RawInput,
        pixels_per_point: f32,
    ) {
        self.drain_receiver();
        if self.steps.is_empty() {
            self.prepare_next(pixels_per_point);
        }

        let injected_step = if let Some(step) = self.steps.pop_front() {
            // egui widgets read modifiers from `InputState`, not necessarily
            // from the PointerButton/Key event that triggered them. Mirror a
            // real held modifier in RawInput for every frame of the gesture.
            // A loopback QA gesture also represents deliberate user input,
            // even when the native test window is behind the HTTP client.
            // Mark that frame focused so focus-aware canvas interactions take
            // the same path as a foreground pointer gesture.
            raw_input.focused = true;
            raw_input.modifiers = step.modifiers;
            raw_input.events.extend(step.events);
            if step.final_step {
                self.tracker.set(step.action_id, ActionPhase::Injected);
            }
            true
        } else {
            false
        };

        // Always schedule a frame after an injected event, including the final
        // release. Widgets can apply a click at the end of the current pass
        // while their resulting popup or dock contents are only registered in
        // the following pass. Without this wake-up a background QA process can
        // publish the pre-click component frame indefinitely.
        if injected_step || !self.steps.is_empty() || !self.commands.is_empty() {
            ctx.request_repaint();
        }
    }

    fn drain_receiver(&mut self) {
        while let Ok(command) = self.receiver.try_recv() {
            self.commands.push_back(command);
        }
    }

    fn prepare_next(&mut self, pixels_per_point: f32) {
        let Some(command) = self.commands.pop_front() else {
            return;
        };
        self.tracker.set(command.id, ActionPhase::Injecting);
        self.steps = build_steps(command, pixels_per_point).into();
    }
}

fn pointer_event(
    position: egui::Pos2,
    button: QaPointerButton,
    pressed: bool,
    modifiers: QaModifiers,
) -> egui::Event {
    egui::Event::PointerButton {
        pos: position,
        button: button.into(),
        pressed,
        modifiers: modifiers.into(),
    }
}

fn build_steps(command: InputCommand, pixels_per_point: f32) -> Vec<FrameStep> {
    let action_id = command.id;
    let mut frames = Vec::new();
    let mut push = |events: Vec<egui::Event>, modifiers: QaModifiers| {
        frames.push(FrameStep {
            action_id,
            events,
            modifiers: modifiers.into(),
            final_step: false,
        });
    };

    match command.action {
        InputAction::Move(request) => {
            let pos = request
                .point()
                .to_points(request.coordinate_space, pixels_per_point);
            push(vec![egui::Event::PointerMoved(pos)], request.modifiers);
        }
        InputAction::Press(request) => {
            let pos = request
                .point()
                .to_points(request.coordinate_space, pixels_per_point);
            push(vec![egui::Event::PointerMoved(pos)], request.modifiers);
            push(
                vec![pointer_event(pos, request.button, true, request.modifiers)],
                request.modifiers,
            );
        }
        InputAction::Release(request) => {
            let pos = request
                .point()
                .to_points(request.coordinate_space, pixels_per_point);
            push(vec![egui::Event::PointerMoved(pos)], request.modifiers);
            push(
                vec![pointer_event(pos, request.button, false, request.modifiers)],
                request.modifiers,
            );
        }
        InputAction::Click(request) => {
            let pos = request
                .point()
                .to_points(request.coordinate_space, pixels_per_point);
            push(vec![egui::Event::PointerMoved(pos)], request.modifiers);
            push(
                vec![pointer_event(pos, request.button, true, request.modifiers)],
                request.modifiers,
            );
            push(
                vec![pointer_event(pos, request.button, false, request.modifiers)],
                request.modifiers,
            );
        }
        InputAction::DoubleClick(request) => {
            let pos = request
                .point()
                .to_points(request.coordinate_space, pixels_per_point);
            push(vec![egui::Event::PointerMoved(pos)], request.modifiers);
            push(
                vec![
                    pointer_event(pos, request.button, true, request.modifiers),
                    pointer_event(pos, request.button, false, request.modifiers),
                    pointer_event(pos, request.button, true, request.modifiers),
                    pointer_event(pos, request.button, false, request.modifiers),
                ],
                request.modifiers,
            );
        }
        InputAction::Drag(request) => {
            let from = request
                .from
                .to_points(request.coordinate_space, pixels_per_point);
            let to = request
                .to
                .to_points(request.coordinate_space, pixels_per_point);
            // Settle the pointer on the hit target before pressing. Combining
            // a long synthetic move and press in one frame makes drag-only
            // widgets treat the approach movement as the first drag delta.
            push(vec![egui::Event::PointerMoved(from)], request.modifiers);
            push(
                vec![pointer_event(from, request.button, true, request.modifiers)],
                request.modifiers,
            );
            for step in 1..=request.steps {
                let factor = f32::from(step) / f32::from(request.steps);
                push(
                    vec![egui::Event::PointerMoved(from.lerp(to, factor))],
                    request.modifiers,
                );
            }
            push(
                vec![pointer_event(to, request.button, false, request.modifiers)],
                request.modifiers,
            );
        }
        InputAction::Key(request) => {
            let key: egui::Key = request.key.into();
            push(
                vec![egui::Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed: request.pressed,
                    repeat: false,
                    modifiers: request.modifiers.into(),
                }],
                request.modifiers,
            );
        }
        InputAction::Text(request) => push(
            vec![egui::Event::Text(request.text)],
            QaModifiers::default(),
        ),
        InputAction::Scroll(request) => {
            let position = QaPoint {
                x: request.x,
                y: request.y,
            }
            .to_points(request.coordinate_space, pixels_per_point);
            push(
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Point,
                        delta: egui::vec2(request.delta_x, request.delta_y),
                        modifiers: request.modifiers.into(),
                    },
                ],
                request.modifiers,
            );
        }
        InputAction::Pinch(request) => {
            let position = QaPoint {
                x: request.x,
                y: request.y,
            }
            .to_points(request.coordinate_space, pixels_per_point);
            push(
                vec![
                    egui::Event::PointerMoved(position),
                    egui::Event::Zoom(request.factor),
                ],
                request.modifiers,
            );
        }
    }

    if let Some(last) = frames.last_mut() {
        last.final_step = true;
    }
    frames
}

#[cfg(test)]
mod tests {
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
}
