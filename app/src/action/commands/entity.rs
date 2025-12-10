use crate::action::command::Command;
use anyhow::Result;
use library::service::project_service::ProjectService;

/// Example command: Moves an entity.
#[derive(Debug)]
pub struct MoveEntityCommand {
    entity_id: String,
    old_position: (f32, f32),
    new_position: (f32, f32),
}

impl MoveEntityCommand {
    pub fn new(entity_id: String, old_position: (f32, f32), new_position: (f32, f32)) -> Self {
        Self {
            entity_id,
            old_position,
            new_position,
        }
    }
}

impl Command for MoveEntityCommand {
    fn execute(&mut self, _service: &mut ProjectService) -> Result<()> {
        // In a real scenario, this would call a method on ProjectService to move the entity.
        // For now, we'll just print.
        println!(
            "Executing MoveEntityCommand: Moving entity {} from {:?} to {:?}",
            self.entity_id, self.old_position, self.new_position
        );
        // service.move_entity(&self.entity_id, self.new_position)?;
        Ok(())
    }

    fn undo(&mut self, _service: &mut ProjectService) -> Result<()> {
        println!(
            "Undoing MoveEntityCommand: Moving entity {} from {:?} back to {:?}",
            self.entity_id, self.new_position, self.old_position
        );
        // service.move_entity(&self.entity_id, self.old_position)?;
        Ok(())
    }

    fn redo(&mut self, _service: &mut ProjectService) -> Result<()> {
        println!(
            "Redoing MoveEntityCommand: Moving entity {} from {:?} to {:?}",
            self.entity_id, self.old_position, self.new_position
        );
        // service.move_entity(&self.entity_id, self.new_position)?;
        Ok(())
    }

    fn name(&self) -> String {
        format!("Move Entity {}", self.entity_id)
    }
}

/// Example command: Changes a property of an entity.
#[derive(Debug)]
pub struct ChangePropertyCommand<T: std::fmt::Debug + Clone + PartialEq + 'static> {
    entity_id: String,
    property_name: String,
    old_value: T,
    new_value: T,
}

impl<T: std::fmt::Debug + Clone + PartialEq + 'static> ChangePropertyCommand<T> {
    pub fn new(entity_id: String, property_name: String, old_value: T, new_value: T) -> Self {
        Self {
            entity_id,
            property_name,
            old_value,
            new_value,
        }
    }
}

impl<T: std::fmt::Debug + Clone + PartialEq + 'static> Command for ChangePropertyCommand<T> {
    fn execute(&mut self, _service: &mut ProjectService) -> Result<()> {
        println!(
            "Executing ChangePropertyCommand: Changing property {} of entity {} from {:?} to {:?}",
            self.property_name, self.entity_id, self.old_value, self.new_value
        );
        // service.set_entity_property(&self.entity_id, &self.property_name, self.new_value.clone())?;
        Ok(())
    }

    fn undo(&mut self, _service: &mut ProjectService) -> Result<()> {
        println!(
            "Undoing ChangePropertyCommand: Changing property {} of entity {} from {:?} back to {:?}",
            self.property_name, self.entity_id, self.new_value, self.old_value
        );
        // service.set_entity_property(&self.entity_id, &self.property_name, self.old_value.clone())?;
        Ok(())
    }

    fn redo(&mut self, _service: &mut ProjectService) -> Result<()> {
        println!(
            "Redoing ChangePropertyCommand: Changing property {} of entity {} from {:?} to {:?}",
            self.property_name, self.entity_id, self.old_value, self.new_value
        );
        // service.set_entity_property(&self.entity_id, &self.property_name, self.new_value.clone())?;
        Ok(())
    }

    fn name(&self) -> String {
        format!("Change Property {} of {}", self.property_name, self.entity_id)
    }
}
