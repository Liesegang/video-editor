use crate::error::LibraryError;
use crate::model::project::Track;
use crate::model::project::project::Project;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub struct TrackHandler;

impl TrackHandler {
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
        let track = Track::new(track_name);
        let id = track.id;
        composition.add_track(track);
        Ok(id)
    }

    pub fn add_track_with_id(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track: Track,
    ) -> Result<Uuid, LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let composition = proj.get_composition_mut(composition_id).ok_or_else(|| {
            LibraryError::Project(format!("Composition with ID {} not found", composition_id))
        })?;
        let track_id = track.id;
        composition.add_track(track);
        Ok(track_id)
    }

    pub fn get_track(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track_id: Uuid,
    ) -> Result<Track, LibraryError> {
        let proj = project
            .read()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let composition = proj
            .compositions
            .iter()
            .find(|c| c.id == composition_id)
            .ok_or_else(|| {
                LibraryError::Project(format!("Composition with ID {} not found", composition_id))
            })?;

        composition
            .tracks
            .iter()
            .find(|t| t.id == track_id)
            .cloned()
            .ok_or_else(|| {
                LibraryError::Project(format!(
                    "Track with ID {} not found in Composition {}",
                    track_id, composition_id
                ))
            })
    }

    pub fn remove_track(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        track_id: Uuid,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let composition = proj.get_composition_mut(composition_id).ok_or_else(|| {
            LibraryError::Project(format!("Composition with ID {} not found", composition_id))
        })?;

        if composition.remove_track(track_id).is_some() {
            Ok(())
        } else {
            Err(LibraryError::Project(format!(
                "Track with ID {} not found in Composition {}",
                track_id, composition_id
            )))
        }
    }

    /// Add a sub-track (child) to an existing parent track.
    pub fn add_sub_track(
        project: &Arc<RwLock<Project>>,
        composition_id: Uuid,
        parent_track_id: Uuid,
        track_name: &str,
    ) -> Result<Uuid, LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let composition = proj.get_composition_mut(composition_id).ok_or_else(|| {
            LibraryError::Project(format!("Composition with ID {} not found", composition_id))
        })?;

        // Recursively find parent track and add child
        fn add_child_to_track(tracks: &mut [Track], parent_id: Uuid, child: Track) -> bool {
            for track in tracks.iter_mut() {
                if track.id == parent_id {
                    track.add_sub_track(child);
                    return true;
                }
                // Recurse into sub-tracks within children
                for item in &mut track.children {
                    if let crate::model::project::TrackItem::SubTrack(sub_track) = item {
                        if sub_track.id == parent_id {
                            sub_track.add_sub_track(child);
                            return true;
                        }
                    }
                }
            }
            false
        }

        let new_track = Track::new(track_name);
        let new_track_id = new_track.id;

        if add_child_to_track(&mut composition.tracks, parent_track_id, new_track) {
            Ok(new_track_id)
        } else {
            Err(LibraryError::Project(format!(
                "Parent track with ID {} not found",
                parent_track_id
            )))
        }
    }
}
