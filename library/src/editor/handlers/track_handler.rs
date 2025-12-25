use crate::error::LibraryError;
use crate::model::project::project::Project;
use crate::model::project::{Node, TrackData};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub struct TrackHandler;

impl TrackHandler {
    /// Add a new track as a child of the composition's root track
    pub fn add_track(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track_name: &str,
    ) -> Result<Uuid, LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let composition = proj.get_composition_mut(composition_id).ok_or_else(|| {
            LibraryError::Project(format!("Composition with ID {} not found", composition_id))
        })?;

        let root_track_id = composition.root_track_id;

        // Create new track
        let new_track = TrackData::new(track_name);
        let new_track_id = new_track.id;

        // Add to nodes registry
        proj.add_node(Node::Track(new_track));

        // Add as child of root track
        if let Some(root_track) = proj.get_track_mut(root_track_id) {
            root_track.add_child(new_track_id);
        } else {
            return Err(LibraryError::Project(format!(
                "Root track {} not found",
                root_track_id
            )));
        }

        Ok(new_track_id)
    }

    /// Add a track with a specific track data (for undo/redo)
    pub fn add_track_with_id(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track: TrackData,
    ) -> Result<Uuid, LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let composition = proj.get_composition_mut(composition_id).ok_or_else(|| {
            LibraryError::Project(format!("Composition with ID {} not found", composition_id))
        })?;

        let root_track_id = composition.root_track_id;
        let track_id = track.id;

        // Add to nodes registry
        proj.add_node(Node::Track(track));

        // Add as child of root track
        if let Some(root_track) = proj.get_track_mut(root_track_id) {
            root_track.add_child(track_id);
        } else {
            return Err(LibraryError::Project(format!(
                "Root track {} not found",
                root_track_id
            )));
        }

        Ok(track_id)
    }

    /// Get a track by ID
    pub fn get_track(
        project: &Arc<RwLock<Project>>,
        _composition_id: Uuid,
        track_id: Uuid,
    ) -> Result<TrackData, LibraryError> {
        let proj = project
            .read()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        proj.get_track(track_id)
            .cloned()
            .ok_or_else(|| LibraryError::Project(format!("Track with ID {} not found", track_id)))
    }

    /// Remove a track by ID
    pub fn remove_track(
        project: &Arc<RwLock<Project>>,
        _composition_id: Uuid,
        track_id: Uuid,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        // Remove from parent's child_ids (need to find parent first)
        let parent_id = proj
            .all_tracks()
            .find(|t| t.child_ids.contains(&track_id))
            .map(|t| t.id);

        if let Some(pid) = parent_id {
            if let Some(parent) = proj.get_track_mut(pid) {
                parent.remove_child(track_id);
            }
        }

        // Remove the track node itself
        if proj.remove_node(track_id).is_some() {
            Ok(())
        } else {
            Err(LibraryError::Project(format!(
                "Track with ID {} not found",
                track_id
            )))
        }
    }

    /// Add a sub-track (child) to an existing parent track
    pub fn add_sub_track(
        project: &Arc<RwLock<Project>>,
        _composition_id: Uuid,
        parent_track_id: Uuid,
        track_name: &str,
    ) -> Result<Uuid, LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        // Create new track
        let new_track = TrackData::new(track_name);
        let new_track_id = new_track.id;

        // Add to nodes registry
        proj.add_node(Node::Track(new_track));

        // Add as child of parent track
        if let Some(parent_track) = proj.get_track_mut(parent_track_id) {
            parent_track.add_child(new_track_id);
            Ok(new_track_id)
        } else {
            Err(LibraryError::Project(format!(
                "Parent track with ID {} not found",
                parent_track_id
            )))
        }
    }
}
