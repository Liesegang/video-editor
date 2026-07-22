//! Domain-neutral pointer orchestration and host intents.

use egui::{Pos2, Rect, Vec2};

use crate::input::interaction_input;
use crate::layout_swipe::{hit_anchor as hit_layout_swipe_anchor, LayoutSwipeState};
use crate::{
    after_click, after_marquee, GraphFrame, ItemId, LayoutSwipeHitArea, LayoutSwipeIntent,
    LayoutSwipePhase, PortDirection, WireDescriptor,
};

mod layout_swipe_preflight;
pub(crate) use layout_swipe_preflight::layout_swipe_wants_pointer;

const MARQUEE_DRAG_THRESHOLD: f32 = 4.0;
const PORT_HIT_RADIUS: f32 = 9.0;
const WIRE_HIT_RADIUS: f32 = 8.0;
const GROUP_RESIZE_HIT_WIDTH: f32 = 7.0;
const MIN_GROUP_SIZE: Vec2 = Vec2::new(80.0, 48.0);

/// A mutation request against the host's authoritative graph.
///
/// Outputs contain only host IDs and values needed to perform one edit. The
/// crate never applies them to a shadow graph.
#[derive(Clone, Debug, PartialEq)]
pub enum EditorOutput<NodeId, PortId, WireId, GroupId> {
    Select {
        items: Vec<ItemId<NodeId, GroupId, WireId>>,
        primary: Option<ItemId<NodeId, GroupId, WireId>>,
    },
    Move {
        items: Vec<ItemId<NodeId, GroupId, WireId>>,
        delta: Vec2,
    },
    /// A non-mutating directional-layout gesture for the host to interpret.
    LayoutSwipe(LayoutSwipeIntent<NodeId>),
    Connect {
        from: PortId,
        to: PortId,
    },
    Disconnect {
        wire: WireId,
    },
    Delete {
        items: Vec<ItemId<NodeId, GroupId, WireId>>,
    },
    Reparent {
        nodes: Vec<NodeId>,
        parent: Option<GroupId>,
    },
    ResizeGroup {
        group: GroupId,
        rect: Rect,
    },
    DeselectWire {
        wire: WireId,
    },
}

/// Enables coherent subsets while a host incrementally replaces an existing
/// renderer. Disabled gestures emit no intent and retain no state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InteractionOptions {
    pub select: bool,
    /// Precise curve selection is independently gated at overview scale.
    pub select_wires: bool,
    /// Rectangle selection is independently gated at overview scale.
    pub marquee: bool,
    pub move_items: bool,
    pub connect: bool,
    pub disconnect: bool,
    pub delete: bool,
    pub reparent: bool,
    pub resize_groups: bool,
    /// Hold-A directional layout gesture target policy.
    pub layout_swipe: LayoutSwipeHitArea,
}

impl InteractionOptions {
    pub const ALL: Self = Self {
        select: true,
        select_wires: true,
        marquee: true,
        move_items: true,
        connect: true,
        disconnect: true,
        delete: true,
        reparent: true,
        resize_groups: true,
        layout_swipe: LayoutSwipeHitArea::Header,
    };

    /// Production migration slice: logical click/marquee selection and wire
    /// deselection are owned here while legacy movement/wire gestures remain
    /// disabled until their adapters are moved as one transaction.
    pub const SELECTION: Self = Self {
        select: true,
        select_wires: true,
        marquee: true,
        move_items: false,
        connect: false,
        disconnect: false,
        delete: false,
        reparent: false,
        resize_groups: false,
        layout_swipe: LayoutSwipeHitArea::Header,
    };

    /// Overview interaction keeps large semantic targets and blank-canvas
    /// deselection available while precise wire and marquee gestures are off.
    pub const OVERVIEW_SELECTION: Self = Self {
        select: true,
        select_wires: false,
        marquee: false,
        move_items: false,
        connect: false,
        disconnect: false,
        delete: false,
        reparent: false,
        resize_groups: false,
        layout_swipe: LayoutSwipeHitArea::Node,
    };
}

impl Default for InteractionOptions {
    fn default() -> Self {
        Self::ALL
    }
}

#[derive(Clone, Debug)]
enum Movable<NodeId, GroupId> {
    Node(NodeId),
    Group(GroupId),
}

#[derive(Clone, Debug)]
enum Gesture<NodeId, PortId, GroupId> {
    Marquee {
        start: Pos2,
        current: Pos2,
        additive: bool,
        transform: egui::emath::TSTransform,
    },
    Move {
        items: Vec<Movable<NodeId, GroupId>>,
        previous: Pos2,
        current: Pos2,
        transform: egui::emath::TSTransform,
    },
    Connect {
        from: PortId,
        current: Pos2,
        transform: egui::emath::TSTransform,
    },
    Resize {
        group: GroupId,
        initial_rect: Rect,
        start: Pos2,
        current: Pos2,
        transform: egui::emath::TSTransform,
    },
    LayoutSwipe(LayoutSwipeState<NodeId>),
}

impl<NodeId, PortId, GroupId> Gesture<NodeId, PortId, GroupId> {
    const fn transform(&self) -> egui::emath::TSTransform {
        match self {
            Self::Marquee { transform, .. }
            | Self::Move { transform, .. }
            | Self::Connect { transform, .. }
            | Self::Resize { transform, .. } => *transform,
            Self::LayoutSwipe(gesture) => gesture.transform(),
        }
    }
}

/// Pointer-lifetime state only. It deliberately contains no graph snapshot,
/// authoritative selection, position map, connection list, undo entry, or
/// render cache.
#[derive(Clone, Debug)]
pub struct InteractionState<NodeId, PortId, WireId, GroupId> {
    gesture: Option<Gesture<NodeId, PortId, GroupId>>,
    wire_marker: std::marker::PhantomData<WireId>,
}

impl<NodeId, PortId, WireId, GroupId> Default
    for InteractionState<NodeId, PortId, WireId, GroupId>
{
    fn default() -> Self {
        Self {
            gesture: None,
            wire_marker: std::marker::PhantomData,
        }
    }
}

impl<NodeId, PortId, WireId, GroupId> InteractionState<NodeId, PortId, WireId, GroupId> {
    /// Transform frozen for the active direct-manipulation gesture.
    pub fn locked_transform(&self) -> Option<egui::emath::TSTransform> {
        self.gesture.as_ref().map(Gesture::transform)
    }

    pub const fn is_marquee_active(&self) -> bool {
        matches!(self.gesture, Some(Gesture::Marquee { .. }))
    }

    pub const fn is_active(&self) -> bool {
        self.gesture.is_some()
    }

    /// Whether the host must suppress competing move, reparent, pan, and zoom
    /// behavior while a directional-layout gesture owns the pointer.
    pub const fn is_layout_swipe_active(&self) -> bool {
        matches!(self.gesture, Some(Gesture::LayoutSwipe(_)))
    }

    pub fn cancel(&mut self) {
        self.gesture = None;
    }
}

pub(crate) fn interact<NodeId, PortId, WireId, GroupId, Key>(
    ui: &mut egui::Ui,
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    state: &mut InteractionState<NodeId, PortId, WireId, GroupId>,
    options: InteractionOptions,
    pointer_blocked: bool,
) -> Vec<EditorOutput<NodeId, PortId, WireId, GroupId>>
where
    NodeId: Clone + Eq,
    PortId: Clone + Eq,
    WireId: Clone + Eq,
    GroupId: Clone + Eq,
    Key: Copy + Eq,
{
    if (!options.select || !options.marquee)
        && matches!(state.gesture, Some(Gesture::Marquee { .. }))
    {
        state.cancel();
    }
    let input = interaction_input(ui);
    let wants_keyboard = ui.ctx().wants_keyboard_input();
    if !input.focused {
        let output = cancel_layout_swipe(state, input.pointer).map(EditorOutput::LayoutSwipe);
        state.cancel();
        return output.into_iter().collect();
    }
    if input.escape && state.is_layout_swipe_active() {
        return cancel_layout_swipe(state, input.pointer)
            .map(EditorOutput::LayoutSwipe)
            .into_iter()
            .collect();
    }

    let mut outputs = keyboard_outputs(ui, frame, state, options);
    if input.escape {
        return outputs;
    }

    let navigation_owns_pointer = input.space_down || input.middle_down;
    let a_held_through_release =
        input.a_down || (input.released && input.pointer_released_before_a);
    let layout_conflict = wants_keyboard
        || navigation_owns_pointer
        || options.layout_swipe == LayoutSwipeHitArea::Disabled
        || !a_held_through_release;
    if state.is_layout_swipe_active() && layout_conflict {
        if let Some(cancel) = cancel_layout_swipe(state, input.pointer) {
            outputs.push(EditorOutput::LayoutSwipe(cancel));
        }
        return outputs;
    }

    if state.gesture.is_none()
        && input.pressed
        && !pointer_blocked
        && !navigation_owns_pointer
        && input
            .press_position
            .is_some_and(|position| frame.viewport.contains(position))
    {
        let Some(screen_position) = input.press_position else {
            return outputs;
        };
        let graph_position = frame.graph_position(screen_position);
        begin_gesture(
            ui,
            frame,
            state,
            options,
            graph_position,
            screen_position,
            input.press_modifiers,
            input.a_down_at_press && !wants_keyboard,
            &mut outputs,
        );
    }

    if state.is_layout_swipe_active() && !a_held_through_release {
        if let Some(cancel) = cancel_layout_swipe(state, input.pointer) {
            outputs.push(EditorOutput::LayoutSwipe(cancel));
        }
        return outputs;
    }

    if let Some(position) = input.pointer {
        update_gesture(
            frame,
            state,
            options,
            position,
            input.down,
            input.released,
            &mut outputs,
        );
    }

    paint_transient(ui, frame, state);

    if input.released {
        finish_gesture(frame, state, options, input.pointer, &mut outputs);
    } else if !input.down && !input.pressed {
        if let Some(cancel) = cancel_layout_swipe(state, input.pointer) {
            outputs.push(EditorOutput::LayoutSwipe(cancel));
        } else {
            state.cancel();
        }
    }

    outputs
}

fn cancel_layout_swipe<NodeId, PortId, WireId, GroupId>(
    state: &mut InteractionState<NodeId, PortId, WireId, GroupId>,
    current: Option<Pos2>,
) -> Option<LayoutSwipeIntent<NodeId>>
where
    NodeId: Clone,
{
    match state.gesture.take() {
        Some(Gesture::LayoutSwipe(gesture)) => {
            Some(gesture.finish(LayoutSwipePhase::Cancel, current))
        }
        gesture => {
            state.gesture = gesture;
            None
        }
    }
}

fn keyboard_outputs<NodeId, PortId, WireId, GroupId, Key>(
    ui: &egui::Ui,
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    state: &mut InteractionState<NodeId, PortId, WireId, GroupId>,
    options: InteractionOptions,
) -> Vec<EditorOutput<NodeId, PortId, WireId, GroupId>>
where
    NodeId: Clone,
    WireId: Clone + Eq,
    GroupId: Clone,
{
    let wants_keyboard = ui.ctx().wants_keyboard_input();
    let (delete, escape) = ui.input(|input| {
        (
            input.key_pressed(egui::Key::Delete) || input.key_pressed(egui::Key::Backspace),
            input.key_pressed(egui::Key::Escape),
        )
    });
    if wants_keyboard {
        return Vec::new();
    }

    if escape {
        state.cancel();
        return frame
            .selection
            .items
            .iter()
            .filter_map(|item| match item {
                ItemId::Wire(wire) => Some(EditorOutput::DeselectWire { wire: wire.clone() }),
                ItemId::Node(_) | ItemId::Group(_) => None,
            })
            .collect();
    }

    if !delete || !options.delete {
        return Vec::new();
    }

    let mut outputs = Vec::new();
    let mut items = Vec::new();
    for item in frame.selection.items {
        match item {
            ItemId::Wire(wire) if options.disconnect => {
                if frame
                    .wires
                    .iter()
                    .any(|descriptor| descriptor.id == *wire && descriptor.editable)
                {
                    outputs.push(EditorOutput::Disconnect { wire: wire.clone() });
                }
            }
            ItemId::Node(node) => items.push(ItemId::Node(node.clone())),
            ItemId::Group(group) => items.push(ItemId::Group(group.clone())),
            ItemId::Wire(_) => {}
        }
    }
    if !items.is_empty() {
        outputs.push(EditorOutput::Delete { items });
    }
    outputs
}

#[allow(
    clippy::too_many_arguments,
    reason = "the frame, input snapshot, and output sink are one immediate-mode gesture boundary"
)]
fn begin_gesture<NodeId, PortId, WireId, GroupId, Key>(
    ui: &egui::Ui,
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    state: &mut InteractionState<NodeId, PortId, WireId, GroupId>,
    options: InteractionOptions,
    graph_position: Pos2,
    screen_position: Pos2,
    modifiers: egui::Modifiers,
    layout_swipe_requested: bool,
    outputs: &mut Vec<EditorOutput<NodeId, PortId, WireId, GroupId>>,
) where
    NodeId: Clone + Eq,
    PortId: Clone + Eq,
    WireId: Clone + Eq,
    GroupId: Clone + Eq,
    Key: Copy + Eq,
{
    if options.connect {
        if let Some(port) = hit_port(frame, graph_position) {
            state.gesture = Some(Gesture::Connect {
                from: port.id.clone(),
                current: screen_position,
                transform: frame.transform,
            });
            capture_pointer(ui);
            return;
        }
    }

    if options.resize_groups {
        if let Some(group) = hit_group_resize(frame, graph_position) {
            state.gesture = Some(Gesture::Resize {
                group: group.id.clone(),
                initial_rect: group.rect,
                start: graph_position,
                current: screen_position,
                transform: frame.transform,
            });
            capture_pointer(ui);
            if options.select {
                select_non_wire(
                    frame,
                    ItemId::Group(group.id.clone()),
                    modifiers.shift,
                    outputs,
                );
            }
            return;
        }
    }

    if layout_swipe_requested {
        if let Some(anchor) = hit_layout_swipe_anchor(frame, graph_position, options.layout_swipe) {
            let gesture = LayoutSwipeState::new(
                anchor.id.clone(),
                screen_position,
                modifiers,
                frame.transform,
            );
            outputs.push(EditorOutput::LayoutSwipe(gesture.start_intent()));
            state.gesture = Some(Gesture::LayoutSwipe(gesture));
            capture_pointer(ui);
            return;
        }
    }

    if let Some(clicked) = hit_selectable(frame, graph_position) {
        if options.select {
            select_non_wire(frame, clicked.clone(), modifiers.shift, outputs);
        }
        let move_handle_hit = match &clicked {
            ItemId::Node(node_id) => frame
                .nodes
                .iter()
                .find(|node| node.id == *node_id)
                .is_some_and(|node| node.header_rect.contains(graph_position)),
            ItemId::Group(group_id) => frame
                .groups
                .iter()
                .find(|group| group.id == *group_id)
                .is_some_and(|group| group.header_rect.contains(graph_position)),
            ItemId::Wire(_) => false,
        };
        if options.move_items && move_handle_hit {
            let items = movable_selection(frame, clicked);
            state.gesture = Some(Gesture::Move {
                items,
                previous: graph_position,
                current: screen_position,
                transform: frame.transform,
            });
            capture_pointer(ui);
        }
        return;
    }

    if options.select_wires || options.disconnect {
        if let Some(wire) = hit_wire(frame, graph_position) {
            if modifiers.alt && options.disconnect && wire.editable {
                outputs.push(EditorOutput::Disconnect {
                    wire: wire.id.clone(),
                });
            } else if options.select {
                let clicked = ItemId::Wire(wire.id.clone());
                let (items, primary) = after_click(
                    frame.selection.items,
                    frame.selection.primary.clone(),
                    clicked,
                    modifiers.shift,
                );
                outputs.push(EditorOutput::Select { items, primary });
            }
            return;
        }
    }

    if options.select && !modifiers.alt {
        if options.marquee {
            state.gesture = Some(Gesture::Marquee {
                start: screen_position,
                current: screen_position,
                additive: modifiers.shift,
                transform: frame.transform,
            });
            capture_pointer(ui);
        } else if !modifiers.shift {
            clear_selection(frame, outputs);
        }
    }
}

fn capture_pointer(ui: &egui::Ui) {
    ui.ctx()
        .set_dragged_id(ui.make_persistent_id("node_editor_ui.interaction"));
}

fn update_gesture<NodeId, PortId, WireId, GroupId, Key>(
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    state: &mut InteractionState<NodeId, PortId, WireId, GroupId>,
    options: InteractionOptions,
    screen_position: Pos2,
    pointer_down: bool,
    pointer_released: bool,
    outputs: &mut Vec<EditorOutput<NodeId, PortId, WireId, GroupId>>,
) where
    NodeId: Clone,
    PortId: Clone,
    WireId: Clone,
    GroupId: Clone,
{
    let Some(gesture) = state.gesture.as_mut() else {
        return;
    };
    match gesture {
        Gesture::Marquee { current, .. } | Gesture::Connect { current, .. } => {
            *current = screen_position;
        }
        Gesture::Move {
            items,
            previous,
            current,
            transform,
        } => {
            *current = screen_position;
            if pointer_down && options.move_items {
                let graph_position = transform.inverse() * screen_position;
                let delta = graph_position - *previous;
                if delta != Vec2::ZERO {
                    outputs.push(EditorOutput::Move {
                        items: items
                            .iter()
                            .map(|item| match item {
                                Movable::Node(node) => ItemId::Node(node.clone()),
                                Movable::Group(group) => ItemId::Group(group.clone()),
                            })
                            .collect(),
                        delta,
                    });
                    *previous = graph_position;
                }
            }
        }
        Gesture::Resize {
            group,
            initial_rect,
            start,
            current,
            transform,
        } => {
            *current = screen_position;
            if pointer_down && options.resize_groups {
                let graph_position = transform.inverse() * screen_position;
                let delta = graph_position - *start;
                let size = (initial_rect.size() + delta).max(MIN_GROUP_SIZE);
                outputs.push(EditorOutput::ResizeGroup {
                    group: group.clone(),
                    rect: Rect::from_min_size(initial_rect.min, size),
                });
            }
        }
        Gesture::LayoutSwipe(gesture) => {
            if pointer_down || pointer_released {
                if let Some(update) = gesture.update(screen_position) {
                    outputs.push(EditorOutput::LayoutSwipe(update));
                }
            }
        }
    }
    let _ = frame;
}

fn finish_gesture<NodeId, PortId, WireId, GroupId, Key>(
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    state: &mut InteractionState<NodeId, PortId, WireId, GroupId>,
    options: InteractionOptions,
    pointer: Option<Pos2>,
    outputs: &mut Vec<EditorOutput<NodeId, PortId, WireId, GroupId>>,
) where
    NodeId: Clone + Eq,
    PortId: Clone + Eq,
    WireId: Clone + Eq,
    GroupId: Clone + Eq,
    Key: Copy + Eq,
{
    let Some(gesture) = state.gesture.take() else {
        return;
    };
    match gesture {
        Gesture::Marquee {
            start,
            current,
            additive,
            transform,
        } if options.select && options.marquee => {
            finish_marquee(frame, start, current, additive, transform, outputs);
        }
        Gesture::Connect {
            from, transform, ..
        } if options.connect => {
            let Some(position) = pointer else {
                return;
            };
            let graph_position = transform.inverse() * position;
            let Some(source) = frame.ports.iter().find(|port| port.id == from) else {
                return;
            };
            let Some(target) = hit_port(frame, graph_position) else {
                return;
            };
            if source.id == target.id
                || source.direction == target.direction
                || source.type_key != target.type_key
                || !source.connectable
                || !target.connectable
            {
                return;
            }
            let (from, to) = match source.direction {
                PortDirection::Output => (source.id.clone(), target.id.clone()),
                PortDirection::Input => (target.id.clone(), source.id.clone()),
            };
            outputs.push(EditorOutput::Connect { from, to });
        }
        Gesture::Move {
            items, transform, ..
        } if options.reparent => {
            let Some(position) = pointer else {
                return;
            };
            let graph_position = transform.inverse() * position;
            let parent = deepest_group_at(frame, graph_position).map(|group| group.id.clone());
            let nodes = items
                .into_iter()
                .filter_map(|item| match item {
                    Movable::Node(node) => Some(node),
                    Movable::Group(_) => None,
                })
                .collect::<Vec<_>>();
            if !nodes.is_empty()
                && nodes.iter().any(|node_id| {
                    frame
                        .nodes
                        .iter()
                        .find(|node| node.id == *node_id)
                        .is_some_and(|node| node.parent != parent)
                })
            {
                outputs.push(EditorOutput::Reparent { nodes, parent });
            }
        }
        Gesture::LayoutSwipe(gesture) => {
            let phase = if gesture.is_activated() {
                LayoutSwipePhase::Commit
            } else {
                LayoutSwipePhase::Cancel
            };
            outputs.push(EditorOutput::LayoutSwipe(gesture.finish(phase, pointer)));
        }
        Gesture::Marquee { .. }
        | Gesture::Connect { .. }
        | Gesture::Move { .. }
        | Gesture::Resize { .. } => {}
    }
}

fn finish_marquee<NodeId, PortId, WireId, GroupId, Key>(
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    start: Pos2,
    current: Pos2,
    additive: bool,
    transform: egui::emath::TSTransform,
    outputs: &mut Vec<EditorOutput<NodeId, PortId, WireId, GroupId>>,
) where
    NodeId: Clone + Eq,
    WireId: Clone + Eq,
    GroupId: Clone + Eq,
{
    if start.distance(current) < MARQUEE_DRAG_THRESHOLD {
        if !additive {
            clear_selection(frame, outputs);
        }
        return;
    }

    let screen_rect = Rect::from_two_pos(start, current);
    let hits = frame
        .selection_order
        .iter()
        .filter(|item| {
            selectable_rect(frame, item)
                .is_some_and(|rect| screen_rect.intersects(transform * rect))
        })
        .cloned()
        .collect::<Vec<_>>();
    let current = frame
        .selection
        .items
        .iter()
        .filter(|item| !matches!(item, ItemId::Wire(_)))
        .cloned()
        .collect::<Vec<_>>();
    let (items, primary) = after_marquee(&current, &hits, additive);
    outputs.push(EditorOutput::Select { items, primary });
    outputs.extend(frame.selection.items.iter().filter_map(|item| match item {
        ItemId::Wire(wire) => Some(EditorOutput::DeselectWire { wire: wire.clone() }),
        ItemId::Node(_) | ItemId::Group(_) => None,
    }));
}

fn paint_transient<NodeId, PortId, WireId, GroupId, Key>(
    ui: &egui::Ui,
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    state: &InteractionState<NodeId, PortId, WireId, GroupId>,
) where
    PortId: Eq,
{
    let painter = ui.painter().with_clip_rect(frame.viewport);
    match state.gesture.as_ref() {
        Some(Gesture::Marquee { start, current, .. }) => {
            painter.rect(
                Rect::from_two_pos(*start, *current).intersect(frame.viewport),
                0.0,
                egui::Color32::from_rgba_premultiplied(76, 146, 255, 30),
                egui::Stroke::new(1.0, egui::Color32::from_rgb(105, 165, 255)),
                egui::StrokeKind::Inside,
            );
        }
        Some(Gesture::Connect {
            from,
            current,
            transform,
        }) => {
            if let Some(port) = frame.ports.iter().find(|port| port.id == *from) {
                painter.line_segment(
                    [*transform * port.center, *current],
                    egui::Stroke::new(2.0, egui::Color32::from_rgb(110, 174, 255)),
                );
            }
        }
        Some(Gesture::Move { .. })
        | Some(Gesture::Resize { .. })
        | Some(Gesture::LayoutSwipe(_))
        | None => {}
    }
}

fn select_non_wire<NodeId, PortId, WireId, GroupId, Key>(
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    clicked: ItemId<NodeId, GroupId, WireId>,
    additive: bool,
    outputs: &mut Vec<EditorOutput<NodeId, PortId, WireId, GroupId>>,
) where
    NodeId: Clone + Eq,
    WireId: Clone + Eq,
    GroupId: Clone + Eq,
{
    let current = frame
        .selection
        .items
        .iter()
        .filter(|item| !matches!(item, ItemId::Wire(_)))
        .cloned()
        .collect::<Vec<_>>();
    let primary = frame
        .selection
        .primary
        .as_ref()
        .filter(|item| !matches!(item, ItemId::Wire(_)))
        .cloned();
    let (items, primary) = after_click(&current, primary, clicked, additive);
    outputs.push(EditorOutput::Select { items, primary });
    outputs.extend(frame.selection.items.iter().filter_map(|item| match item {
        ItemId::Wire(wire) => Some(EditorOutput::DeselectWire { wire: wire.clone() }),
        ItemId::Node(_) | ItemId::Group(_) => None,
    }));
}

fn clear_selection<NodeId, PortId, WireId, GroupId, Key>(
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    outputs: &mut Vec<EditorOutput<NodeId, PortId, WireId, GroupId>>,
) where
    WireId: Clone,
{
    outputs.push(EditorOutput::Select {
        items: Vec::new(),
        primary: None,
    });
    outputs.extend(frame.selection.items.iter().filter_map(|item| match item {
        ItemId::Wire(wire) => Some(EditorOutput::DeselectWire { wire: wire.clone() }),
        ItemId::Node(_) | ItemId::Group(_) => None,
    }));
}

fn movable_selection<NodeId, PortId, WireId, GroupId, Key>(
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    clicked: ItemId<NodeId, GroupId, WireId>,
) -> Vec<Movable<NodeId, GroupId>>
where
    NodeId: Clone + Eq,
    WireId: Eq,
    GroupId: Clone + Eq,
{
    if !frame.selection.items.contains(&clicked) {
        return match clicked {
            ItemId::Node(node) => vec![Movable::Node(node)],
            ItemId::Group(group) => vec![Movable::Group(group)],
            ItemId::Wire(_) => Vec::new(),
        };
    }
    frame
        .selection
        .items
        .iter()
        .filter_map(|item| match item {
            ItemId::Node(node) => Some(Movable::Node(node.clone())),
            ItemId::Group(group) => Some(Movable::Group(group.clone())),
            ItemId::Wire(_) => None,
        })
        .collect()
}

fn hit_selectable<NodeId, PortId, WireId, GroupId, Key>(
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    position: Pos2,
) -> Option<ItemId<NodeId, GroupId, WireId>>
where
    NodeId: Clone + Eq,
    WireId: Clone,
    GroupId: Clone + Eq,
{
    frame
        .selection_order
        .iter()
        .rev()
        .find(|item| selectable_rect(frame, item).is_some_and(|rect| rect.contains(position)))
        .cloned()
}

fn selectable_rect<NodeId, PortId, WireId, GroupId, Key>(
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    item: &ItemId<NodeId, GroupId, WireId>,
) -> Option<Rect>
where
    NodeId: Eq,
    GroupId: Eq,
{
    match item {
        ItemId::Node(node_id) => frame
            .nodes
            .iter()
            .find(|node| node.id == *node_id)
            .map(|node| node.rect),
        ItemId::Group(group_id) => frame
            .groups
            .iter()
            .find(|group| group.id == *group_id)
            .map(|group| group.header_rect),
        ItemId::Wire(_) => None,
    }
}

fn deepest_group_at<'a, NodeId, PortId, WireId, GroupId, Key>(
    frame: &'a GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    position: Pos2,
) -> Option<&'a crate::GroupDescriptor<'a, GroupId>> {
    frame
        .groups
        .iter()
        .filter(|group| group.rect.contains(position))
        .min_by(|left, right| {
            let left_area = left.rect.width() * left.rect.height();
            let right_area = right.rect.width() * right.rect.height();
            left_area.total_cmp(&right_area)
        })
}

fn hit_group_resize<'a, NodeId, PortId, WireId, GroupId, Key>(
    frame: &'a GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    position: Pos2,
) -> Option<&'a crate::GroupDescriptor<'a, GroupId>> {
    let scale = frame.transform.scaling.abs().max(f32::EPSILON);
    let width = GROUP_RESIZE_HIT_WIDTH / scale;
    frame.groups.iter().rev().find(|group| {
        if !group.resizable || !group.rect.expand(width).contains(position) {
            return false;
        }
        (position.x - group.rect.right()).abs() <= width
            || (position.y - group.rect.bottom()).abs() <= width
    })
}

fn hit_port<'a, NodeId, PortId, WireId, GroupId, Key>(
    frame: &'a GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    position: Pos2,
) -> Option<&'a crate::PortDescriptor<'a, NodeId, PortId, GroupId, Key>> {
    let scale = frame.transform.scaling.abs().max(f32::EPSILON);
    let radius = PORT_HIT_RADIUS / scale;
    frame
        .ports
        .iter()
        .rev()
        .find(|port| port.connectable && port.center.distance(position) <= radius)
}

fn hit_wire<'a, NodeId, PortId, WireId, GroupId, Key>(
    frame: &'a GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    position: Pos2,
) -> Option<&'a WireDescriptor<PortId, WireId>> {
    let scale = frame.transform.scaling.abs().max(f32::EPSILON);
    let radius = WIRE_HIT_RADIUS / scale;
    frame
        .wires
        .iter()
        .rev()
        .find(|wire| wire.curve.distance_to(position) <= radius)
}
