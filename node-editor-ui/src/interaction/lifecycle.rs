use egui::{Pos2, Rect, Vec2};

use crate::input::interaction_input;
use crate::layout_swipe::hit_anchor as hit_layout_swipe_anchor;
use crate::{after_click, GraphFrame, ItemId, LayoutSwipeHitArea, LayoutSwipePhase, PortDirection};

use super::{
    hit, keyboard, selection, transient, EditorOutput, Gesture, InteractionOptions,
    InteractionState, Movable, MoveEndOutcome,
};

const MOVE_DRAG_THRESHOLD: f32 = 4.0;
const MIN_GROUP_SIZE: Vec2 = Vec2::new(80.0, 48.0);

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
    let input = interaction_input(ui);
    let mut outputs = Vec::new();
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

    transient::paint(ui, frame, state);

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
        Some(Gesture::Connect { .. }) => !options.connect,
        Some(Gesture::Resize { .. }) => !options.resize_groups,
        Some(Gesture::LayoutSwipe(_)) => options.layout_swipe == LayoutSwipeHitArea::Disabled,
        None => false,
    };
    if disabled {
        cancel_gesture(state, current, outputs);
    }
}

fn cancel_gesture<NodeId, PortId, WireId, GroupId>(
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
            | Gesture::Resize { .. },
        )
        | None => {}
    }
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
        Gesture::Marquee { current, .. } | Gesture::Connect { current, .. } => {
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
        | Gesture::Move { .. }
        | Gesture::Resize { .. } => {}
    }
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
