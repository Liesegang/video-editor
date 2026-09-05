use super::*;

use crate::model::authoring::{Paint, PaintDefinition, PaintDefinitionId};
use crate::model::property::ColorValue;

const MISSING_DEFINITION: &str = "Missing Paint Definition";
const INVALID_NAME: &str = "Paint Definition name must not be empty";
const NOT_UNGROUPED: &str = "Paint Definition is not in the ungrouped Palette order";

impl TimelineEditorService {
    /// Adds one managed Paint to the end of the ungrouped Project Palette.
    pub fn add_paint_definition(
        &self,
        name: String,
        paint: Paint,
    ) -> Result<(PaintDefinitionId, ChangeSet), LibraryError> {
        let name = normalized_name(name)?;
        let definition_id = PaintDefinitionId::new();
        let mut session = self.write_session()?;
        session
            .transact(vec![ProjectInvalidation::ProjectPalette], |project| {
                project.palette.definitions.insert(
                    definition_id,
                    PaintDefinition {
                        id: definition_id,
                        name,
                        paint,
                        tags: Vec::new(),
                    },
                );
                project.palette.ungrouped_order.push(definition_id);
                Ok(definition_id)
            })
            .map_err(LibraryError::Validation)
    }

    /// Adds one managed Solid swatch to the end of the ungrouped Palette.
    pub fn add_solid_paint_definition(
        &self,
        name: String,
        color: ColorValue,
    ) -> Result<(PaintDefinitionId, ChangeSet), LibraryError> {
        self.add_paint_definition(name, Paint::Solid(color))
    }

    pub fn rename_paint_definition(
        &self,
        definition_id: PaintDefinitionId,
        name: String,
    ) -> Result<ChangeSet, LibraryError> {
        let name = normalized_name(name)?;
        let mut session = self.write_session()?;
        let definition = session
            .project()
            .palette
            .definitions
            .get(&definition_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!("{MISSING_DEFINITION}: {definition_id}"))
            })?;
        if definition.name == name {
            return Ok(unchanged(session.revision()));
        }
        session
            .transact(
                vec![ProjectInvalidation::PaintDefinition { definition_id }],
                |project| {
                    project
                        .palette
                        .definitions
                        .get_mut(&definition_id)
                        .ok_or_else(|| format!("{MISSING_DEFINITION}: {definition_id}"))?
                        .name = name;
                    Ok(())
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    /// Moves an ungrouped swatch to its final index in the visible order.
    pub fn reorder_paint_definition(
        &self,
        definition_id: PaintDefinitionId,
        new_index: usize,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        if !session
            .project()
            .palette
            .definitions
            .contains_key(&definition_id)
        {
            return Err(LibraryError::Validation(format!(
                "{MISSING_DEFINITION}: {definition_id}"
            )));
        }
        let old_index = session
            .project()
            .palette
            .ungrouped_order
            .iter()
            .position(|candidate| *candidate == definition_id)
            .ok_or_else(|| LibraryError::Validation(format!("{NOT_UNGROUPED}: {definition_id}")))?;
        if new_index >= session.project().palette.ungrouped_order.len() {
            return Err(LibraryError::Validation(format!(
                "Paint Definition reorder index {new_index} is outside ungrouped Palette"
            )));
        }
        if old_index == new_index {
            return Ok(unchanged(session.revision()));
        }
        session
            .transact(vec![ProjectInvalidation::ProjectPalette], |project| {
                let moved = project.palette.ungrouped_order.remove(old_index);
                project.palette.ungrouped_order.insert(new_index, moved);
                Ok(())
            })
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub fn delete_paint_definition(
        &self,
        definition_id: PaintDefinitionId,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        session
            .transact(vec![ProjectInvalidation::ProjectPalette], |project| {
                project
                    .palette
                    .definitions
                    .remove(&definition_id)
                    .ok_or_else(|| format!("{MISSING_DEFINITION}: {definition_id}"))?;
                project
                    .palette
                    .ungrouped_order
                    .retain(|candidate| *candidate != definition_id);
                for group in &mut project.palette.groups {
                    group
                        .definition_order
                        .retain(|candidate| *candidate != definition_id);
                }
                Ok(())
            })
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }
}

fn normalized_name(name: String) -> Result<String, LibraryError> {
    let name = name.trim();
    if name.is_empty() {
        Err(LibraryError::Validation(INVALID_NAME.to_string()))
    } else {
        Ok(name.to_string())
    }
}

fn unchanged(revision: ProjectRevision) -> ChangeSet {
    ChangeSet {
        revision,
        invalidations: Vec::new(),
    }
}
