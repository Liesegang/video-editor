use std::collections::{HashMap, HashSet};

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
        inserted_connection_id: Option<Uuid>,
        kind: StructuralMergeKind,
    ) {
        let Some(target) = self.structural_merge_target_for(container, kind) else {
            return;
        };
        let inserted_connection_id = inserted_connection_id.filter(|id| {
            self.connections
                .iter()
                .any(|connection| connection.id == *id && connection.to == target)
        });
        let children = self.structural_child_owners(container);
        let child_set = children.iter().copied().collect::<HashSet<_>>();
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

        if let Some(inserted_id) = inserted_connection_id {
            let child_for_insert = self
                .connections
                .iter()
                .find(|connection| connection.id == inserted_id)
                .map(|connection| connection.from.owner);
            if let Some(child_index) = child_for_insert
                .and_then(|child| children.iter().position(|candidate| *candidate == child))
            {
                ordered_ids.retain(|id| *id != inserted_id);
                let insertion_index = children[..child_index]
                    .iter()
                    .rev()
                    .find_map(|neighbor| {
                        self.structural_connection_id(&target, *neighbor)
                            .and_then(|id| {
                                ordered_ids.iter().position(|candidate| *candidate == id)
                            })
                            .map(|index| index + 1)
                    })
                    .or_else(|| {
                        children[child_index + 1..].iter().find_map(|neighbor| {
                            self.structural_connection_id(&target, *neighbor)
                                .and_then(|id| {
                                    ordered_ids.iter().position(|candidate| *candidate == id)
                                })
                        })
                    })
                    .unwrap_or(ordered_ids.len());
                ordered_ids.insert(insertion_index.min(ordered_ids.len()), inserted_id);
            }
        }

        let connection_child = self
            .connections
            .iter()
            .filter(|connection| connection.to == target)
            .map(|connection| (connection.id, connection.from.owner))
            .collect::<HashMap<_, _>>();
        let managed_slots = ordered_ids
            .iter()
            .enumerate()
            .filter_map(|(index, id)| {
                connection_child
                    .get(id)
                    .is_some_and(|owner| child_set.contains(owner))
                    .then_some(index)
            })
            .collect::<Vec<_>>();
        let desired_ids = children
            .iter()
            .filter_map(|child| self.structural_connection_id(&target, *child))
            .collect::<Vec<_>>();
        if managed_slots.len() == desired_ids.len() {
            for (slot, id) in managed_slots.into_iter().zip(desired_ids) {
                ordered_ids[slot] = id;
            }
        }
        self.assign_target_orders(&ordered_ids);
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
