//! Composition and Track lifecycle and ordering commands.

use super::lifecycle::ProjectManager;
use crate::editor::handlers;
use crate::error::LibraryError;
use crate::model::Track;
use crate::model::project::Composition;
use uuid::Uuid;

impl ProjectManager {
    pub fn remove_composition_fully(&self, comp_id: Uuid) -> Result<(), LibraryError> {
        let mut project_write = self.project.write().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project write lock: {}", e))
        })?;

        project_write
            .remove_composition(comp_id)
            .map(|_| ())
            .ok_or_else(|| {
                LibraryError::Project(format!("Composition with ID {} not found", comp_id))
            })
    }

    pub fn add_composition(
        &self,
        name: &str,
        width: u32,
        height: u32,
        fps: f64,
        duration: f64,
    ) -> Result<Uuid, LibraryError> {
        handlers::composition_handler::CompositionHandler::add_composition(
            &self.project,
            name,
            width.into(),
            height.into(),
            fps,
            duration,
        )
    }

    pub fn get_composition(&self, id: Uuid) -> Result<Composition, LibraryError> {
        handlers::composition_handler::CompositionHandler::get_composition(&self.project, id)
    }

    pub fn update_composition(
        &self,
        id: Uuid,
        name: &str,
        width: u32,
        height: u32,
        fps: f64,
        duration: f64,
    ) -> Result<(), LibraryError> {
        handlers::composition_handler::CompositionHandler::update_composition(
            &self.project,
            id,
            name,
            width,
            height,
            fps,
            duration,
        )
    }

    pub fn is_composition_used(&self, comp_id: Uuid) -> bool {
        handlers::composition_handler::CompositionHandler::is_composition_used(
            &self.project,
            comp_id,
        )
    }

    pub fn add_track(&self, composition_id: Uuid, track_name: &str) -> Result<Uuid, LibraryError> {
        handlers::track_handler::TrackHandler::add_track(&self.project, composition_id, track_name)
    }

    pub fn add_track_with_id(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        track_name: &str,
    ) -> Result<Uuid, LibraryError> {
        let mut track = Track::new(track_name);
        track.id = track_id;
        handlers::track_handler::TrackHandler::add_track_with_id(
            &self.project,
            composition_id,
            track,
        )
    }

    pub fn get_track(&self, composition_id: Uuid, track_id: Uuid) -> Result<Track, LibraryError> {
        handlers::track_handler::TrackHandler::get_track(&self.project, composition_id, track_id)
    }

    pub fn remove_track(&self, composition_id: Uuid, track_id: Uuid) -> Result<(), LibraryError> {
        handlers::track_handler::TrackHandler::remove_track(&self.project, composition_id, track_id)
    }

    pub fn rename_track(&self, track_id: Uuid, new_name: &str) -> Result<(), LibraryError> {
        handlers::track_handler::TrackHandler::rename_track(&self.project, track_id, new_name)
    }

    pub fn move_track_within_composition(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        destination_index: usize,
    ) -> Result<bool, LibraryError> {
        handlers::track_handler::TrackHandler::move_track_within_composition(
            &self.project,
            composition_id,
            track_id,
            destination_index,
        )
    }
}
