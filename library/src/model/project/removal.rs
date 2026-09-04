//! Cascading removal of Project entities and their graph connections.

use uuid::Uuid;

use crate::model::{Clip, Node, NodeContent, Track};

use super::{Composition, PortOwner, Project, ProjectGraphError};

impl Project {
    pub fn remove_node(&mut self, node_id: Uuid) -> Result<Option<Node>, ProjectGraphError> {
        if let Some(container) = self.structural_merge_owner(node_id) {
            return Err(ProjectGraphError::CannotRemoveStructuralMerge { container, node_id });
        }
        Ok(self.remove_node_unchecked(node_id))
    }

    fn remove_node_unchecked(&mut self, node_id: Uuid) -> Option<Node> {
        self.detach_node(node_id);
        let connection_ids = self
            .connections
            .iter()
            .filter(|connection| {
                connection.from.owner == PortOwner::Node(node_id)
                    || connection.to.owner == PortOwner::Node(node_id)
            })
            .map(|connection| connection.id)
            .collect::<Vec<_>>();
        self.disconnect_connections_unchecked(connection_ids);
        self.nodes.remove(&node_id)
    }

    pub fn remove_clip(&mut self, clip_id: Uuid) -> Option<Clip> {
        let clip = self.clips.remove(&clip_id)?;
        self.detach_clip(clip_id);
        let connection_ids = self
            .connections
            .iter()
            .filter(|connection| {
                connection.from.owner == PortOwner::Clip(clip_id)
                    || connection.to.owner == PortOwner::Clip(clip_id)
            })
            .map(|connection| connection.id)
            .collect::<Vec<_>>();
        self.disconnect_connections_unchecked(connection_ids);
        for node_id in clip.node_ids.clone() {
            self.remove_node_unchecked(node_id);
        }
        Some(clip)
    }

    pub fn remove_track(&mut self, track_id: Uuid) -> Option<Track> {
        let track = self.tracks.remove(&track_id)?;
        self.detach_track(track_id);
        let connection_ids = self
            .connections
            .iter()
            .filter(|connection| {
                connection.from.owner == PortOwner::Track(track_id)
                    || connection.to.owner == PortOwner::Track(track_id)
            })
            .map(|connection| connection.id)
            .collect::<Vec<_>>();
        self.disconnect_connections_unchecked(connection_ids);
        for clip_id in track.clip_ids.clone() {
            self.remove_clip(clip_id);
        }
        for node_id in track.node_ids.clone() {
            self.remove_node_unchecked(node_id);
        }
        Some(track)
    }

    pub fn remove_composition(&mut self, composition_id: Uuid) -> Option<Composition> {
        let index = self
            .compositions
            .iter()
            .position(|item| item.id == composition_id)?;
        let composition = self.compositions.remove(index);
        for track_id in composition.track_ids.clone() {
            self.remove_track(track_id);
        }
        for node_id in composition.node_ids.clone() {
            self.remove_node_unchecked(node_id);
        }
        let instances = self
            .nodes
            .values()
            .filter(|node| matches!(node.content(), NodeContent::CompositionInstance(instance) if instance.composition_id == composition_id))
            .map(|node| node.id)
            .collect::<Vec<_>>();
        for node_id in instances {
            self.remove_node_unchecked(node_id);
        }
        let connection_ids = self
            .connections
            .iter()
            .filter(|connection| {
                connection.from.owner == PortOwner::Composition(composition_id)
                    || connection.to.owner == PortOwner::Composition(composition_id)
            })
            .map(|connection| connection.id)
            .collect::<Vec<_>>();
        self.disconnect_connections_unchecked(connection_ids);
        Some(composition)
    }
}
