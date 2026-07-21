use crate::state::context_types::{SelectionState, SelectionTarget};
use crate::ui::panels::node_editor::GraphItem;
use eframe::egui;
use egui_snarl::Snarl;
use library::model::project::PortOwner;
use uuid::Uuid;

pub(in crate::ui::panels::node_editor) fn captured_snarl_drag_node(
    context: &egui::Context,
    snarl: &Snarl<GraphItem>,
    snarl_id: egui::Id,
) -> Option<Uuid> {
    captured_snarl_drag_target(context, snarl, snarl_id)?.node_id()
}

/// Convert Snarl's transient frame ID to the one authoritative app identity.
/// Selection policy itself lives in `node-editor-ui` and consumes the same
/// borrowed `GraphFrame` as the reusable renderer.
pub(in crate::ui::panels::node_editor) fn captured_snarl_drag_target(
    context: &egui::Context,
    snarl: &Snarl<GraphItem>,
    snarl_id: egui::Id,
) -> Option<SelectionTarget> {
    let dragged_id = context.dragged_id()?;
    snarl.node_ids().find_map(|(node_id, item)| {
        if snarl_id.with(("snarl-node", node_id)).with("frame") != dragged_id {
            return None;
        }
        Some(match *item {
            GraphItem::Node(id) => SelectionTarget::Node(id),
            GraphItem::Container(owner) | GraphItem::PortAnchor { owner, .. } => {
                selection_target_for_owner(owner)
            }
        })
    })
}

pub(in crate::ui::panels::node_editor) const fn selection_target_for_owner(
    owner: PortOwner,
) -> SelectionTarget {
    match owner {
        PortOwner::Node(id) => SelectionTarget::Node(id),
        PortOwner::Clip(id) => SelectionTarget::Clip(id),
        PortOwner::Track(id) => SelectionTarget::Track(id),
        PortOwner::Composition(id) => SelectionTarget::Composition(id),
    }
}

pub(in crate::ui::panels::node_editor) fn select_logical_item(
    selection: &mut SelectionState,
    clicked: SelectionTarget,
    additive: bool,
) -> bool {
    let (targets, primary) =
        node_editor_ui::after_click(selection.targets(), selection.primary(), clicked, additive);
    if selection.targets() == targets && selection.primary() == primary {
        return false;
    }
    selection.replace(targets, primary);
    true
}

#[cfg(test)]
pub(in crate::ui::panels::node_editor) fn node_selection_after_snarl_click(
    current_targets: &[SelectionTarget],
    current_primary: Option<SelectionTarget>,
    snarl_selected_node_ids: &[Uuid],
    clicked_node_id: Uuid,
    modifiers: egui::Modifiers,
) -> (Vec<SelectionTarget>, Option<SelectionTarget>) {
    // egui-snarl applies Shift/Cmd selection changes before `Snarl::show`
    // returns. Cmd without Shift is its deselect gesture. In that case the
    // clicked Node is intentionally absent from the post-show snapshot.
    if modifiers.shift {
        let mut targets = current_targets
            .iter()
            .copied()
            .filter(|target| !matches!(target, SelectionTarget::Node(_)))
            .collect::<Vec<_>>();
        for node_id in snarl_selected_node_ids {
            let target = SelectionTarget::Node(*node_id);
            if !targets.contains(&target) {
                targets.push(target);
            }
        }
        let clicked = SelectionTarget::Node(clicked_node_id);
        if !targets.contains(&clicked) {
            targets.push(clicked);
        }
        return (targets, Some(clicked));
    }

    if modifiers.command {
        let clicked = SelectionTarget::Node(clicked_node_id);
        let targets = current_targets
            .iter()
            .copied()
            .filter(|target| *target != clicked)
            .collect::<Vec<_>>();
        let primary = current_primary
            .filter(|target| targets.contains(target))
            .or_else(|| targets.last().copied());
        return (targets, primary);
    }

    if snarl_selected_node_ids.contains(&clicked_node_id) {
        let targets = snarl_selected_node_ids
            .iter()
            .copied()
            .map(SelectionTarget::Node)
            .collect::<Vec<_>>();
        return (targets, Some(SelectionTarget::Node(clicked_node_id)));
    }

    let target = SelectionTarget::Node(clicked_node_id);
    (vec![target], Some(target))
}
