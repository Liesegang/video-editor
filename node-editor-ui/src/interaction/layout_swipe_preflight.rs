//! Read-only ownership preflight for incrementally migrated host layers.

use crate::input::interaction_input;
use crate::{GraphFrame, LayoutSwipeHitArea};

use super::{hit_group_resize, hit_port, InteractionOptions, InteractionState};

/// Whether hold-A layout owns this frame's primary press.
///
/// A Node target wins over a crossing wire. Explicit ports and group resize
/// edges retain their higher priority when those interactions are enabled (or
/// when the host reports them through `pointer_blocked`).
pub(crate) fn layout_swipe_wants_pointer<NodeId, PortId, WireId, GroupId, Key>(
    ui: &egui::Ui,
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    state: &InteractionState<NodeId, PortId, WireId, GroupId>,
    options: InteractionOptions,
    pointer_blocked: bool,
) -> bool
where
    NodeId: Eq,
    GroupId: Eq,
{
    if state.is_layout_swipe_active() {
        return true;
    }
    if state.gesture.is_some()
        || pointer_blocked
        || options.layout_swipe == LayoutSwipeHitArea::Disabled
    {
        return false;
    }
    let input = interaction_input(ui);
    if !input.focused
        || ui.ctx().wants_keyboard_input()
        || input.space_down
        || input.middle_down
        || !input.pressed
        || !input.a_down_at_press
    {
        return false;
    }
    let Some(screen_position) = input
        .press_position
        .filter(|position| frame.viewport.contains(*position))
    else {
        return false;
    };
    let graph_position = frame.graph_position(screen_position);
    if options.connect && hit_port(frame, graph_position).is_some() {
        return false;
    }
    if options.resize_groups && hit_group_resize(frame, graph_position).is_some() {
        return false;
    }
    crate::layout_swipe::hit_anchor(frame, graph_position, options.layout_swipe).is_some()
}
