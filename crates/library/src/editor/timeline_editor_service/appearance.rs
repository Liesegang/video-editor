//! Direct Text/Shape appearance-stack authoring.

use super::*;
use crate::editor::AppearanceOperationFactory;
use crate::model::authoring::AppearanceOperation;

#[path = "appearance/node_clip.rs"]
pub(super) mod node_clip;
pub use node_clip::{NodeClipAppearanceEntry, NodeClipAppearanceStack};

impl TimelineEditorService {
    pub fn add_appearance_operation(
        &self,
        plugins: &PluginManager,
        item_id: TimelineItemId,
        component_id: &str,
        index: usize,
    ) -> Result<(uuid::Uuid, ChangeSet), LibraryError> {
        let operation = AppearanceOperationFactory::create(plugins, component_id)?;
        let operation_id = operation.id;
        let changes = self.edit_item(item_id, |item| {
            let operations = appearance_operations_mut(item, item_id)?;
            if index > operations.len() {
                return Err(format!(
                    "Appearance index {index} is outside item {item_id}"
                ));
            }
            operations.insert(index, operation);
            Ok(())
        })?;
        Ok((operation_id, changes))
    }

    pub fn remove_appearance_operation(
        &self,
        item_id: TimelineItemId,
        operation_id: uuid::Uuid,
    ) -> Result<ChangeSet, LibraryError> {
        self.edit_item(item_id, |item| {
            let operations = appearance_operations_mut(item, item_id)?;
            let index = operation_index(operations, operation_id)?;
            operations.remove(index);
            Ok(())
        })
    }

    pub fn reorder_appearance_operation(
        &self,
        item_id: TimelineItemId,
        operation_id: uuid::Uuid,
        new_index: usize,
    ) -> Result<ChangeSet, LibraryError> {
        self.edit_item(item_id, |item| {
            let operations = appearance_operations_mut(item, item_id)?;
            if new_index >= operations.len() {
                return Err(format!(
                    "Appearance index {new_index} is outside item {item_id}"
                ));
            }
            let old_index = operation_index(operations, operation_id)?;
            let operation = operations.remove(old_index);
            operations.insert(new_index, operation);
            Ok(())
        })
    }

    pub fn set_appearance_property(
        &self,
        plugins: &PluginManager,
        item_id: TimelineItemId,
        operation_id: uuid::Uuid,
        key: &str,
        local_time: MediaTime,
        value: PropertyValue,
    ) -> Result<ChangeSet, LibraryError> {
        let snapshot = self.snapshot()?;
        let item = snapshot
            .items
            .get(&item_id)
            .ok_or_else(|| LibraryError::Validation(format!("Missing Timeline item {item_id}")))?;
        let operation = appearance_operations(item, item_id)?
            .iter()
            .find(|operation| operation.id == operation_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!("Missing Appearance operation {operation_id}"))
            })?;
        let descriptor = plugins.operation_descriptor(
            crate::plugin::STYLE_CATEGORY,
            &operation.operation.component_id,
            crate::plugin::STYLE_APPLY_OPERATION,
        )?;
        let definition = descriptor
            .properties()
            .iter()
            .find(|definition| definition.name() == key)
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "Appearance operation {operation_id} has no Property '{key}'"
                ))
            })?;
        definition
            .validate_value(&value)
            .map_err(LibraryError::Validation)?;
        let property = operation.properties.get(key).cloned().ok_or_else(|| {
            LibraryError::Validation(format!(
                "Appearance operation {operation_id} has no Property '{key}'"
            ))
        })?;
        drop(snapshot);

        let owner = AuthoringPropertyOwner::Appearance {
            item_id,
            operation_id,
        };
        match property.evaluator.as_str() {
            "constant" => self.apply_authored_property_values(
                owner,
                vec![AuthoringPropertyValueUpdate {
                    key: key.to_string(),
                    value,
                    target: AuthoringPropertyValueTarget::Constant,
                }],
            ),
            "keyframe" => self.apply_authored_property_values(
                owner,
                vec![AuthoringPropertyValueUpdate {
                    key: key.to_string(),
                    value,
                    target: AuthoringPropertyValueTarget::Keyframe { local_time },
                }],
            ),
            _ => {
                let mut property = property;
                property.properties.insert("value".to_string(), value);
                self.set_authored_property(owner, key.to_string(), property)
            }
        }
    }
}

pub(crate) fn appearance_operations(
    item: &TimelineItem,
    item_id: TimelineItemId,
) -> Result<&[AppearanceOperation], LibraryError> {
    match &item.source {
        SourceRef::Text {
            appearance_operations,
            ..
        } => Ok(appearance_operations),
        SourceRef::Shape { shape } => Ok(&shape.appearance_operations),
        _ => Err(LibraryError::Validation(format!(
            "Timeline item {item_id} has no direct Appearance stack"
        ))),
    }
}

pub(super) fn appearance_operations_mut(
    item: &mut TimelineItem,
    item_id: TimelineItemId,
) -> Result<&mut Vec<AppearanceOperation>, String> {
    match &mut item.source {
        SourceRef::Text {
            appearance_operations,
            ..
        } => Ok(appearance_operations),
        SourceRef::Shape { shape } => Ok(&mut shape.appearance_operations),
        _ => Err(format!(
            "Timeline item {item_id} has no direct Appearance stack"
        )),
    }
}

fn operation_index(
    operations: &[AppearanceOperation],
    operation_id: uuid::Uuid,
) -> Result<usize, String> {
    operations
        .iter()
        .position(|operation| operation.id == operation_id)
        .ok_or_else(|| format!("Missing Appearance operation {operation_id}"))
}
