//! Referential-integrity validation for Project containment.

use std::collections::{HashMap, HashSet};

use super::{Project, ProjectGraphError};

impl Project {
    pub fn validate_containment(&self) -> Vec<ProjectGraphError> {
        let mut errors = Vec::new();
        let mut composition_ids = HashSet::new();
        for composition in &self.compositions {
            if !composition_ids.insert(composition.id) {
                errors.push(ProjectGraphError::DuplicateCompositionId(composition.id));
            }
            match composition.frame_count() {
                Ok(frame_count) => {
                    if composition.work_area_in > composition.work_area_out
                        || composition.work_area_out > frame_count
                    {
                        errors.push(ProjectGraphError::InvalidCompositionWorkArea {
                            composition_id: composition.id,
                            work_area_in: composition.work_area_in,
                            work_area_out: composition.work_area_out,
                            frame_count,
                        });
                    }
                }
                Err(reason) => errors.push(ProjectGraphError::InvalidCompositionSettings {
                    composition_id: composition.id,
                    reason,
                }),
            }
        }
        for (key, track) in &self.tracks {
            if *key != track.id {
                errors.push(ProjectGraphError::TrackKeyMismatch {
                    key: *key,
                    entity_id: track.id,
                });
            }
        }
        for (key, clip) in &self.clips {
            if *key != clip.id {
                errors.push(ProjectGraphError::ClipKeyMismatch {
                    key: *key,
                    entity_id: clip.id,
                });
            }
        }
        for (key, node) in &self.nodes {
            if *key != node.id {
                errors.push(ProjectGraphError::NodeKeyMismatch {
                    key: *key,
                    entity_id: node.id,
                });
            }
        }
        for (key, resource) in &self.resources {
            if *key != resource.id {
                errors.push(ProjectGraphError::ResourceKeyMismatch {
                    key: *key,
                    entity_id: resource.id,
                });
            }
        }
        let mut asset_ids = HashSet::new();
        for asset in &self.assets {
            if !asset_ids.insert(asset.id) {
                errors.push(ProjectGraphError::DuplicateAssetId(asset.id));
            }
        }
        let mut connection_ids = HashSet::new();
        for connection in &self.connections {
            if !connection_ids.insert(connection.id) {
                errors.push(ProjectGraphError::DuplicateConnectionId(connection.id));
            }
        }

        let mut track_owners = HashMap::new();
        for composition in &self.compositions {
            for track_id in &composition.track_ids {
                if !self.tracks.contains_key(track_id) {
                    errors.push(ProjectGraphError::TrackNotFound(*track_id));
                }
                if let Some(composition_id) = track_owners.insert(*track_id, composition.id) {
                    errors.push(ProjectGraphError::TrackAlreadyContained {
                        track_id: *track_id,
                        composition_id,
                    });
                }
            }
        }
        for track_id in self.tracks.keys() {
            if !track_owners.contains_key(track_id) {
                errors.push(ProjectGraphError::TrackHasNoComposition(*track_id));
            }
        }

        let mut clip_owners = HashMap::new();
        for track in self.tracks.values() {
            for clip_id in &track.clip_ids {
                if !self.clips.contains_key(clip_id) {
                    errors.push(ProjectGraphError::ClipNotFound(*clip_id));
                }
                if let Some(track_id) = clip_owners.insert(*clip_id, track.id) {
                    errors.push(ProjectGraphError::ClipAlreadyContained {
                        clip_id: *clip_id,
                        track_id,
                    });
                }
            }
        }
        for clip_id in self.clips.keys() {
            if !clip_owners.contains_key(clip_id) {
                errors.push(ProjectGraphError::ClipHasNoTrack(*clip_id));
            }
        }

        let mut owners = HashMap::new();
        for view in self.container_views() {
            for node_id in view.node_ids {
                if !self.nodes.contains_key(node_id) {
                    errors.push(ProjectGraphError::NodeNotFound(*node_id));
                }
                if let Some(previous) = owners.insert(*node_id, view.container) {
                    errors.push(ProjectGraphError::NodeAlreadyContained {
                        node_id: *node_id,
                        container: previous,
                    });
                }
            }
            for output_node_id in [view.image_output_node_id, view.audio_output_node_id]
                .into_iter()
                .flatten()
            {
                if !view.node_ids.contains(&output_node_id) {
                    errors.push(ProjectGraphError::OutputNodeOutsideContainer {
                        node_id: output_node_id,
                        container: view.container,
                    });
                }
            }
        }
        for node_id in self.nodes.keys() {
            if !owners.contains_key(node_id) {
                errors.push(ProjectGraphError::NodeHasNoContainer(*node_id));
            }
        }
        errors.extend(self.validate_structural_merges());
        errors
    }
}
