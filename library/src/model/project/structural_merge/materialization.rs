use std::collections::HashSet;

use uuid::Uuid;

use super::super::{
    NodeContainer, PortAddress, PortOwner, Project, ProjectConnection, ProjectGraphError,
};
use super::contract::{StructuralMergeKind, StructuralMergePairSpec, structural_merge_pair};
use crate::model::node::{Clip, Track};
use crate::model::project::Composition;

impl Project {
    pub(in crate::model::project) fn insert_composition_with_structural_merge(
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
        if composition.structural_sound_merge_node_id == composition.structural_merge_node_id
            || self
                .nodes
                .contains_key(&composition.structural_sound_merge_node_id)
        {
            return Err(ProjectGraphError::NodeGraphNodeAlreadyExists(
                composition.structural_sound_merge_node_id,
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
        if !composition
            .node_ids
            .contains(&composition.structural_sound_merge_node_id)
        {
            return Err(ProjectGraphError::StructuralMergeNodeOutsideContainer {
                container: NodeContainer::Composition(composition.id),
                node_id: composition.structural_sound_merge_node_id,
            });
        }

        let container = NodeContainer::Composition(composition.id);
        let after_child_right = composition
            .track_ids
            .iter()
            .filter_map(|track_id| self.get_track(*track_id))
            .map(|track| track.ui_position[0] + track.ui_size[0])
            .max_by(f32::total_cmp);
        let (structural_merge, structural_sound_merge) =
            structural_merge_pair(StructuralMergePairSpec {
                image_id: composition.structural_merge_node_id,
                image_name: "Composition Merge",
                sound_id: composition.structural_sound_merge_node_id,
                sound_name: "Composition Sound Merge",
                container_position: composition.ui_position,
                container_size: composition.ui_size,
                after_child_right,
            });
        let child_ids = composition.track_ids.clone();
        self.nodes.insert(structural_merge.id, structural_merge);
        self.nodes
            .insert(structural_sound_merge.id, structural_sound_merge);
        self.compositions.push(composition);

        for track_id in child_ids {
            if self.tracks.contains_key(&track_id) {
                self.create_default_structural_edge(container, PortOwner::Track(track_id));
            }
        }
        self.reorder_structural_children(container, None);
        Ok(())
    }

    pub(in crate::model::project) fn insert_track_with_structural_merge(
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
        if track.structural_sound_merge_node_id == track.structural_merge_node_id
            || self
                .nodes
                .contains_key(&track.structural_sound_merge_node_id)
        {
            return Err(ProjectGraphError::NodeGraphNodeAlreadyExists(
                track.structural_sound_merge_node_id,
            ));
        }
        if !track.node_ids.contains(&track.structural_merge_node_id) {
            return Err(ProjectGraphError::StructuralMergeNodeOutsideContainer {
                container: NodeContainer::Track(track.id),
                node_id: track.structural_merge_node_id,
            });
        }
        if !track
            .node_ids
            .contains(&track.structural_sound_merge_node_id)
        {
            return Err(ProjectGraphError::StructuralMergeNodeOutsideContainer {
                container: NodeContainer::Track(track.id),
                node_id: track.structural_sound_merge_node_id,
            });
        }

        let container = NodeContainer::Track(track.id);
        let (structural_merge, structural_sound_merge) =
            structural_merge_pair(StructuralMergePairSpec {
                image_id: track.structural_merge_node_id,
                image_name: "Track Merge",
                sound_id: track.structural_sound_merge_node_id,
                sound_name: "Track Sound Merge",
                container_position: track.ui_position,
                container_size: track.ui_size,
                after_child_right: None,
            });
        let child_ids = track.clip_ids.clone();
        let track_id = track.id;
        self.nodes.insert(structural_merge.id, structural_merge);
        self.nodes
            .insert(structural_sound_merge.id, structural_sound_merge);
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

    pub(in crate::model::project) fn insert_clip_with_structural_edges(&mut self, clip: Clip) {
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

    pub(in crate::model::project) fn transition_structural_child(
        &mut self,
        old_parent: Option<NodeContainer>,
        new_parent: NodeContainer,
        child: PortOwner,
    ) {
        if old_parent == Some(new_parent) {
            self.reorder_structural_children(new_parent, None);
            return;
        }

        for kind in StructuralMergeKind::ALL {
            self.transition_structural_child_for(old_parent, new_parent, child, kind);
        }
    }

    fn transition_structural_child_for(
        &mut self,
        old_parent: Option<NodeContainer>,
        new_parent: NodeContainer,
        child: PortOwner,
        kind: StructuralMergeKind,
    ) {
        let source = PortAddress::new(child, kind.source_port());
        let new_target = self.structural_merge_target_for(new_parent, kind);
        let moved_connection_id = old_parent
            .and_then(|parent| self.structural_merge_target_for(parent, kind))
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
                self.reorder_structural_children_for(new_parent, Some(connection_id), kind);
            }
            return;
        }

        self.create_default_structural_edge_for(new_parent, child, kind);
    }

    pub(in crate::model::project) fn disconnect_structural_child(
        &mut self,
        parents: &[NodeContainer],
        child: PortOwner,
    ) {
        let mut connection_ids = Vec::new();
        for kind in StructuralMergeKind::ALL {
            let targets = parents
                .iter()
                .filter_map(|parent| self.structural_merge_target_for(*parent, kind))
                .collect::<HashSet<_>>();
            let source = PortAddress::new(child, kind.source_port());
            connection_ids.extend(
                self.connections
                    .iter()
                    .filter(|connection| {
                        connection.from == source && targets.contains(&connection.to)
                    })
                    .map(|connection| connection.id),
            );
        }
        self.disconnect_connections_unchecked(connection_ids);
    }

    fn create_default_structural_edge(&mut self, container: NodeContainer, child: PortOwner) {
        for kind in StructuralMergeKind::ALL {
            self.create_default_structural_edge_for(container, child, kind);
        }
    }

    fn create_default_structural_edge_for(
        &mut self,
        container: NodeContainer,
        child: PortOwner,
        kind: StructuralMergeKind,
    ) {
        let Some(target) = self.structural_merge_target_for(container, kind) else {
            return;
        };
        let source = PortAddress::new(child, kind.source_port());
        if self
            .connections
            .iter()
            .any(|connection| connection.from == source && connection.to == target)
        {
            self.reorder_structural_children_for(container, None, kind);
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
        self.reorder_structural_children_for(container, Some(id), kind);
    }
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
