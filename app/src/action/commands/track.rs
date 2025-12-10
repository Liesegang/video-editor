use crate::action::command::Command;
use anyhow::Result;
use library::service::project_service::ProjectService;
use uuid::Uuid;

#[derive(Debug)]
pub struct AddTrackCommand {
    composition_id: Uuid,
    track_id: Option<Uuid>, // Store generated ID
    track_name: String,
}

impl AddTrackCommand {
    pub fn new(composition_id: Uuid, track_name: String) -> Self {
        Self {
            composition_id,
            track_id: None,
            track_name,
        }
    }
}

impl Command for AddTrackCommand {
    fn execute(&mut self, service: &mut ProjectService) -> Result<()> {
        let new_track_id = service.add_track(self.composition_id, &self.track_name)?;
        self.track_id = Some(new_track_id);
        Ok(())
    }

    fn undo(&mut self, service: &mut ProjectService) -> Result<()> {
        if let Some(track_id) = self.track_id {
            service.remove_track(self.composition_id, track_id)?;
        }
        Ok(())
    }

    fn redo(&mut self, service: &mut ProjectService) -> Result<()> {
        // Redo should ideally re-add the same track with the same ID,
        // but ProjectService::add_track generates a new ID.
        // For now, we'll re-add a new track with the same name.
        // A more robust solution would involve ProjectService having a restore_track method
        // that takes a full Track object including its ID.
        let new_track_id = service.add_track(self.composition_id, &self.track_name)?;
        self.track_id = Some(new_track_id);
        Ok(())
    }

    fn name(&self) -> String {
        format!("Add Track '{}'", self.track_name)
    }
}

#[derive(Debug)]
pub struct RemoveTrackCommand {
    composition_id: Uuid,
    track_id: Uuid,
    removed_track: Option<library::model::project::Track>, // Store the whole track for undo
}

impl RemoveTrackCommand {
    pub fn new(composition_id: Uuid, track_id: Uuid) -> Self {
        Self {
            composition_id,
            track_id,
            removed_track: None,
        }
    }
}

impl Command for RemoveTrackCommand {
    fn execute(&mut self, service: &mut ProjectService) -> Result<()> {
        // Need to retrieve the track before removing it for undo
        let track_to_remove = service.get_track(self.composition_id, self.track_id)?;
        self.removed_track = Some(track_to_remove);

        service.remove_track(self.composition_id, self.track_id)?;
        Ok(())
    }

    fn undo(&mut self, service: &mut ProjectService) -> Result<()> {
        if let Some(track) = self.removed_track.take() { // Use .take() to move the track out
            service.add_track_with_id(self.composition_id, track)?;
        }
        Ok(())
    }

    fn redo(&mut self, service: &mut ProjectService) -> Result<()> {
        // Re-execute removal. Need to retrieve the track again if it was restored by undo.
        // This is a bit tricky: `removed_track` might be None if undo hasn't happened yet.
        // For simplicity, we'll assume `execute` always stores it and `undo` always takes it.
        // If redo is called after execute, `removed_track` will be None, and we need to fetch it.
        // If redo is called after undo, `removed_track` will be Some, and we can use that.
        let track_to_remove = service.get_track(self.composition_id, self.track_id)?;
        self.removed_track = Some(track_to_remove);
        service.remove_track(self.composition_id, self.track_id)?;
        Ok(())
    }

    fn name(&self) -> String {
        format!("Remove Track {}", self.track_id)
    }
}
