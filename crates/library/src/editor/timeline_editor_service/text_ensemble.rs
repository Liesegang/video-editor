use super::*;
use crate::editor::{TextEnsembleOperationFactory, TextEnsembleOperationKind};

#[path = "text_ensemble/node_clip.rs"]
mod node_clip;
pub use node_clip::{NodeClipTextEnsembleEntry, NodeClipTextEnsembleStack};

impl TimelineEditorService {
    pub fn add_text_ensemble_operation_by_id(
        &self,
        plugins: &PluginManager,
        item_id: TimelineItemId,
        kind: TextEnsembleOperationKind,
        component_id: &str,
    ) -> Result<(uuid::Uuid, ChangeSet), LibraryError> {
        let operation = TextEnsembleOperationFactory::create(plugins, kind, component_id)?;
        let operation_id = operation.id;
        let changes = self.edit_item(item_id, |item| {
            let operations = text_operations_mut(item, item_id)?;
            let index = match kind {
                TextEnsembleOperationKind::Effector => operations
                    .iter()
                    .position(|candidate| {
                        candidate.operation.category == crate::plugin::DECORATOR_CATEGORY
                    })
                    .unwrap_or(operations.len()),
                TextEnsembleOperationKind::Decorator => operations.len(),
            };
            operations.insert(index, operation);
            Ok(())
        })?;
        Ok((operation_id, changes))
    }

    pub fn remove_text_ensemble_operation(
        &self,
        item_id: TimelineItemId,
        operation_id: uuid::Uuid,
    ) -> Result<ChangeSet, LibraryError> {
        self.edit_item(item_id, |item| {
            let operations = text_operations_mut(item, item_id)?;
            let index = operation_index(operations, operation_id)?;
            operations.remove(index);
            Ok(())
        })
    }

    pub fn reorder_text_ensemble_operation(
        &self,
        item_id: TimelineItemId,
        operation_id: uuid::Uuid,
        new_index: usize,
    ) -> Result<ChangeSet, LibraryError> {
        self.edit_item(item_id, |item| {
            let operations = text_operations_mut(item, item_id)?;
            if new_index >= operations.len() {
                return Err(format!(
                    "Text Ensemble index {new_index} is outside item {item_id}"
                ));
            }
            let old_index = operation_index(operations, operation_id)?;
            if operations[old_index].operation.category != operations[new_index].operation.category
            {
                return Err(
                    "Text Ensemble operations can only be reordered within their execution phase"
                        .to_string(),
                );
            }
            let operation = operations.remove(old_index);
            operations.insert(new_index, operation);
            Ok(())
        })
    }

    pub fn set_text_ensemble_property(
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
        let operation = text_operations(item, item_id)?
            .iter()
            .find(|operation| operation.id == operation_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!("Missing Text Ensemble operation {operation_id}"))
            })?;
        let descriptor = plugins.text_ensemble_operation_descriptor(
            &operation.operation.category,
            &operation.operation.component_id,
        )?;
        let definition = descriptor
            .properties()
            .iter()
            .find(|definition| definition.name() == key)
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "Text Ensemble operation {operation_id} has no Property '{key}'"
                ))
            })?;
        definition
            .validate_value(&value)
            .map_err(LibraryError::Validation)?;
        let property = operation.properties.get(key).cloned().ok_or_else(|| {
            LibraryError::Validation(format!(
                "Text Ensemble operation {operation_id} has no Property '{key}'"
            ))
        })?;
        drop(snapshot);

        let owner = AuthoringPropertyOwner::TextEnsemble {
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

fn text_operations(
    item: &TimelineItem,
    item_id: TimelineItemId,
) -> Result<&[crate::model::authoring::TextEnsembleOperation], LibraryError> {
    let SourceRef::Text {
        ensemble_operations,
        ..
    } = &item.source
    else {
        return Err(LibraryError::Validation(format!(
            "Timeline item {item_id} is not Text"
        )));
    };
    Ok(ensemble_operations)
}

fn text_operations_mut(
    item: &mut TimelineItem,
    item_id: TimelineItemId,
) -> Result<&mut Vec<crate::model::authoring::TextEnsembleOperation>, String> {
    let SourceRef::Text {
        ensemble_operations,
        ..
    } = &mut item.source
    else {
        return Err(format!("Timeline item {item_id} is not Text"));
    };
    Ok(ensemble_operations)
}

fn operation_index(
    operations: &[crate::model::authoring::TextEnsembleOperation],
    operation_id: uuid::Uuid,
) -> Result<usize, String> {
    operations
        .iter()
        .position(|operation| operation.id == operation_id)
        .ok_or_else(|| format!("Missing Text Ensemble operation {operation_id}"))
}
