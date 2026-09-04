use crate::state::context_types::{SelectionState, SelectionTarget};
use library::model::project::PortOwner;

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

pub(in crate::ui::panels::node_editor) fn selected_container_owners(
    selection: &SelectionState,
) -> Vec<PortOwner> {
    selection
        .targets()
        .iter()
        .filter_map(|target| match *target {
            SelectionTarget::Composition(id) => Some(PortOwner::Composition(id)),
            SelectionTarget::Track(id) => Some(PortOwner::Track(id)),
            SelectionTarget::Clip(id) => Some(PortOwner::Clip(id)),
            SelectionTarget::Node(_) | SelectionTarget::TimelineItem(_) => None,
        })
        .collect()
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
mod container_selection_tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn composition_track_and_clip_project_to_independent_group_owners() {
        let composition = Uuid::from_u128(1);
        let track = Uuid::from_u128(2);
        let clip = Uuid::from_u128(3);
        let node = Uuid::from_u128(4);
        let mut selection = SelectionState::default();
        selection.replace(
            [
                SelectionTarget::Composition(composition),
                SelectionTarget::Track(track),
                SelectionTarget::Clip(clip),
                SelectionTarget::Node(node),
            ],
            Some(SelectionTarget::Node(node)),
        );

        assert_eq!(
            selected_container_owners(&selection),
            [
                PortOwner::Composition(composition),
                PortOwner::Track(track),
                PortOwner::Clip(clip),
            ]
        );
    }

    #[test]
    fn node_selection_never_implicitly_highlights_its_parent_group() {
        let mut selection = SelectionState::default();
        selection.replace(
            [SelectionTarget::Node(Uuid::from_u128(5))],
            Some(SelectionTarget::Node(Uuid::from_u128(5))),
        );
        assert!(selected_container_owners(&selection).is_empty());
    }
}
