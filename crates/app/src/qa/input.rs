use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::sync::mpsc::Receiver;
use std::sync::Mutex;
use std::time::Duration;

mod validation;

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
    E,
    L,
    N,
    O,
    P,
    Q,
    S,
    T,
    Z,
    Comma,
    Space,
    Escape,
    Enter,
    Backspace,
    Delete,
}

impl From<QaKey> for egui::Key {
    fn from(value: QaKey) -> Self {
        match value {
            QaKey::A => Self::A,
            QaKey::E => Self::E,
            QaKey::L => Self::L,
            QaKey::N => Self::N,
            QaKey::O => Self::O,
            QaKey::P => Self::P,
            QaKey::Q => Self::Q,
            QaKey::S => Self::S,
            QaKey::T => Self::T,
            QaKey::Z => Self::Z,
            QaKey::Comma => Self::Comma,
            QaKey::Space => Self::Space,
            QaKey::Escape => Self::Escape,
            QaKey::Enter => Self::Enter,
            QaKey::Backspace => Self::Backspace,
            QaKey::Delete => Self::Delete,
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
    CloseRequest,
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
    viewport_close: bool,
    final_step: bool,
}

pub struct InputSequencer {
    receiver: Receiver<InputCommand>,
    tracker: std::sync::Arc<ActionTracker>,
    commands: VecDeque<InputCommand>,
    steps: VecDeque<FrameStep>,
    double_click_not_before: Option<f64>,
}

impl InputSequencer {
    pub fn new(receiver: Receiver<InputCommand>, tracker: std::sync::Arc<ActionTracker>) -> Self {
        Self {
            receiver,
            tracker,
            commands: VecDeque::new(),
            steps: VecDeque::new(),
            double_click_not_before: None,
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
            self.prepare_next(ctx, raw_input, pixels_per_point);
        }
        let waiting_for_double_click =
            self.steps.is_empty() && self.double_click_not_before.is_some();

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
            if step.viewport_close {
                let viewport = raw_input
                    .viewports
                    .entry(raw_input.viewport_id)
                    .or_default();
                viewport.events.push(egui::ViewportEvent::Close);
            }
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
        if injected_step
            || !self.steps.is_empty()
            || (!self.commands.is_empty() && !waiting_for_double_click)
        {
            ctx.request_repaint();
        }
    }

    fn drain_receiver(&mut self) {
        while let Ok(command) = self.receiver.try_recv() {
            self.commands.push_back(command);
        }
    }

    fn prepare_next(
        &mut self,
        ctx: &egui::Context,
        raw_input: &egui::RawInput,
        pixels_per_point: f32,
    ) {
        let Some(command) = self.commands.front() else {
            self.double_click_not_before = None;
            return;
        };
        if matches!(&command.action, InputAction::DoubleClick(_)) {
            let now = monotonic_input_time(ctx, raw_input);
            let deadline = *self.double_click_not_before.get_or_insert_with(|| {
                now + ctx.options(|options| options.input_options.max_double_click_delay * 2.0)
            });
            if now < deadline {
                return;
            }
        }

        let Some(command) = self.commands.pop_front() else {
            return;
        };
        self.double_click_not_before = None;
        self.tracker.set(command.id, ActionPhase::Injecting);
        self.steps = build_steps(command, pixels_per_point).into();
    }

    /// Schedules the wake for a delayed DoubleClick from inside the active
    /// egui pass. `inject_for_frame` runs in eframe's raw-input hook, before
    /// `begin_pass_repaint_logic` resets repaint state, so delayed repaint
    /// ownership must live here instead.
    pub(super) fn schedule_pending_repaint(&self, ctx: &egui::Context) {
        let Some(deadline) = self.double_click_not_before else {
            return;
        };
        let now = ctx.input(|input| input.time);
        if now < deadline {
            ctx.request_repaint_after(Duration::from_secs_f64(deadline - now));
        } else {
            ctx.request_repaint();
        }
    }
}

fn monotonic_input_time(ctx: &egui::Context, raw_input: &egui::RawInput) -> f64 {
    let previous = ctx.input(|input| input.time);
    raw_input
        .time
        .filter(|time| time.is_finite())
        .unwrap_or(previous)
        .max(previous)
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
            viewport_close: false,
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
        InputAction::CloseRequest => frames.push(FrameStep {
            action_id,
            events: Vec::new(),
            modifiers: egui::Modifiers::NONE,
            viewport_close: true,
            final_step: false,
        }),
    }

    if let Some(last) = frames.last_mut() {
        last.final_step = true;
    }
    frames
}

#[cfg(test)]
#[path = "input/tests.rs"]
mod tests;
