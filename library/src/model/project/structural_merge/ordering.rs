use std::collections::HashSet;

use uuid::Uuid;

use super::super::{NodeContainer, PortAddress, Project};
use super::contract::StructuralMergeKind;

impl Project {
    pub(in crate::model::project) fn reorder_structural_children(
        &mut self,
        container: NodeContainer,
        inserted_connection_id: Option<Uuid>,
    ) {
        for kind in StructuralMergeKind::ALL {
            self.reorder_structural_children_for(container, inserted_connection_id, kind);
        }
    }

    pub(super) fn reorder_structural_children_for(
        &mut self,
        container: NodeContainer,
        _inserted_connection_id: Option<Uuid>,
        kind: StructuralMergeKind,
    ) {
        let Some(target) = self.structural_merge_target_for(container, kind) else {
            return;
        };
        let children = self.structural_child_owners(container);
        let mut ordered_ids = self
            .connections
            .iter()
            .filter(|connection| connection.to == target)
            .map(|connection| (connection.order, connection.id))
            .collect::<Vec<_>>();
        ordered_ids.sort_by_key(|(order, id)| (*order, *id));
        let mut ordered_ids = ordered_ids
            .into_iter()
            .map(|(_, id)| id)
            .collect::<Vec<_>>();
        let desired_ids = children
            .iter()
            .filter_map(|child| self.structural_connection_id(&target, *child))
            .collect::<Vec<_>>();
        let desired_set = desired_ids.iter().copied().collect::<HashSet<_>>();
        ordered_ids.retain(|id| !desired_set.contains(id));
        let mut canonical_ids = desired_ids;
        canonical_ids.extend(ordered_ids);
        self.assign_target_orders(&canonical_ids);
    }

    pub(in crate::model::project) fn sync_child_order_from_structural_target(
        &mut self,
        target: &PortAddress,
    ) {
        let Some(container) = self.container_for_structural_target(target) else {
            return;
        };
        let current_children = self.structural_child_owners(container);
        let child_set = current_children.iter().copied().collect::<HashSet<_>>();
        let mut ordered = self
            .connections
            .iter()
            .filter(|connection| {
                connection.to == *target && child_set.contains(&connection.from.owner)
            })
            .map(|connection| (connection.order, connection.id, connection.from.owner))
            .collect::<Vec<_>>();
        ordered.sort_by_key(|(order, id, _)| (*order, *id));
        let desired = ordered
            .into_iter()
            .map(|(_, _, owner)| owner.id())
            .collect::<Vec<_>>();
        let connected = desired.iter().copied().collect::<HashSet<_>>();
        let slots = current_children
            .iter()
            .enumerate()
            .filter_map(|(index, owner)| connected.contains(&owner.id()).then_some(index))
            .collect::<Vec<_>>();
        if slots.len() != desired.len() {
            return;
        }
        match container {
            NodeContainer::Composition(id) => {
                if let Some(composition) = self.get_composition_mut(id) {
                    for (slot, child_id) in slots.into_iter().zip(desired) {
                        composition.track_ids[slot] = child_id;
                    }
                }
            }
            NodeContainer::Track(id) => {
                if let Some(track) = self.get_track_mut(id) {
                    for (slot, child_id) in slots.into_iter().zip(desired) {
                        track.clip_ids[slot] = child_id;
                    }
                }
            }
            NodeContainer::Clip(_) => {}
        }
        for kind in StructuralMergeKind::ALL {
            self.reorder_structural_children_for(container, None, kind);
        }
    }

    pub(super) fn normalize_structural_target(&mut self, target: &PortAddress) {
        let mut ids = self
            .connections
            .iter()
            .filter(|connection| connection.to == *target)
            .map(|connection| (connection.order, connection.id))
            .collect::<Vec<_>>();
        ids.sort_by_key(|(order, id)| (*order, *id));
        self.assign_target_orders(&ids.into_iter().map(|(_, id)| id).collect::<Vec<_>>());
    }

    fn assign_target_orders(&mut self, ids: &[Uuid]) {
        for (order, id) in ids.iter().enumerate() {
            if let Some(connection) = self
                .connections
                .iter_mut()
                .find(|connection| connection.id == *id)
            {
                connection.order = order as i64;
            }
        }
    }
}
