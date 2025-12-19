use crate::error::LibraryError;
use crate::core::model::Track;
use crate::core::model::project::Project;
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
}
