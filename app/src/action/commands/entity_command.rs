use crate::action::command::Command;
use anyhow::Result;
use library::service::project_service::ProjectService;
use uuid::Uuid;

#[derive(Debug)]
pub struct AddEntityCommand {
    composition_id: Uuid,
    track_id: Uuid,
    entity_id: Option<Uuid>, // Store generated ID
    entity_type: String,
    start_time: f64,
    end_time: f64,
}

impl AddEntityCommand {
    pub fn new(
        composition_id: Uuid,
        track_id: Uuid,
        entity_type: String,
        start_time: f64,
        end_time: f64,
    ) -> Self {
        Self {
            composition_id,
            track_id,
            entity_id: None,
            entity_type,
            start_time,
            end_time,
        }
    }
}

impl Command for AddEntityCommand {
    fn execute(&mut self, service: &mut ProjectService) -> Result<()> {
        let new_entity_id = service.add_entity_to_track(
            self.composition_id,
            self.track_id,
            &self.entity_type,
            self.start_time,
            self.end_time,
        )?;
        self.entity_id = Some(new_entity_id);
        Ok(())
    }

    fn undo(&mut self, service: &mut ProjectService) -> Result<()> {
        if let Some(entity_id) = self.entity_id {
            service.remove_entity_from_track(self.composition_id, self.track_id, entity_id)?;
        }
        Ok(())
    }

    fn redo(&mut self, service: &mut ProjectService) -> Result<()> {
        // Redo re-adds the entity. Since add_entity_to_track generates a new ID,
        // we'll need to update the stored entity_id.
        let new_entity_id = service.add_entity_to_track(
            self.composition_id,
            self.track_id,
            &self.entity_type,
            self.start_time,
            self.end_time,
        )?;
        self.entity_id = Some(new_entity_id);
        Ok(())
    }

    fn name(&self) -> String {
        format!("Add Entity '{}' to Track {}", self.entity_type, self.track_id)
    }
}

#[derive(Debug)]
pub struct MoveEntityTimeCommand {
    composition_id: Uuid,
    track_id: Uuid,
    entity_id: Uuid,
    old_start_time: f64,
    old_end_time: f64,
    new_start_time: f64,
    new_end_time: f64,
}

impl MoveEntityTimeCommand {
    pub fn new(
        composition_id: Uuid,
        track_id: Uuid,
        entity_id: Uuid,
        old_start_time: f64,
        old_end_time: f64,
        new_start_time: f64,
        new_end_time: f64,
    ) -> Self {
        Self {
            composition_id,
            track_id,
            entity_id,
            old_start_time,
            old_end_time,
            new_start_time,
            new_end_time,
        }
    }
}

impl Command for MoveEntityTimeCommand {
    fn execute(&mut self, service: &mut ProjectService) -> Result<()> {
        service.update_entity_time(
            self.composition_id,
            self.track_id,
            self.entity_id,
            self.new_start_time,
            self.new_end_time,
        )?;
        Ok(())
    }

    fn undo(&mut self, service: &mut ProjectService) -> Result<()> {
        service.update_entity_time(
            self.composition_id,
            self.track_id,
            self.entity_id,
            self.old_start_time,
            self.old_end_time,
        )?;
        Ok(())
    }

    fn redo(&mut self, service: &mut ProjectService) -> Result<()> {
        self.execute(service)
    }

    fn name(&self) -> String {
        format!("Move Entity Time for {}", self.entity_id)
    }
}
