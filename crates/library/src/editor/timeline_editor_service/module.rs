use super::*;

mod connections;
mod output;
mod parameter_automation;
pub(super) mod removal;

use connections::{
    connect_definition_ports, disconnect_definition_connection, reconnect_definition_connection,
    set_definition_connection_blend_mode,
};
use output::{
    require_insertable_processing_node, require_output_state, require_removable_processing_node,
};

impl TimelineEditorService {
    pub fn add_module_definition(
        &self,
        definition: ModuleDefinition,
    ) -> Result<ChangeSet, LibraryError> {
        let definition_id = definition.id;
        let mut session = self.write_session()?;
        session
            .transact(
                vec![ProjectInvalidation::ModuleDefinition { definition_id }],
                |project| {
                    if project.module_definitions.contains_key(&definition_id) {
                        return Err(format!("Module definition {definition_id} already exists"));
                    }
                    project.module_definitions.insert(definition_id, definition);
                    Ok(())
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    /// Atomically creates a private definition and its sole Node Clip owner.
    pub fn create_private_module_item(
        &self,
        definition: ModuleDefinition,
        placement: ModuleItemPlacement,
    ) -> Result<(TimelineItemId, ModuleInstanceId, ChangeSet), LibraryError> {
        if !matches!(
            definition.sharing,
            crate::model::authoring::ModuleDefinitionSharing::Private
        ) {
            return Err(LibraryError::Validation(
                "A newly owned Node Clip definition must be Private".to_string(),
            ));
        }
        let mut session = self.write_session()?;
        let timeline_id = timeline_for_track(session.project(), placement.track_id)?;
        session
            .transact(
                vec![ProjectInvalidation::TimelineStructure { timeline_id }],
                |project| {
                    if project.module_definitions.contains_key(&definition.id) {
                        return Err(format!(
                            "Module definition {} already exists",
                            definition.id
                        ));
                    }
                    let definition_id = definition.id;
                    project.module_definitions.insert(definition_id, definition);
                    let instance_id = ModuleInstanceId::new();
                    project.module_instances.insert(
                        instance_id,
                        ModuleInstance {
                            id: instance_id,
                            definition_id,
                            parameter_overrides: placement.parameter_overrides,
                        },
                    );
                    let item_id = TimelineItemId::new();
                    project.items.insert(
                        item_id,
                        TimelineItem {
                            id: item_id,
                            track_id: placement.track_id,
                            name: placement.name,
                            source: SourceRef::Module(ModuleInvocation {
                                instance_id,
                                output_id: placement.output_id,
                                input_bindings: placement.input_bindings,
                                automation_tracks: HashMap::new(),
                            }),
                            interval: placement.interval,
                            time_map: TimeMap::default(),
                            layer: placement.layer,
                            parent: None,
                            blend_mode: BlendMode::Normal,
                            authored_properties: PropertyMap::new(),
                        },
                    );
                    place_item_at_layer(project, item_id, placement.track_id, placement.layer)?;
                    Ok((item_id, instance_id))
                },
            )
            .map(|((item_id, instance_id), changes)| (item_id, instance_id, changes))
            .map_err(LibraryError::Validation)
    }

    pub fn place_module_item(
        &self,
        definition_id: ModuleDefinitionId,
        placement: ModuleItemPlacement,
    ) -> Result<(TimelineItemId, ModuleInstanceId, ChangeSet), LibraryError> {
        let mut session = self.write_session()?;
        let timeline_id = timeline_for_track(session.project(), placement.track_id)?;
        session
            .transact(
                vec![ProjectInvalidation::TimelineStructure { timeline_id }],
                |project| {
                    if !project.module_definitions.contains_key(&definition_id) {
                        return Err(format!("Missing Module definition {definition_id}"));
                    }
                    let instance_id = ModuleInstanceId::new();
                    project.module_instances.insert(
                        instance_id,
                        ModuleInstance {
                            id: instance_id,
                            definition_id,
                            parameter_overrides: placement.parameter_overrides,
                        },
                    );
                    let item_id = TimelineItemId::new();
                    project.items.insert(
                        item_id,
                        TimelineItem {
                            id: item_id,
                            track_id: placement.track_id,
                            name: placement.name,
                            source: SourceRef::Module(ModuleInvocation {
                                instance_id,
                                output_id: placement.output_id,
                                input_bindings: placement.input_bindings,
                                automation_tracks: HashMap::new(),
                            }),
                            interval: placement.interval,
                            time_map: TimeMap::default(),
                            layer: placement.layer,
                            parent: None,
                            blend_mode: BlendMode::Normal,
                            authored_properties: PropertyMap::new(),
                        },
                    );
                    place_item_at_layer(project, item_id, placement.track_id, placement.layer)?;
                    Ok((item_id, instance_id))
                },
            )
            .map(|((item_id, instance_id), changes)| (item_id, instance_id, changes))
            .map_err(LibraryError::Validation)
    }

    pub fn bind_module_input(
        &self,
        item_id: TimelineItemId,
        input_id: PublishedMediaInputId,
        binding: MediaInputBinding,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let timeline_id = timeline_for_item(session.project(), item_id)?;
        session
            .transact(
                vec![ProjectInvalidation::Item {
                    timeline_id,
                    item_id,
                }],
                |project| {
                    item_module_invocation_mut(project, item_id)?
                        .input_bindings
                        .insert(input_id, binding);
                    Ok(())
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub fn unbind_module_input(
        &self,
        item_id: TimelineItemId,
        input_id: PublishedMediaInputId,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let timeline_id = timeline_for_item(session.project(), item_id)?;
        session
            .transact(
                vec![ProjectInvalidation::Item {
                    timeline_id,
                    item_id,
                }],
                |project| {
                    let removed = item_module_invocation_mut(project, item_id)?
                        .input_bindings
                        .remove(&input_id);
                    removed
                        .map(|_| ())
                        .ok_or_else(|| format!("Published media input {input_id} is not bound"))
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    /// Resolves the definition an ordinary instance edit may mutate. Reusable
    /// templates are cloned even when they currently have only one instance.
    pub fn prepare_instance_definition_for_edit(
        &self,
        instance_id: ModuleInstanceId,
    ) -> Result<PreparedModuleDefinitionEdit, LibraryError> {
        let mut session = self.write_session()?;
        let instance = session
            .project()
            .module_instances
            .get(&instance_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!("Missing Module instance {instance_id}"))
            })?;
        let definition = session
            .project()
            .module_definitions
            .get(&instance.definition_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!(
                    "Missing Module definition {}",
                    instance.definition_id
                ))
            })?;
        if matches!(
            definition.sharing,
            crate::model::authoring::ModuleDefinitionSharing::Private
        ) {
            return Ok(PreparedModuleDefinitionEdit {
                definition_id: definition.id,
                cloned: false,
                changes: None,
            });
        }
        let (definition_id, changes) = session
            .transact(
                vec![ProjectInvalidation::ModuleInstance { instance_id }],
                |project| private_definition_for_instance(project, instance_id),
            )
            .map_err(LibraryError::Validation)?;
        Ok(PreparedModuleDefinitionEdit {
            definition_id,
            cloned: true,
            changes: Some(changes),
        })
    }

    pub fn shared_definition_impact(
        &self,
        definition_id: ModuleDefinitionId,
    ) -> Result<usize, LibraryError> {
        reusable_definition_instance_count(self.read_session()?.project(), definition_id)
            .map_err(LibraryError::Validation)
    }

    pub fn add_instance_module_node(
        &self,
        instance_id: ModuleInstanceId,
        node: Node,
    ) -> Result<(uuid::Uuid, ModuleDefinitionId, ChangeSet), LibraryError> {
        self.edit_instance_definition(instance_id, |definition| {
            add_node_to_definition(definition, node)
        })
    }

    pub fn connect_instance_module_ports(
        &self,
        instance_id: ModuleInstanceId,
        from: crate::model::authoring::ModulePortAddress,
        to: crate::model::authoring::ModulePortAddress,
        order: i64,
    ) -> Result<(ModuleConnectionId, ModuleDefinitionId, ChangeSet), LibraryError> {
        self.edit_instance_definition(instance_id, |definition| {
            connect_definition_ports(definition, from, to, order)
        })
    }

    pub fn disconnect_instance_module_connection(
        &self,
        instance_id: ModuleInstanceId,
        connection_id: ModuleConnectionId,
    ) -> Result<(ModuleDefinitionId, ChangeSet), LibraryError> {
        self.edit_instance_definition(instance_id, |definition| {
            disconnect_definition_connection(definition, connection_id)
        })
        .map(|(_, definition_id, changes)| (definition_id, changes))
    }

    /// Moves either endpoint of one existing connection in a single
    /// copy-on-write transaction. Stable edge identity, input order and Blend
    /// metadata survive the edit; the complete candidate topology is
    /// validated before commit.
    pub fn reconnect_instance_module_connection(
        &self,
        instance_id: ModuleInstanceId,
        connection_id: ModuleConnectionId,
        from: crate::model::authoring::ModulePortAddress,
        to: crate::model::authoring::ModulePortAddress,
    ) -> Result<(ModuleDefinitionId, ChangeSet), LibraryError> {
        self.edit_instance_definition(instance_id, |definition| {
            reconnect_definition_connection(definition, connection_id, from, to)
        })
        .map(|(_, definition_id, changes)| (definition_id, changes))
    }

    pub fn set_instance_module_connection_blend_mode(
        &self,
        instance_id: ModuleInstanceId,
        connection_id: ModuleConnectionId,
        blend_mode: BlendMode,
    ) -> Result<(ModuleDefinitionId, ChangeSet), LibraryError> {
        self.edit_instance_definition(instance_id, |definition| {
            set_definition_connection_blend_mode(definition, connection_id, blend_mode)
        })
        .map(|(_, definition_id, changes)| (definition_id, changes))
    }

    pub fn set_instance_module_node_state(
        &self,
        instance_id: ModuleInstanceId,
        node_id: uuid::Uuid,
        name: String,
        enabled: bool,
        bypassed: bool,
    ) -> Result<(ModuleDefinitionId, ChangeSet), LibraryError> {
        self.edit_instance_definition(instance_id, |definition| {
            set_definition_node_state(definition, node_id, name, enabled, bypassed)
        })
        .map(|(_, definition_id, changes)| (definition_id, changes))
    }

    pub fn set_instance_module_node_presentation(
        &self,
        instance_id: ModuleInstanceId,
        node_id: uuid::Uuid,
        position: [f32; 2],
        size: [f32; 2],
        collapsed: bool,
    ) -> Result<(ModuleDefinitionId, ChangeSet), LibraryError> {
        self.edit_instance_definition_presentation(instance_id, |definition| {
            set_definition_node_presentation(definition, node_id, position, size, collapsed)
        })
        .map(|(_, definition_id, changes)| (definition_id, changes))
    }

    /// Applies a complete layout operation as one undoable presentation edit.
    /// Layout belongs to the Module document and does not detach one reusable
    /// instance or invalidate executable topology.
    pub fn set_instance_module_node_presentations(
        &self,
        instance_id: ModuleInstanceId,
        updates: Vec<ModuleNodePresentationUpdate>,
    ) -> Result<(ModuleDefinitionId, ChangeSet), LibraryError> {
        self.edit_instance_definition_presentation(instance_id, |definition| {
            set_definition_node_presentations(definition, &updates)
        })
        .map(|(_, definition_id, changes)| (definition_id, changes))
    }

    pub fn set_instance_module_node_property(
        &self,
        instance_id: ModuleInstanceId,
        node_id: uuid::Uuid,
        key: String,
        property: Property,
    ) -> Result<(ModuleDefinitionId, ChangeSet), LibraryError> {
        self.edit_instance_definition(instance_id, |definition| {
            set_definition_node_property(definition, node_id, key, property)
        })
        .map(|(_, definition_id, changes)| (definition_id, changes))
    }

    /// Explicit shared-template edit. Call `shared_definition_impact` before
    /// confirmation in UI; the result repeats the affected instance count.
    pub fn add_shared_module_node(
        &self,
        definition_id: ModuleDefinitionId,
        node: Node,
    ) -> Result<SharedModuleEdit<uuid::Uuid>, LibraryError> {
        self.edit_shared_definition(definition_id, |definition| {
            add_node_to_definition(definition, node)
        })
    }

    pub fn connect_shared_module_ports(
        &self,
        definition_id: ModuleDefinitionId,
        from: crate::model::authoring::ModulePortAddress,
        to: crate::model::authoring::ModulePortAddress,
        order: i64,
    ) -> Result<SharedModuleEdit<ModuleConnectionId>, LibraryError> {
        self.edit_shared_definition(definition_id, |definition| {
            connect_definition_ports(definition, from, to, order)
        })
    }

    pub fn disconnect_shared_module_connection(
        &self,
        definition_id: ModuleDefinitionId,
        connection_id: ModuleConnectionId,
    ) -> Result<SharedModuleEdit<()>, LibraryError> {
        self.edit_shared_definition(definition_id, |definition| {
            disconnect_definition_connection(definition, connection_id)
        })
    }

    pub fn set_shared_module_connection_blend_mode(
        &self,
        definition_id: ModuleDefinitionId,
        connection_id: ModuleConnectionId,
        blend_mode: BlendMode,
    ) -> Result<SharedModuleEdit<()>, LibraryError> {
        self.edit_shared_definition(definition_id, |definition| {
            set_definition_connection_blend_mode(definition, connection_id, blend_mode)
        })
    }

    pub fn set_shared_module_node_state(
        &self,
        definition_id: ModuleDefinitionId,
        node_id: uuid::Uuid,
        name: String,
        enabled: bool,
        bypassed: bool,
    ) -> Result<SharedModuleEdit<()>, LibraryError> {
        self.edit_shared_definition(definition_id, |definition| {
            set_definition_node_state(definition, node_id, name, enabled, bypassed)
        })
    }

    pub fn set_shared_module_node_presentation(
        &self,
        definition_id: ModuleDefinitionId,
        node_id: uuid::Uuid,
        position: [f32; 2],
        size: [f32; 2],
        collapsed: bool,
    ) -> Result<SharedModuleEdit<()>, LibraryError> {
        self.edit_shared_definition_presentation(definition_id, |definition| {
            set_definition_node_presentation(definition, node_id, position, size, collapsed)
        })
    }

    pub fn set_shared_module_node_property(
        &self,
        definition_id: ModuleDefinitionId,
        node_id: uuid::Uuid,
        key: String,
        property: Property,
    ) -> Result<SharedModuleEdit<()>, LibraryError> {
        self.edit_shared_definition(definition_id, |definition| {
            set_definition_node_property(definition, node_id, key, property)
        })
    }

    pub(super) fn edit_instance_definition<T>(
        &self,
        instance_id: ModuleInstanceId,
        edit: impl FnOnce(&mut ModuleDefinition) -> Result<T, String>,
    ) -> Result<(T, ModuleDefinitionId, ChangeSet), LibraryError> {
        let mut session = self.write_session()?;
        session
            .transact(
                vec![ProjectInvalidation::ModuleInstance { instance_id }],
                |project| {
                    let definition_id = private_definition_for_instance(project, instance_id)?;
                    let value = edit(module_definition_mut(project, definition_id)?)?;
                    Ok((value, definition_id))
                },
            )
            .map(|((value, definition_id), changes)| (value, definition_id, changes))
            .map_err(LibraryError::Validation)
    }

    fn edit_instance_definition_presentation<T>(
        &self,
        instance_id: ModuleInstanceId,
        edit: impl FnOnce(&mut ModuleDefinition) -> Result<T, String>,
    ) -> Result<(T, ModuleDefinitionId, ChangeSet), LibraryError> {
        let mut session = self.write_session()?;
        session
            .transact(Vec::new(), |project| {
                let definition_id = project
                    .module_instances
                    .get(&instance_id)
                    .ok_or_else(|| format!("Missing Module instance {instance_id}"))?
                    .definition_id;
                let value = edit(module_definition_mut(project, definition_id)?)?;
                Ok((value, definition_id))
            })
            .map(|((value, definition_id), changes)| (value, definition_id, changes))
            .map_err(LibraryError::Validation)
    }

    pub(super) fn edit_shared_definition<T>(
        &self,
        definition_id: ModuleDefinitionId,
        edit: impl FnOnce(&mut ModuleDefinition) -> Result<T, String>,
    ) -> Result<SharedModuleEdit<T>, LibraryError> {
        let mut session = self.write_session()?;
        let affected_instance_count =
            reusable_definition_instance_count(session.project(), definition_id)
                .map_err(LibraryError::Validation)?;
        let (value, changes) = session
            .transact(
                vec![ProjectInvalidation::ModuleDefinition { definition_id }],
                |project| edit(module_definition_mut(project, definition_id)?),
            )
            .map_err(LibraryError::Validation)?;
        Ok(SharedModuleEdit {
            value,
            affected_instance_count,
            changes,
        })
    }

    fn edit_shared_definition_presentation<T>(
        &self,
        definition_id: ModuleDefinitionId,
        edit: impl FnOnce(&mut ModuleDefinition) -> Result<T, String>,
    ) -> Result<SharedModuleEdit<T>, LibraryError> {
        let mut session = self.write_session()?;
        let affected_instance_count =
            reusable_definition_instance_count(session.project(), definition_id)
                .map_err(LibraryError::Validation)?;
        let (value, changes) = session
            .transact(Vec::new(), |project| {
                edit(module_definition_mut(project, definition_id)?)
            })
            .map_err(LibraryError::Validation)?;
        Ok(SharedModuleEdit {
            value,
            affected_instance_count,
            changes,
        })
    }
}

fn item_module_invocation_mut(
    project: &mut AuthoringProject,
    item_id: TimelineItemId,
) -> Result<&mut ModuleInvocation, String> {
    let item = project
        .items
        .get_mut(&item_id)
        .ok_or_else(|| format!("Missing Timeline item {item_id}"))?;
    let SourceRef::Module(invocation) = &mut item.source else {
        return Err(format!("Timeline item {item_id} is not a Node Clip"));
    };
    Ok(invocation)
}

pub(super) fn module_definition_mut(
    project: &mut AuthoringProject,
    definition_id: ModuleDefinitionId,
) -> Result<&mut ModuleDefinition, String> {
    project
        .module_definitions
        .get_mut(&definition_id)
        .ok_or_else(|| format!("Missing Module definition {definition_id}"))
}

fn reusable_definition_instance_count(
    project: &AuthoringProject,
    definition_id: ModuleDefinitionId,
) -> Result<usize, String> {
    let definition = project
        .module_definitions
        .get(&definition_id)
        .ok_or_else(|| format!("Missing Module definition {definition_id}"))?;
    if !matches!(
        definition.sharing,
        crate::model::authoring::ModuleDefinitionSharing::ReusableTemplate(_)
    ) {
        return Err(format!(
            "Module definition {definition_id} is private; edit its instance instead"
        ));
    }
    Ok(project
        .module_instances
        .values()
        .filter(|instance| instance.definition_id == definition_id)
        .count())
}

pub(super) fn private_definition_for_instance(
    project: &mut AuthoringProject,
    instance_id: ModuleInstanceId,
) -> Result<ModuleDefinitionId, String> {
    let definition_id = project
        .module_instances
        .get(&instance_id)
        .ok_or_else(|| format!("Missing Module instance {instance_id}"))?
        .definition_id;
    let definition = project
        .module_definitions
        .get(&definition_id)
        .cloned()
        .ok_or_else(|| format!("Missing Module definition {definition_id}"))?;
    let make_remaining_local_private = matches!(
        definition.sharing,
        crate::model::authoring::ModuleDefinitionSharing::SharedLocal
    ) && project
        .module_instances
        .values()
        .filter(|instance| instance.definition_id == definition_id)
        .count()
        == 2;
    match definition.sharing {
        crate::model::authoring::ModuleDefinitionSharing::Private => {
            return Ok(definition_id);
        }
        crate::model::authoring::ModuleDefinitionSharing::SharedLocal => {
            let instance_count = project
                .module_instances
                .values()
                .filter(|instance| instance.definition_id == definition_id)
                .count();
            if instance_count == 1 {
                project
                    .module_definitions
                    .get_mut(&definition_id)
                    .ok_or_else(|| format!("Missing Module definition {definition_id}"))?
                    .sharing = crate::model::authoring::ModuleDefinitionSharing::Private;
                return Ok(definition_id);
            }
        }
        crate::model::authoring::ModuleDefinitionSharing::ReusableTemplate(_) => {}
    }
    let private_id = ModuleDefinitionId::new();
    project.module_definitions.insert(
        private_id,
        ModuleDefinition {
            id: private_id,
            name: format!("{} (Instance)", definition.name),
            sharing: crate::model::authoring::ModuleDefinitionSharing::Private,
            ..definition
        },
    );
    project
        .module_instances
        .get_mut(&instance_id)
        .ok_or_else(|| format!("Missing Module instance {instance_id}"))?
        .definition_id = private_id;
    if make_remaining_local_private {
        project
            .module_definitions
            .get_mut(&definition_id)
            .ok_or_else(|| format!("Missing Module definition {definition_id}"))?
            .sharing = crate::model::authoring::ModuleDefinitionSharing::Private;
    }
    Ok(private_id)
}

pub(super) fn remove_instance_and_private_definition(
    project: &mut AuthoringProject,
    instance_id: ModuleInstanceId,
) {
    let definition_id = project
        .module_instances
        .remove(&instance_id)
        .map(|instance| instance.definition_id);
    let Some(definition_id) = definition_id else {
        return;
    };
    let sharing = project
        .module_definitions
        .get(&definition_id)
        .map(|definition| definition.sharing.clone());
    let remaining = project
        .module_instances
        .values()
        .filter(|instance| instance.definition_id == definition_id)
        .count();
    match (sharing, remaining) {
        (Some(crate::model::authoring::ModuleDefinitionSharing::Private), _)
        | (Some(crate::model::authoring::ModuleDefinitionSharing::SharedLocal), 0) => {
            project.module_definitions.remove(&definition_id);
        }
        (Some(crate::model::authoring::ModuleDefinitionSharing::SharedLocal), 1) => {
            if let Some(definition) = project.module_definitions.get_mut(&definition_id) {
                definition.sharing = crate::model::authoring::ModuleDefinitionSharing::Private;
            }
        }
        _ => {}
    }
}

pub(super) fn add_node_to_definition(
    definition: &mut ModuleDefinition,
    node: Node,
) -> Result<uuid::Uuid, String> {
    require_insertable_processing_node(&node)?;
    definition
        .host_contract
        .validate_authored_processing_node(&node)?;
    let node_id = node.id;
    if definition.graph.nodes.insert(node_id, node).is_some() {
        return Err(format!("Module Node {node_id} already exists"));
    }
    bump_topology_revision(definition)?;
    Ok(node_id)
}

fn set_definition_node_state(
    definition: &mut ModuleDefinition,
    node_id: uuid::Uuid,
    name: String,
    enabled: bool,
    bypassed: bool,
) -> Result<(), String> {
    let node = definition
        .graph
        .nodes
        .get_mut(&node_id)
        .ok_or_else(|| format!("Missing Module Node {node_id}"))?;
    require_output_state(node, enabled, bypassed)?;
    node.name = name;
    node.enabled = enabled;
    node.bypassed = bypassed;
    bump_topology_revision(definition)
}

fn set_definition_node_presentation(
    definition: &mut ModuleDefinition,
    node_id: uuid::Uuid,
    position: [f32; 2],
    size: [f32; 2],
    collapsed: bool,
) -> Result<(), String> {
    set_definition_node_presentations(
        definition,
        &[ModuleNodePresentationUpdate {
            node_id,
            position,
            size,
            collapsed,
        }],
    )
}

fn set_definition_node_presentations(
    definition: &mut ModuleDefinition,
    updates: &[ModuleNodePresentationUpdate],
) -> Result<(), String> {
    if updates.is_empty() {
        return Err("Module Node presentation update must not be empty".to_string());
    }
    let mut node_ids = std::collections::HashSet::with_capacity(updates.len());
    for update in updates {
        if !update.position.into_iter().all(f32::is_finite)
            || !update
                .size
                .into_iter()
                .all(|component| component.is_finite() && component > 0.0)
        {
            return Err(
                "Module Node presentation must be finite and have positive size".to_string(),
            );
        }
        if !node_ids.insert(update.node_id) {
            return Err(format!(
                "Duplicate Module Node presentation update {}",
                update.node_id
            ));
        }
        if !definition.graph.nodes.contains_key(&update.node_id) {
            return Err(format!("Missing Module Node {}", update.node_id));
        }
    }
    for update in updates {
        let node = definition
            .graph
            .nodes
            .get_mut(&update.node_id)
            .ok_or_else(|| format!("Missing Module Node {}", update.node_id))?;
        node.ui_position = update.position;
        node.ui_size = update.size;
        node.ui_collapsed = update.collapsed;
    }
    Ok(())
}

fn set_definition_node_property(
    definition: &mut ModuleDefinition,
    node_id: uuid::Uuid,
    key: String,
    property: Property,
) -> Result<(), String> {
    if definition.is_protected_host_boundary_node(node_id) {
        return Err(format!(
            "Transition Module Node {node_id} is a protected host boundary; its value is supplied by the Timeline"
        ));
    }
    definition
        .graph
        .nodes
        .get_mut(&node_id)
        .ok_or_else(|| format!("Missing Module Node {node_id}"))?
        .set_property(key, property)?;
    bump_topology_revision(definition)
}

pub(super) fn bump_topology_revision(definition: &mut ModuleDefinition) -> Result<(), String> {
    definition.topology_revision = definition
        .topology_revision
        .checked_add(1)
        .ok_or_else(|| "Module topology revision overflow".to_string())?;
    Ok(())
}

pub(super) fn bump_interface_version(definition: &mut ModuleDefinition) -> Result<(), String> {
    definition.interface_version = definition
        .interface_version
        .checked_add(1)
        .ok_or_else(|| "Module interface version overflow".to_string())?;
    Ok(())
}
