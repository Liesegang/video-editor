use egui::{Pos2, Rect, Vec2};

use crate::input::interaction_input;
use crate::layout_swipe::hit_anchor as hit_layout_swipe_anchor;
use crate::wire::ReconnectEndpoint;
use crate::{
    after_click, GraphFrame, ItemId, LayoutSwipeHitArea, LayoutSwipePhase, PortDirection, PortOwner,
};

use super::{
    hit, keyboard, selection, transient, EditorOutput, Gesture, InteractionOptions,
    InteractionState, Movable, MoveEndOutcome,
};

const MOVE_DRAG_THRESHOLD: f32 = 4.0;
const CUT_SAMPLE_DISTANCE: f32 = 3.0;
const CUT_WIRE_TOLERANCE: f32 = 3.0;
const MIN_GROUP_SIZE: Vec2 = Vec2::new(80.0, 48.0);

pub(crate) fn interact<NodeId, PortId, WireId, GroupId, Key>(
    ui: &mut egui::Ui,
    overlay_painter: &egui::Painter,
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
    let input = interaction_input(ui);
    let mut outputs = Vec::new();
    // Node controls may open a popup outside their own body rectangle. Raw
    // graph hit testing must not see through that popup, even when the color
    // slider is geometrically over another Node or the pointer leaves it.
    if egui::Popup::is_any_open(ui.ctx()) {
        cancel_gesture(state, input.pointer, &mut outputs);
        return outputs;
    }
    let layout_swipe_was_active = state.is_layout_swipe_active();
    cancel_disabled_gesture(state, options, input.pointer, &mut outputs);
    if layout_swipe_was_active && !state.is_layout_swipe_active() {
        return outputs;
    }
    if !input.focused {
        cancel_gesture(state, input.pointer, &mut outputs);
        return outputs;
    }
    if input.escape && state.is_layout_swipe_active() {
        cancel_gesture(state, input.pointer, &mut outputs);
        return outputs;
    }

    outputs.extend(keyboard::outputs(ui, frame, state, options));
    if input.escape {
        return outputs;
    }

    let wants_keyboard = ui.ctx().wants_keyboard_input();
    let navigation_owns_pointer = input.space_down || input.middle_down;
    let a_held_through_release =
        input.a_down || (input.released && input.pointer_released_before_a);
    let layout_conflict = wants_keyboard
        || navigation_owns_pointer
        || options.layout_swipe == LayoutSwipeHitArea::Disabled
        || !a_held_through_release;
    if state.is_layout_swipe_active() && layout_conflict {
        cancel_gesture(state, input.pointer, &mut outputs);
        return outputs;
    }

    if state.gesture.is_none()
        && input.secondary_pressed
        && !pointer_blocked
        && input
            .secondary_press_position
            .is_some_and(|position| frame.viewport.contains(position))
    {
        let Some(screen_position) = input.secondary_press_position else {
            return outputs;
        };
        begin_secondary(
            ui,
            frame,
            state,
            options,
            frame.graph_position(screen_position),
            screen_position,
            input.secondary_press_modifiers,
        );
    }

    if is_secondary_gesture(state) {
        if let Some(position) = input.pointer {
            update_secondary(
                state,
                position,
                input.secondary_down || input.secondary_released,
            );
        }
        transient::paint(overlay_painter, frame, state);
        if input.secondary_released {
            finish_secondary(frame, state, options, input.pointer, &mut outputs);
        } else if !input.secondary_pressed && (!input.secondary_down || !input.has_pointer) {
            cancel_gesture(state, input.pointer, &mut outputs);
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
        begin(
            ui,
            frame,
            state,
            options,
            frame.graph_position(screen_position),
            screen_position,
            input.press_modifiers,
            input.a_down_at_press && !wants_keyboard,
            &mut outputs,
        );
    }

    if state.is_layout_swipe_active() && !a_held_through_release {
        cancel_gesture(state, input.pointer, &mut outputs);
        return outputs;
    }

    if let Some(position) = input.pointer {
        update(
            state,
            options,
            position,
            input.down || input.released,
            &mut outputs,
        );
    }

    transient::paint(overlay_painter, frame, state);

    if input.released {
        finish(frame, state, options, input.pointer, &mut outputs);
    } else if !input.pressed && (!input.down || !input.has_pointer) {
        cancel_gesture(state, input.pointer, &mut outputs);
    }

    outputs
}

fn cancel_disabled_gesture<NodeId, PortId, WireId, GroupId>(
    state: &mut InteractionState<NodeId, PortId, WireId, GroupId>,
    options: InteractionOptions,
    current: Option<Pos2>,
    outputs: &mut Vec<EditorOutput<NodeId, PortId, WireId, GroupId>>,
) where
    NodeId: Clone,
{
    let disabled = match state.gesture.as_ref() {
        Some(Gesture::Hold { .. } | Gesture::Move { .. }) => !options.move_items,
        Some(Gesture::Marquee { .. }) => !options.select || !options.marquee,
        Some(Gesture::Connect { .. } | Gesture::Reconnect { .. }) => !options.connect,
        Some(Gesture::LazyConnect { .. }) => !options.connect,
        Some(Gesture::WireSecondary { .. } | Gesture::CutWires { .. }) => !options.disconnect,
        Some(Gesture::Resize { .. }) => !options.resize_groups,
        Some(Gesture::LayoutSwipe(_)) => options.layout_swipe == LayoutSwipeHitArea::Disabled,
        None => false,
    };
    if disabled {
        cancel_gesture(state, current, outputs);
    }
}

pub(crate) fn cancel_gesture<NodeId, PortId, WireId, GroupId>(
    state: &mut InteractionState<NodeId, PortId, WireId, GroupId>,
    current: Option<Pos2>,
    outputs: &mut Vec<EditorOutput<NodeId, PortId, WireId, GroupId>>,
) where
    NodeId: Clone,
{
    match state.gesture.take() {
        Some(Gesture::Move { started: true, .. }) => outputs.push(EditorOutput::MoveEnd {
            outcome: MoveEndOutcome::Cancelled,
        }),
        Some(Gesture::LayoutSwipe(gesture)) => {
            outputs.push(EditorOutput::LayoutSwipe(
                gesture.finish(LayoutSwipePhase::Cancel, current),
            ));
        }
        Some(
            Gesture::Hold { .. }
            | Gesture::Marquee { .. }
            | Gesture::Move { .. }
            | Gesture::Connect { .. }
            | Gesture::Reconnect { .. }
            | Gesture::WireSecondary { .. }
            | Gesture::CutWires { .. }
            | Gesture::LazyConnect { .. }
            | Gesture::Resize { .. },
        )
        | None => {}
    }
}

fn is_secondary_gesture<NodeId, PortId, WireId, GroupId>(
    state: &InteractionState<NodeId, PortId, WireId, GroupId>,
) -> bool {
    matches!(
        state.gesture,
        Some(
            Gesture::WireSecondary { .. } | Gesture::CutWires { .. } | Gesture::LazyConnect { .. }
        )
    )
}

fn begin_secondary<NodeId, PortId, WireId, GroupId, Key>(
    ui: &egui::Ui,
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    state: &mut InteractionState<NodeId, PortId, WireId, GroupId>,
    options: InteractionOptions,
    graph_position: Pos2,
    screen_position: Pos2,
    modifiers: egui::Modifiers,
) where
    NodeId: Clone + Eq,
    WireId: Clone,
{
    if modifiers.ctrl && options.disconnect {
        state.gesture = Some(Gesture::CutWires {
            points: vec![screen_position],
            transform: frame.transform,
        });
        capture_pointer(ui);
        return;
    }
    if modifiers.alt && options.connect {
        if let Some(node) = hit::node(frame, graph_position) {
            state.gesture = Some(Gesture::LazyConnect {
                from_node: node.clone(),
                start: screen_position,
                current: screen_position,
                transform: frame.transform,
            });
            capture_pointer(ui);
        }
        return;
    }
    if hit::node(frame, graph_position).is_some() || hit::port(frame, graph_position).is_some() {
        return;
    }
    if options.disconnect {
        if let Some(wire) = hit::wire(frame, graph_position).filter(|wire| wire.editable) {
            state.gesture = Some(Gesture::WireSecondary {
                wire: wire.id.clone(),
                start: screen_position,
                current: screen_position,
                transform: frame.transform,
            });
            capture_pointer(ui);
        }
    }
}

fn update_secondary<NodeId, PortId, WireId, GroupId>(
    state: &mut InteractionState<NodeId, PortId, WireId, GroupId>,
    screen_position: Pos2,
    pointer_active: bool,
) {
    if !pointer_active {
        return;
    }
    match state.gesture.as_mut() {
        Some(Gesture::WireSecondary { current, .. } | Gesture::LazyConnect { current, .. }) => {
            *current = screen_position
        }
        Some(Gesture::CutWires { points, .. }) => {
            if points
                .last()
                .is_none_or(|point| point.distance(screen_position) >= CUT_SAMPLE_DISTANCE)
            {
                points.push(screen_position);
            }
        }
        Some(
            Gesture::Hold { .. }
            | Gesture::Marquee { .. }
            | Gesture::Move { .. }
            | Gesture::Connect { .. }
            | Gesture::Reconnect { .. }
            | Gesture::Resize { .. }
            | Gesture::LayoutSwipe(_),
        )
        | None => {}
    }
}

fn finish_secondary<NodeId, PortId, WireId, GroupId, Key>(
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    state: &mut InteractionState<NodeId, PortId, WireId, GroupId>,
    options: InteractionOptions,
    pointer: Option<Pos2>,
    outputs: &mut Vec<EditorOutput<NodeId, PortId, WireId, GroupId>>,
) where
    NodeId: Clone + Eq,
    PortId: Clone + Eq,
    WireId: Clone + Eq,
    GroupId: Eq,
    Key: Copy + Eq,
{
    let Some(gesture) = state.gesture.take() else {
        return;
    };
    match gesture {
        Gesture::WireSecondary {
            wire,
            start,
            current,
            ..
        } if options.disconnect && start.distance(current) < MOVE_DRAG_THRESHOLD => {
            outputs.push(EditorOutput::WireContextMenu {
                wire,
                screen_position: current,
            });
        }
        Gesture::CutWires {
            mut points,
            transform,
        } if options.disconnect => {
            if let Some(pointer) = pointer {
                points.push(pointer);
            }
            let travel = points
                .windows(2)
                .map(|segment| segment[0].distance(segment[1]))
                .sum::<f32>();
            if travel < MOVE_DRAG_THRESHOLD {
                return;
            }
            let scale = transform.scaling.abs().max(f32::EPSILON);
            for wire in frame.wires.iter().filter(|wire| wire.editable) {
                let crossed = points.windows(2).any(|segment| {
                    wire.curve.intersects_segment(
                        transform.inverse() * segment[0],
                        transform.inverse() * segment[1],
                        CUT_WIRE_TOLERANCE / scale,
                    )
                });
                if crossed {
                    outputs.push(EditorOutput::Disconnect {
                        wire: wire.id.clone(),
                    });
                }
            }
        }
        Gesture::LazyConnect {
            from_node,
            transform,
            ..
        } if options.connect => {
            let Some(pointer) = pointer else {
                return;
            };
            let Some(to_node) = hit::node(frame, transform.inverse() * pointer) else {
                return;
            };
            finish_lazy_connect(frame, &from_node, to_node, outputs);
        }
        Gesture::WireSecondary { .. } | Gesture::CutWires { .. } | Gesture::LazyConnect { .. } => {}
        Gesture::Hold { .. }
        | Gesture::Marquee { .. }
        | Gesture::Move { .. }
        | Gesture::Connect { .. }
        | Gesture::Reconnect { .. }
        | Gesture::Resize { .. }
        | Gesture::LayoutSwipe(_) => {}
    }
}

fn finish_lazy_connect<NodeId, PortId, WireId, GroupId, Key>(
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    from_node: &NodeId,
    to_node: &NodeId,
    outputs: &mut Vec<EditorOutput<NodeId, PortId, WireId, GroupId>>,
) where
    NodeId: Clone + Eq,
    PortId: Clone + Eq,
    GroupId: Eq,
    Key: Copy + Eq,
{
    if from_node == to_node {
        return;
    }
    let Some((from, to)) = compatible_node_port_ids(frame, from_node, to_node)
        .or_else(|| compatible_node_port_ids(frame, to_node, from_node))
    else {
        return;
    };
    outputs.push(EditorOutput::Connect { from, to });
}

fn compatible_node_port_ids<NodeId, PortId, WireId, GroupId, Key>(
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    output_node: &NodeId,
    input_node: &NodeId,
) -> Option<(PortId, PortId)>
where
    NodeId: Clone + Eq,
    PortId: Clone,
    GroupId: Eq,
{
    frame
        .ports
        .iter()
        .filter(|port| {
            port.owner == PortOwner::Node(output_node.clone())
                && port.direction == PortDirection::Output
                && port.connectable
        })
        .find_map(|source| {
            frame
                .ports
                .iter()
                .find(|target| {
                    target.owner == PortOwner::Node(input_node.clone())
                        && target.direction == PortDirection::Input
                        && target.connectable
                        && (frame.ports_compatible)(
                            source.type_key.value(),
                            target.type_key.value(),
                        )
                })
                .map(|target| (source.id.clone(), target.id.clone()))
        })
}

#[allow(
    clippy::too_many_arguments,
    reason = "the frame, input snapshot, and output sink are one immediate-mode gesture boundary"
)]
fn begin<NodeId, PortId, WireId, GroupId, Key>(
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
        if let Some((wire, endpoint)) = hit::reconnect_handle(frame, graph_position) {
            state.gesture = Some(Gesture::Reconnect {
                wire,
                endpoint,
                current: screen_position,
                transform: frame.transform,
            });
            capture_pointer(ui);
            return;
        }
    }

    if options.connect {
        if let Some(port) = hit::port(frame, graph_position) {
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
        if let Some(group) = hit::group_resize(frame, graph_position) {
            state.gesture = Some(Gesture::Resize {
                group: group.id.clone(),
                initial_rect: group.rect,
                start: graph_position,
                current: screen_position,
                transform: frame.transform,
            });
            capture_pointer(ui);
            if options.select {
                selection::select_non_wire(
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
            let gesture = crate::layout_swipe::LayoutSwipeState::new(
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

    if let Some(clicked) = hit::selectable(frame, graph_position) {
        if options.select {
            selection::select_non_wire(
                frame,
                clicked.clone(),
                modifiers.shift || modifiers.command,
                outputs,
            );
        }
        let movement_modifier = modifiers.shift || modifiers.command;
        if options.move_items
            && hit::move_handle(frame, &clicked, graph_position)
            && !movement_modifier
        {
            let grabbed = match clicked.clone() {
                ItemId::Node(node) => Movable::Node(node),
                ItemId::Group(group) => Movable::Group(group),
                ItemId::Wire(_) => return,
            };
            state.gesture = Some(Gesture::Move {
                items: selection::movable_snapshot(frame, clicked),
                grabbed,
                start_screen: screen_position,
                started: false,
                previous: graph_position,
                current: screen_position,
                transform: frame.transform,
            });
            capture_pointer(ui);
        } else if options.move_items && !movement_modifier && matches!(clicked, ItemId::Node(_)) {
            state.gesture = Some(Gesture::Hold {
                transform: frame.transform,
            });
            capture_pointer(ui);
        }
        return;
    }

    if options.select_wires || options.disconnect {
        if let Some(wire) = hit::wire(frame, graph_position) {
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
                    modifiers.shift || modifiers.command,
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
            selection::clear(frame, outputs);
        }
    }
}

fn capture_pointer(ui: &egui::Ui) {
    ui.ctx()
        .set_dragged_id(ui.make_persistent_id("node_editor_ui.interaction"));
}

fn update<NodeId, PortId, WireId, GroupId>(
    state: &mut InteractionState<NodeId, PortId, WireId, GroupId>,
    options: InteractionOptions,
    screen_position: Pos2,
    pointer_active: bool,
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
        Gesture::Hold { .. } => {}
        Gesture::Marquee { current, .. }
        | Gesture::Connect { current, .. }
        | Gesture::Reconnect { current, .. } => {
            *current = screen_position;
        }
        Gesture::Move {
            items,
            grabbed,
            start_screen,
            started,
            previous,
            current,
            transform,
        } => {
            *current = screen_position;
            if !pointer_active || !options.move_items {
                return;
            }
            if !*started && start_screen.distance(screen_position) < MOVE_DRAG_THRESHOLD {
                return;
            }
            *started = true;
            let graph_position = transform.inverse() * screen_position;
            let delta = graph_position - *previous;
            if delta != Vec2::ZERO {
                outputs.push(EditorOutput::Move {
                    items: items.iter().map(movable_item).collect(),
                    grabbed: movable_item(grabbed),
                    delta,
                });
                *previous = graph_position;
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
            if pointer_active && options.resize_groups {
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
            if pointer_active {
                if let Some(update) = gesture.update(screen_position) {
                    outputs.push(EditorOutput::LayoutSwipe(update));
                }
            }
        }
        Gesture::WireSecondary { .. } | Gesture::CutWires { .. } | Gesture::LazyConnect { .. } => {}
    }
}

fn movable_item<NodeId: Clone, WireId, GroupId: Clone>(
    item: &Movable<NodeId, GroupId>,
) -> ItemId<NodeId, GroupId, WireId> {
    match item {
        Movable::Node(node) => ItemId::Node(node.clone()),
        Movable::Group(group) => ItemId::Group(group.clone()),
    }
}

fn finish<NodeId, PortId, WireId, GroupId, Key>(
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
            selection::finish_marquee(frame, start, current, additive, transform, outputs);
        }
        Gesture::Connect {
            from, transform, ..
        } if options.connect => finish_connect(frame, from, transform, pointer, outputs),
        Gesture::Reconnect {
            wire,
            endpoint,
            transform,
            ..
        } if options.connect => {
            finish_reconnect(frame, wire, endpoint, transform, pointer, outputs);
        }
        Gesture::Move {
            items,
            transform,
            started: true,
            ..
        } => {
            if options.reparent {
                finish_reparent(frame, items, transform, pointer, outputs);
            }
            outputs.push(EditorOutput::MoveEnd {
                outcome: MoveEndOutcome::Released,
            });
        }
        Gesture::LayoutSwipe(gesture) => {
            let phase = if gesture.is_activated() {
                LayoutSwipePhase::Commit
            } else {
                LayoutSwipePhase::Cancel
            };
            outputs.push(EditorOutput::LayoutSwipe(gesture.finish(phase, pointer)));
        }
        Gesture::Hold { .. }
        | Gesture::Marquee { .. }
        | Gesture::Connect { .. }
        | Gesture::Reconnect { .. }
        | Gesture::WireSecondary { .. }
        | Gesture::CutWires { .. }
        | Gesture::LazyConnect { .. }
        | Gesture::Move { .. }
        | Gesture::Resize { .. } => {}
    }
}

fn finish_reconnect<NodeId, PortId, WireId, GroupId, Key>(
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    wire_id: WireId,
    endpoint: ReconnectEndpoint,
    transform: egui::emath::TSTransform,
    pointer: Option<Pos2>,
    outputs: &mut Vec<EditorOutput<NodeId, PortId, WireId, GroupId>>,
) where
    PortId: Clone + Eq,
    WireId: Clone + Eq,
    Key: Copy + Eq,
{
    let Some(wire) = frame.wires.iter().find(|wire| wire.id == wire_id) else {
        return;
    };
    let Some(position) = pointer else {
        return;
    };
    let graph_position = transform.inverse() * position;
    let Some(candidate) = hit::port(frame, graph_position) else {
        return;
    };
    let (fixed_id, wanted_direction) = match endpoint {
        ReconnectEndpoint::Source => (&wire.to, PortDirection::Output),
        ReconnectEndpoint::Target => (&wire.from, PortDirection::Input),
    };
    let Some(fixed) = frame.ports.iter().find(|port| port.id == *fixed_id) else {
        return;
    };
    let types_compatible = match endpoint {
        ReconnectEndpoint::Source => {
            (frame.ports_compatible)(candidate.type_key.value(), fixed.type_key.value())
        }
        ReconnectEndpoint::Target => {
            (frame.ports_compatible)(fixed.type_key.value(), candidate.type_key.value())
        }
    };
    if candidate.direction != wanted_direction
        || candidate.id == fixed.id
        || !types_compatible
        || !candidate.connectable
        || !fixed.connectable
    {
        return;
    }
    let (from, to) = match endpoint {
        ReconnectEndpoint::Source => (candidate.id.clone(), fixed.id.clone()),
        ReconnectEndpoint::Target => (fixed.id.clone(), candidate.id.clone()),
    };
    if from == wire.from && to == wire.to {
        return;
    }
    outputs.push(EditorOutput::Reconnect {
        wire: wire.id.clone(),
        from,
        to,
    });
}

fn finish_connect<NodeId, PortId, WireId, GroupId, Key>(
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    from: PortId,
    transform: egui::emath::TSTransform,
    pointer: Option<Pos2>,
    outputs: &mut Vec<EditorOutput<NodeId, PortId, WireId, GroupId>>,
) where
    PortId: Clone + Eq,
    Key: Copy + Eq,
{
    let Some(position) = pointer else {
        return;
    };
    let graph_position = transform.inverse() * position;
    let Some(source) = frame.ports.iter().find(|port| port.id == from) else {
        return;
    };
    let Some(target) = hit::port(frame, graph_position) else {
        return;
    };
    if source.id == target.id || source.direction == target.direction {
        return;
    }
    let (output, input) = match source.direction {
        PortDirection::Output => (source, target),
        PortDirection::Input => (target, source),
    };
    if !output.connectable
        || !input.connectable
        || !(frame.ports_compatible)(output.type_key.value(), input.type_key.value())
    {
        return;
    }
    outputs.push(EditorOutput::Connect {
        from: output.id.clone(),
        to: input.id.clone(),
    });
}

fn finish_reparent<NodeId, PortId, WireId, GroupId, Key>(
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    items: Vec<Movable<NodeId, GroupId>>,
    transform: egui::emath::TSTransform,
    pointer: Option<Pos2>,
    outputs: &mut Vec<EditorOutput<NodeId, PortId, WireId, GroupId>>,
) where
    NodeId: Clone + Eq,
    GroupId: Clone + Eq,
{
    let Some(position) = pointer else {
        return;
    };
    let graph_position = transform.inverse() * position;
    let parent = hit::deepest_group(frame, graph_position).map(|group| group.id.clone());
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
