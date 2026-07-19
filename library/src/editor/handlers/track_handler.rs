use crate::error::LibraryError;
use crate::model::Track;
use crate::model::project::Project;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub struct TrackHandler;

impl TrackHandler {
    /// Add a new top-level track to the composition.
    pub fn add_track(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track_name: &str,
    ) -> Result<Uuid, LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let new_track = Track::new(track_name);
        let new_track_id = new_track.id;
        proj.add_track(new_track);

        if let Err(error) = proj.attach_track_to_composition(composition_id, new_track_id) {
            proj.remove_track(new_track_id);
            return Err(LibraryError::Project(error.to_string()));
        }

        Ok(new_track_id)
    }

    /// Add a track with a specific track data (for undo/redo)
    pub fn add_track_with_id(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track: Track,
    ) -> Result<Uuid, LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        let track_id = track.id;
        proj.add_track(track);

        if let Err(error) = proj.attach_track_to_composition(composition_id, track_id) {
            proj.remove_track(track_id);
            return Err(LibraryError::Project(error.to_string()));
        }

        Ok(track_id)
    }

    /// Get a track by ID
    pub fn get_track(
        project: &Arc<RwLock<Project>>,
        _composition_id: Uuid,
        track_id: Uuid,
    ) -> Result<Track, LibraryError> {
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
        composition_id: Uuid,
        track_id: Uuid,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        if proj.get_track(track_id).is_none() {
            return Err(LibraryError::Project(format!(
                "Track with ID {} not found",
                track_id
            )));
        }
        if proj.find_composition_for_track(track_id) != Some(composition_id) {
            return Err(LibraryError::Project(format!(
                "Track with ID {} is not in composition {}",
                track_id, composition_id
            )));
        }

        if proj.remove_track(track_id).is_some() {
            Ok(())
        } else {
            Err(LibraryError::Project(format!(
                "Track with ID {} not found",
                track_id
            )))
        }
    }

    /// Rename a track
    pub fn rename_track(
        project: &Arc<RwLock<Project>>,
        track_id: Uuid,
        new_name: &str,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        if let Some(track) = proj.get_track_mut(track_id) {
            track.name = new_name.to_string();
            Ok(())
        } else {
            Err(LibraryError::Project(format!(
                "Track with ID {} not found",
                track_id
            )))
        }
    }

    /// Reorder a top-level Track inside its current Composition.
    pub fn move_track_within_composition(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track_id: Uuid,
        destination_index: usize,
    ) -> Result<bool, LibraryError> {
        let mut project = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        project
            .move_track_within_composition(composition_id, track_id, destination_index)
            .map_err(|error| LibraryError::Project(error.to_string()))
    }
}
