use std::collections::HashSet;

use super::*;
use crate::model::authoring::{PublishedMediaInputId, PublishedParameterId};

use super::super::interface::cleanup_removed_interface_dependents;

#[derive(Debug)]
pub(in crate::editor::timeline_editor_service) struct RemovedInterfaceEntries {
    pub(in crate::editor::timeline_editor_service) parameter_ids: Vec<PublishedParameterId>,
    pub(in crate::editor::timeline_editor_service) media_input_ids: Vec<PublishedMediaInputId>,
}

impl TimelineEditorService {
    pub fn remove_instance_module_node(
        &self,
        instance_id: ModuleInstanceId,
        node_id: uuid::Uuid,
    ) -> Result<(ModuleDefinitionId, ChangeSet), LibraryError> {
        self.remove_instance_module_nodes(instance_id, vec![node_id])
            .map(|(_, definition_id, changes)| (definition_id, changes))
    }

    /// Deletes one selection as a single topology transaction and removes
    /// only the Published-interface dependents owned by the affected
    /// instance. Shared definitions are copy-on-write before cleanup.
    pub fn remove_instance_module_nodes(
        &self,
        instance_id: ModuleInstanceId,
        node_ids: Vec<uuid::Uuid>,
    ) -> Result<(ModuleInterfaceEditImpact, ModuleDefinitionId, ChangeSet), LibraryError> {
        let mut session = self.write_session()?;
        session
            .transact(
                vec![ProjectInvalidation::ModuleInstance { instance_id }],
                |project| {
                    let definition_id = private_definition_for_instance(project, instance_id)?;
                    let removed = remove_nodes_from_definition(
                        module_definition_mut(project, definition_id)?,
                        &node_ids,
                    )?;
                    let impact = cleanup_removed_interface_dependents(
                        project,
                        &[instance_id],
                        removed.parameter_ids,
                        removed.media_input_ids,
                    )?;
                    Ok((impact, definition_id))
                },
            )
            .map(|((impact, definition_id), changes)| (impact, definition_id, changes))
            .map_err(LibraryError::Validation)
    }

    pub fn remove_shared_module_node(
        &self,
        definition_id: ModuleDefinitionId,
        node_id: uuid::Uuid,
    ) -> Result<SharedModuleEdit<()>, LibraryError> {
        self.remove_shared_module_nodes(definition_id, vec![node_id])
            .map(|edit| SharedModuleEdit {
                value: (),
                affected_instance_count: edit.affected_instance_count,
                changes: edit.changes,
            })
    }

    /// Explicit reusable-template deletion. All instances which share the
    /// definition lose dependents of interfaces owned by the deleted Nodes in
    /// the same transaction.
    pub fn remove_shared_module_nodes(
        &self,
        definition_id: ModuleDefinitionId,
        node_ids: Vec<uuid::Uuid>,
    ) -> Result<SharedModuleEdit<ModuleInterfaceEditImpact>, LibraryError> {
        let mut session = self.write_session()?;
        let definition = session
            .project()
            .module_definitions
            .get(&definition_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!("Missing Module definition {definition_id}"))
            })?;
        if !matches!(
            definition.sharing,
            crate::model::authoring::ModuleDefinitionSharing::ReusableTemplate(_)
        ) {
            return Err(LibraryError::Validation(format!(
                "Module definition {definition_id} is not a reusable template; edit its instance"
            )));
        }
        let instance_ids = session
            .project()
            .module_instances
            .values()
            .filter(|instance| instance.definition_id == definition_id)
            .map(|instance| instance.id)
            .collect::<Vec<_>>();
        let affected_instance_count = instance_ids.len();
        let (impact, changes) = session
            .transact(
                vec![ProjectInvalidation::ModuleDefinition { definition_id }],
                |project| {
                    let removed = remove_nodes_from_definition(
                        module_definition_mut(project, definition_id)?,
                        &node_ids,
                    )?;
                    cleanup_removed_interface_dependents(
                        project,
                        &instance_ids,
                        removed.parameter_ids,
                        removed.media_input_ids,
                    )
                },
            )
            .map_err(LibraryError::Validation)?;
        Ok(SharedModuleEdit {
            value: impact,
            affected_instance_count,
            changes,
        })
    }
}

pub(in crate::editor::timeline_editor_service) fn remove_nodes_from_definition(
    definition: &mut ModuleDefinition,
    node_ids: &[uuid::Uuid],
) -> Result<RemovedInterfaceEntries, String> {
    if node_ids.is_empty() {
        return Err("At least one Module Node must be selected for deletion".to_string());
    }

    let mut selected_node_ids = HashSet::with_capacity(node_ids.len());
    for node_id in node_ids {
        if !selected_node_ids.insert(*node_id) {
            return Err("Module Node deletion contains duplicate IDs".to_string());
        }
        require_unprotected_transition_node(definition, *node_id)?;
        require_removable_processing_node(definition, *node_id)?;
        if !definition.graph.nodes.contains_key(node_id) {
            return Err(format!("Missing Module Node {node_id}"));
        }
    }

    let parameter_ids = definition
        .interface
        .parameters
        .iter()
        .filter(|entry| selected_node_ids.contains(&entry.target.node_id))
        .map(|entry| entry.id)
        .collect::<Vec<_>>();
    let media_input_ids = definition
        .interface
        .media_inputs
        .iter()
        .filter(|entry| selected_node_ids.contains(&entry.target.node_id))
        .map(|entry| entry.id)
        .collect::<Vec<_>>();
    let interface_changed = !parameter_ids.is_empty()
        || !media_input_ids.is_empty()
        || definition
            .interface
            .signals
            .iter()
            .any(|entry| selected_node_ids.contains(&entry.source.node_id))
        || definition
            .interface
            .actions
            .iter()
            .any(|entry| selected_node_ids.contains(&entry.target.node_id));

    definition
        .graph
        .nodes
        .retain(|node_id, _| !selected_node_ids.contains(node_id));
    definition.graph.connections.retain(|connection| {
        !selected_node_ids.contains(&connection.from.node_id)
            && !selected_node_ids.contains(&connection.to.node_id)
    });
    definition
        .interface
        .parameters
        .retain(|entry| !selected_node_ids.contains(&entry.target.node_id));
    definition
        .interface
        .media_inputs
        .retain(|entry| !selected_node_ids.contains(&entry.target.node_id));
    definition
        .interface
        .signals
        .retain(|entry| !selected_node_ids.contains(&entry.source.node_id));
    definition
        .interface
        .actions
        .retain(|entry| !selected_node_ids.contains(&entry.target.node_id));
    bump_topology_revision(definition)?;
    if interface_changed {
        bump_interface_version(definition)?;
    }

    Ok(RemovedInterfaceEntries {
        parameter_ids,
        media_input_ids,
    })
}

fn require_unprotected_transition_node(
    definition: &ModuleDefinition,
    node_id: uuid::Uuid,
) -> Result<(), String> {
    if definition.is_protected_host_boundary_node(node_id) {
        Err(format!(
            "Transition Module Node {node_id} is a protected A/B/Progress/Output boundary and cannot be deleted"
        ))
    } else {
        Ok(())
    }
}
