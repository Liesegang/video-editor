use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use super::{
    IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, NodeContainer, PortAddress, PortDataType, PortDirection,
    PortOwner, Project, ProjectConnection, ProjectGraphError,
};
use crate::model::{Clip, Composition, Node, NodeContent, Track};

impl Project {
    pub(super) fn insert_composition_with_structural_merge(
        &mut self,
        composition: Composition,
    ) -> Result<(), ProjectGraphError> {
        let composition_id = composition.id;
        let missing_tracks = composition
            .track_ids
            .iter()
            .copied()
            .filter(|track_id| !self.tracks.contains_key(track_id))
            .collect::<HashSet<_>>();
        let baseline = self.validate_connections();
        let mut candidate = self.clone();
        candidate.insert_composition_with_structural_merge_in_place(composition)?;
        if let Some(error) = first_disallowed_new_error(
            &baseline,
            candidate.validate_connections(),
            |error| matches!(error, ProjectGraphError::TrackNotFound(track_id) if missing_tracks.contains(track_id)),
        ) {
            return Err(error);
        }
        debug_assert!(candidate.get_composition(composition_id).is_some());
        *self = candidate;
        Ok(())
    }

    fn insert_composition_with_structural_merge_in_place(
        &mut self,
        composition: Composition,
    ) -> Result<(), ProjectGraphError> {
        if self
            .compositions
            .iter()
            .any(|existing| existing.id == composition.id)
        {
            return Err(ProjectGraphError::DuplicateCompositionId(composition.id));
        }
        if self
            .nodes
            .contains_key(&composition.structural_merge_node_id)
        {
            return Err(ProjectGraphError::NodeGraphNodeAlreadyExists(
                composition.structural_merge_node_id,
            ));
        }
        if !composition
            .node_ids
            .contains(&composition.structural_merge_node_id)
        {
            return Err(ProjectGraphError::StructuralMergeNodeOutsideContainer {
                container: NodeContainer::Composition(composition.id),
                node_id: composition.structural_merge_node_id,
            });
        }

        let container = NodeContainer::Composition(composition.id);
        let structural_merge = structural_merge_node(
            composition.structural_merge_node_id,
            "Composition Merge",
            composition.ui_position,
        );
        let child_ids = composition.track_ids.clone();
        self.nodes.insert(structural_merge.id, structural_merge);
        self.compositions.push(composition);

        for track_id in child_ids {
            if self.tracks.contains_key(&track_id) {
                self.create_default_structural_edge(container, PortOwner::Track(track_id));
            }
        }
        self.reorder_structural_children(container, None);
        Ok(())
    }

    pub(super) fn insert_track_with_structural_merge(
        &mut self,
        track: Track,
    ) -> Result<(), ProjectGraphError> {
        let track_id = track.id;
        let missing_clips = track
            .clip_ids
            .iter()
            .copied()
            .filter(|clip_id| !self.clips.contains_key(clip_id))
            .collect::<HashSet<_>>();
        let baseline = self.validate_connections();
        let mut candidate = self.clone();
        candidate.insert_track_with_structural_merge_in_place(track)?;
        if let Some(error) = first_disallowed_new_error(
            &baseline,
            candidate.validate_connections(),
            |error| {
                matches!(error, ProjectGraphError::TrackHasNoComposition(id) if *id == track_id)
                    || matches!(error, ProjectGraphError::ClipNotFound(clip_id) if missing_clips.contains(clip_id))
            },
        ) {
            return Err(error);
        }
        *self = candidate;
        Ok(())
    }

    fn insert_track_with_structural_merge_in_place(
        &mut self,
        track: Track,
    ) -> Result<(), ProjectGraphError> {
        if self.tracks.contains_key(&track.id) {
            return Err(ProjectGraphError::TrackAlreadyExists(track.id));
        }
        if self.nodes.contains_key(&track.structural_merge_node_id) {
            return Err(ProjectGraphError::NodeGraphNodeAlreadyExists(
                track.structural_merge_node_id,
            ));
        }
        if !track.node_ids.contains(&track.structural_merge_node_id) {
            return Err(ProjectGraphError::StructuralMergeNodeOutsideContainer {
                container: NodeContainer::Track(track.id),
                node_id: track.structural_merge_node_id,
            });
        }

        let container = NodeContainer::Track(track.id);
        let structural_merge = structural_merge_node(
            track.structural_merge_node_id,
            "Track Merge",
            track.ui_position,
        );
        let child_ids = track.clip_ids.clone();
        let track_id = track.id;
        self.nodes.insert(structural_merge.id, structural_merge);
        self.tracks.insert(track_id, track);

        for clip_id in child_ids {
            if self.clips.contains_key(&clip_id) {
                self.create_default_structural_edge(container, PortOwner::Clip(clip_id));
            }
        }
        self.reorder_structural_children(container, None);

        let parents = self
            .compositions
            .iter()
            .filter(|composition| composition.track_ids.contains(&track_id))
            .map(|composition| NodeContainer::Composition(composition.id))
            .collect::<Vec<_>>();
        for parent in parents {
            self.create_default_structural_edge(parent, PortOwner::Track(track_id));
            self.reorder_structural_children(parent, None);
        }
        Ok(())
    }

    pub(super) fn insert_clip_with_structural_edges(&mut self, clip: Clip) {
        let clip_id = clip.id;
        self.clips.insert(clip_id, clip);
        let parents = self
            .tracks
            .values()
            .filter(|track| track.clip_ids.contains(&clip_id))
            .map(|track| NodeContainer::Track(track.id))
            .collect::<Vec<_>>();
        for parent in parents {
            self.create_default_structural_edge(parent, PortOwner::Clip(clip_id));
            self.reorder_structural_children(parent, None);
        }
    }

    pub(super) fn structural_merge_id(&self, container: NodeContainer) -> Option<Uuid> {
        match container {
            NodeContainer::Composition(id) => self
                .get_composition(id)
                .map(|composition| composition.structural_merge_node_id),
            NodeContainer::Track(id) => self
                .get_track(id)
                .map(|track| track.structural_merge_node_id),
            NodeContainer::Clip(_) => None,
        }
    }

    pub(super) fn structural_merge_owner(&self, node_id: Uuid) -> Option<NodeContainer> {
        self.compositions
            .iter()
            .find(|composition| composition.structural_merge_node_id == node_id)
            .map(|composition| NodeContainer::Composition(composition.id))
            .or_else(|| {
                self.tracks
                    .values()
                    .find(|track| track.structural_merge_node_id == node_id)
                    .map(|track| NodeContainer::Track(track.id))
            })
    }

    pub(super) fn structural_merge_is_well_formed(&self, container: NodeContainer) -> bool {
        let Some(node_id) = self.structural_merge_id(container) else {
            return false;
        };
        let directly_contained = match container {
            NodeContainer::Composition(id) => self
                .get_composition(id)
                .is_some_and(|composition| composition.node_ids.contains(&node_id)),
            NodeContainer::Track(id) => self
                .get_track(id)
                .is_some_and(|track| track.node_ids.contains(&node_id)),
            NodeContainer::Clip(_) => false,
        };
        directly_contained
            && self
                .get_node(node_id)
                .is_some_and(|node| matches!(node.content(), NodeContent::Merge))
    }

    pub(super) fn structural_merge_reaches_output(
        &self,
        container: NodeContainer,
        output_node_id: Uuid,
        connections: &[ProjectConnection],
    ) -> bool {
        let Some(structural_merge_id) = self.structural_merge_id(container) else {
            return false;
        };
        if structural_merge_id == output_node_id {
            return true;
        }

        let mut pending = vec![output_node_id];
        let mut visited = HashSet::new();
        while let Some(target_node_id) = pending.pop() {
            if !visited.insert(target_node_id) {
                continue;
            }
            for connection in connections
                .iter()
                .filter(|connection| connection.to.owner == PortOwner::Node(target_node_id))
            {
                let source_is_image = self
                    .port_definition(&connection.from, PortDirection::Output)
                    .is_some_and(|port| port.data_type == PortDataType::Image);
                let target_is_image = self
                    .port_definition(&connection.to, PortDirection::Input)
                    .is_some_and(|port| port.data_type == PortDataType::Image);
                if !source_is_image || !target_is_image {
                    continue;
                }
                let PortOwner::Node(source_node_id) = connection.from.owner else {
                    continue;
                };
                if source_node_id == structural_merge_id {
                    return true;
                }
                pending.push(source_node_id);
            }
        }
        false
    }

    pub(super) fn transition_structural_child(
        &mut self,
        old_parent: Option<NodeContainer>,
        new_parent: NodeContainer,
        child: PortOwner,
    ) {
        if old_parent == Some(new_parent) {
            self.reorder_structural_children(new_parent, None);
            return;
        }

        let source = PortAddress::new(child, IMAGE_OUTPUT_PORT);
        let new_target = self.structural_merge_target(new_parent);
        let moved_connection_id = old_parent
            .and_then(|parent| self.structural_merge_target(parent))
            .and_then(|old_target| {
                self.connections
                    .iter()
                    .position(|connection| connection.from == source && connection.to == old_target)
                    .map(|index| {
                        let connection_id = self.connections[index].id;
                        if let Some(new_target) = &new_target {
                            self.connections[index].to = new_target.clone();
                        }
                        (connection_id, old_target)
                    })
            });

        if let Some((connection_id, old_target)) = moved_connection_id {
            self.normalize_structural_target(&old_target);
            if new_target.is_some() {
                self.reorder_structural_children(new_parent, Some(connection_id));
            }
            return;
        }

        self.create_default_structural_edge(new_parent, child);
    }

    pub(super) fn disconnect_structural_child(
        &mut self,
        parents: &[NodeContainer],
        child: PortOwner,
    ) {
        let targets = parents
            .iter()
            .filter_map(|parent| self.structural_merge_target(*parent))
            .collect::<HashSet<_>>();
        let source = PortAddress::new(child, IMAGE_OUTPUT_PORT);
        let connection_ids = self
            .connections
            .iter()
            .filter(|connection| connection.from == source && targets.contains(&connection.to))
            .map(|connection| connection.id)
            .collect::<Vec<_>>();
        self.disconnect_connections(connection_ids);
    }

    pub(super) fn reorder_structural_children(
        &mut self,
        container: NodeContainer,
        inserted_connection_id: Option<Uuid>,
    ) {
        let Some(target) = self.structural_merge_target(container) else {
            return;
        };
        let children = self.structural_child_owners(container);
        let child_set = children.iter().copied().collect::<HashSet<_>>();
        let mut ordered_connections = self
            .connections
            .iter()
            .filter(|connection| connection.to == target)
            .map(|connection| (connection.order, connection.id))
            .collect::<Vec<_>>();
        ordered_connections.sort_by_key(|(order, id)| (*order, *id));
        let mut ordered_ids = ordered_connections
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
                let insertion_index = {
                    children[..child_index]
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
                        .unwrap_or(ordered_ids.len())
                };
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

    pub(super) fn sync_child_order_from_structural_target(&mut self, target: &PortAddress) {
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
    }

    pub(super) fn validate_structural_merges(&self) -> Vec<ProjectGraphError> {
        let containers = self
            .compositions
            .iter()
            .map(|composition| NodeContainer::Composition(composition.id))
            .chain(
                self.tracks
                    .values()
                    .map(|track| NodeContainer::Track(track.id)),
            );
        let mut errors = Vec::new();
        for container in containers {
            let Some(node_id) = self.structural_merge_id(container) else {
                continue;
            };
            let Some(node) = self.get_node(node_id) else {
                errors.push(ProjectGraphError::StructuralMergeNodeMissing { container, node_id });
                continue;
            };
            let directly_contained = match container {
                NodeContainer::Composition(id) => self
                    .get_composition(id)
                    .is_some_and(|composition| composition.node_ids.contains(&node_id)),
                NodeContainer::Track(id) => self
                    .get_track(id)
                    .is_some_and(|track| track.node_ids.contains(&node_id)),
                NodeContainer::Clip(_) => false,
            };
            if !directly_contained {
                errors.push(ProjectGraphError::StructuralMergeNodeOutsideContainer {
                    container,
                    node_id,
                });
                continue;
            }
            if !matches!(node.content(), NodeContent::Merge) {
                errors.push(ProjectGraphError::StructuralMergeNodeWrongType { container, node_id });
                continue;
            }

            let output_node_id = match container {
                NodeContainer::Composition(id) => self
                    .get_composition(id)
                    .and_then(|composition| composition.output_node_id),
                NodeContainer::Track(id) => {
                    self.get_track(id).and_then(|track| track.output_node_id)
                }
                NodeContainer::Clip(_) => None,
            };
            if let Some(output_node_id) = output_node_id
                && self.find_node_container(output_node_id) == Some(container)
                && !self.structural_merge_reaches_output(
                    container,
                    output_node_id,
                    &self.connections,
                )
            {
                errors.push(ProjectGraphError::StructuralMergeDoesNotReachOutput {
                    container,
                    node_id,
                    output_node_id,
                });
            }

            let Some(target) = self.structural_merge_target(container) else {
                continue;
            };
            for child in self.structural_child_owners(container) {
                if self
                    .connections
                    .iter()
                    .filter(|connection| {
                        connection.to == target
                            && connection.from == PortAddress::new(child, IMAGE_OUTPUT_PORT)
                    })
                    .count()
                    > 1
                {
                    errors.push(ProjectGraphError::DuplicateStructuralChildEdge {
                        container,
                        node_id,
                        child,
                    });
                }
            }
        }
        errors
    }

    fn create_default_structural_edge(&mut self, container: NodeContainer, child: PortOwner) {
        let Some(target) = self.structural_merge_target(container) else {
            return;
        };
        let source = PortAddress::new(child, IMAGE_OUTPUT_PORT);
        if self
            .connections
            .iter()
            .any(|connection| connection.from == source && connection.to == target)
        {
            self.reorder_structural_children(container, None);
            return;
        }
        let order = self
            .connections
            .iter()
            .filter(|connection| connection.to == target)
            .map(|connection| connection.order)
            .max()
            .unwrap_or(-1)
            + 1;
        let mut connection = ProjectConnection::new(source, target, order);
        while self
            .connections
            .iter()
            .any(|existing| existing.id == connection.id)
        {
            connection.id = Uuid::new_v4();
        }
        let id = connection.id;
        self.connections.push(connection);
        self.reorder_structural_children(container, Some(id));
    }

    fn structural_merge_target(&self, container: NodeContainer) -> Option<PortAddress> {
        self.structural_merge_id(container)
            .map(|node_id| PortAddress::new(PortOwner::Node(node_id), MERGE_IMAGES_PORT))
    }

    fn structural_child_owners(&self, container: NodeContainer) -> Vec<PortOwner> {
        match container {
            NodeContainer::Composition(id) => self
                .get_composition(id)
                .map(|composition| {
                    composition
                        .track_ids
                        .iter()
                        .copied()
                        .map(PortOwner::Track)
                        .collect()
                })
                .unwrap_or_default(),
            NodeContainer::Track(id) => self
                .get_track(id)
                .map(|track| {
                    track
                        .clip_ids
                        .iter()
                        .copied()
                        .map(PortOwner::Clip)
                        .collect()
                })
                .unwrap_or_default(),
            NodeContainer::Clip(_) => Vec::new(),
        }
    }

    fn structural_connection_id(&self, target: &PortAddress, child: PortOwner) -> Option<Uuid> {
        let source = PortAddress::new(child, IMAGE_OUTPUT_PORT);
        self.connections
            .iter()
            .find(|connection| connection.from == source && connection.to == *target)
            .map(|connection| connection.id)
    }

    fn normalize_structural_target(&mut self, target: &PortAddress) {
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

    pub(super) fn container_for_structural_target(
        &self,
        target: &PortAddress,
    ) -> Option<NodeContainer> {
        if target.port != MERGE_IMAGES_PORT {
            return None;
        }
        let PortOwner::Node(node_id) = target.owner else {
            return None;
        };
        self.compositions
            .iter()
            .find(|composition| composition.structural_merge_node_id == node_id)
            .map(|composition| NodeContainer::Composition(composition.id))
            .or_else(|| {
                self.tracks
                    .values()
                    .find(|track| track.structural_merge_node_id == node_id)
                    .map(|track| NodeContainer::Track(track.id))
            })
    }
}

fn structural_merge_node(id: Uuid, name: &str, container_position: [f32; 2]) -> Node {
    let mut node = Node::new_merge(name);
    node.id = id;
    node.ui_position = [container_position[0] + 80.0, container_position[1] + 80.0];
    node
}

fn first_disallowed_new_error(
    baseline: &[ProjectGraphError],
    current: Vec<ProjectGraphError>,
    mut allowed: impl FnMut(&ProjectGraphError) -> bool,
) -> Option<ProjectGraphError> {
    let mut unmatched_baseline = baseline.to_vec();
    current.into_iter().find(|error| {
        if let Some(index) = unmatched_baseline
            .iter()
            .position(|baseline_error| baseline_error == error)
        {
            unmatched_baseline.remove(index);
            return false;
        }
        !allowed(error)
    })
}
