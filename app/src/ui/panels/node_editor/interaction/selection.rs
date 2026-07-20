use crate::state::context_types::SelectionTarget;
use eframe::egui;
use uuid::Uuid;

pub(in crate::ui::panels::node_editor) fn node_selection_after_snarl_click(
    current_targets: &[SelectionTarget],
    current_primary: Option<SelectionTarget>,
    snarl_selected_node_ids: &[Uuid],
    clicked_node_id: Uuid,
    modifiers: egui::Modifiers,
) -> (Vec<SelectionTarget>, Option<SelectionTarget>) {
    // egui-snarl applies Shift/Cmd selection changes before `Snarl::show`
    // returns. Cmd without Shift is its deselect gesture. In that case the
    // clicked Node is intentionally absent from the post-show snapshot; do
    // not mistake that absence for an update race and select it again.
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
