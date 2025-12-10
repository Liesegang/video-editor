use crate::action::command::Command;
use anyhow::Result;
use library::service::project_service::ProjectService;
use library::model::project::property::PropertyValue;
use uuid::Uuid;

/// Example command: Moves an entity.
#[derive(Debug)]
pub struct MoveEntityCommand {
    composition_id: Uuid,
    track_id: Uuid,
    entity_id: Uuid,
    old_position: (f32, f32),
    new_position: (f32, f32),
}

impl MoveEntityCommand {
    pub fn new(composition_id: Uuid, track_id: Uuid, entity_id: Uuid, old_position: (f32, f32), new_position: (f32, f32)) -> Self {
        Self {
            composition_id,
            track_id,
            entity_id,
            old_position,
            new_position,
        }
    }
}

impl Command for MoveEntityCommand {
    fn execute(&mut self, service: &mut ProjectService) -> Result<()> {
        service.update_entity_property(
            self.composition_id,
            self.track_id,
            self.entity_id,
            "position_x",
            PropertyValue::Number(self.new_position.0 as f64),
        )?;
        service.update_entity_property(
            self.composition_id,
            self.track_id,
            self.entity_id,
            "position_y",
            PropertyValue::Number(self.new_position.1 as f64),
        )?;
        Ok(())
    }

    fn undo(&mut self, service: &mut ProjectService) -> Result<()> {
        service.update_entity_property(
            self.composition_id,
            self.track_id,
            self.entity_id,
            "position_x",
            PropertyValue::Number(self.old_position.0 as f64),
        )?;
        service.update_entity_property(
            self.composition_id,
            self.track_id,
            self.entity_id,
            "position_y",
            PropertyValue::Number(self.old_position.1 as f64),
        )?;
        Ok(())
    }

    fn redo(&mut self, service: &mut ProjectService) -> Result<()> {
        self.execute(service)
    }

    fn name(&self) -> String {
        format!("Move Entity {}", self.entity_id)
    }
}

/// Example command: Changes a property of an entity.
#[derive(Debug)]
pub struct ChangePropertyCommand<T: std::fmt::Debug + Clone + PartialEq + 'static> {
    composition_id: Uuid,
    track_id: Uuid,
    entity_id: Uuid,
    property_name: String,
    old_value: T,
    new_value: T,
}

impl<T: std::fmt::Debug + Clone + PartialEq + 'static> ChangePropertyCommand<T> {
    pub fn new(composition_id: Uuid, track_id: Uuid, entity_id: Uuid, property_name: String, old_value: T, new_value: T) -> Self {
        Self {
            composition_id,
            track_id,
            entity_id,
            property_name,
            old_value,
            new_value,
        }
    }
}

impl<T: std::fmt::Debug + Clone + PartialEq + 'static + Into<PropertyValue>> Command for ChangePropertyCommand<T> {
    fn execute(&mut self, service: &mut ProjectService) -> Result<()> {
        service.update_entity_property(
            self.composition_id,
            self.track_id,
            self.entity_id,
            &self.property_name,
            self.new_value.clone().into(),
        )?;
        Ok(())
    }

    fn undo(&mut self, service: &mut ProjectService) -> Result<()> {
        service.update_entity_property(
            self.composition_id,
            self.track_id,
            self.entity_id,
            &self.property_name,
            self.old_value.clone().into(),
        )?;
        Ok(())
    }

    fn redo(&mut self, service: &mut ProjectService) -> Result<()> {
        self.execute(service)
    }

    fn name(&self) -> String {
        format!("Change Property {} of {}", self.property_name, self.entity_id)
    }
}
