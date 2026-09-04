//! Instance values and Timeline-owned automation for published Module parameters.

use super::super::*;
use super::item_module_invocation_mut;

impl TimelineEditorService {
    pub fn set_module_parameter(
        &self,
        instance_id: ModuleInstanceId,
        parameter_id: PublishedParameterId,
        value: PropertyValue,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        session
            .transact(
                vec![ProjectInvalidation::ModuleInstance { instance_id }],
                |project| {
                    project
                        .module_instances
                        .get_mut(&instance_id)
                        .ok_or_else(|| format!("Missing Module instance {instance_id}"))?
                        .parameter_overrides
                        .insert(parameter_id, value);
                    Ok(())
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    /// Switches a published parameter back to its instance-local constant in
    /// one undoable edit. The Timeline automation and instance value cannot be
    /// left disagreeing across two user-visible transactions.
    pub fn set_module_parameter_constant(
        &self,
        item_id: TimelineItemId,
        parameter_id: PublishedParameterId,
        value: PropertyValue,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let timeline_id = timeline_for_item(session.project(), item_id)?;
        let instance_id = session
            .project()
            .items
            .get(&item_id)
            .and_then(|item| match &item.source {
                SourceRef::Module(invocation) => Some(invocation.instance_id),
                _ => None,
            })
            .ok_or_else(|| {
                LibraryError::Validation(format!("Timeline item {item_id} is not a Node Clip"))
            })?;
        session
            .transact(
                vec![
                    ProjectInvalidation::Item {
                        timeline_id,
                        item_id,
                    },
                    ProjectInvalidation::ModuleInstance { instance_id },
                ],
                |project| {
                    let authored_instance_id = {
                        let invocation = item_module_invocation_mut(project, item_id)?;
                        invocation.automation_tracks.remove(&parameter_id);
                        invocation.instance_id
                    };
                    if authored_instance_id != instance_id {
                        return Err(format!(
                            "Node Clip {item_id} changed Module instance during the edit"
                        ));
                    }
                    project
                        .module_instances
                        .get_mut(&instance_id)
                        .ok_or_else(|| format!("Missing Module instance {instance_id}"))?
                        .parameter_overrides
                        .insert(parameter_id, value);
                    Ok(())
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    /// Removes one instance-local value so the invocation follows the
    /// definition's published default again. This never mutates Module
    /// topology or another instance.
    pub fn clear_module_parameter_override(
        &self,
        instance_id: ModuleInstanceId,
        parameter_id: PublishedParameterId,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        session
            .transact(
                vec![ProjectInvalidation::ModuleInstance { instance_id }],
                |project| {
                    let instance = project
                        .module_instances
                        .get_mut(&instance_id)
                        .ok_or_else(|| format!("Missing Module instance {instance_id}"))?;
                    instance
                        .parameter_overrides
                        .remove(&parameter_id)
                        .map(|_| ())
                        .ok_or_else(|| {
                            format!(
                                "Module instance {instance_id} has no override for {parameter_id}"
                            )
                        })
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub fn upsert_module_parameter_keyframe(
        &self,
        item_id: TimelineItemId,
        parameter_id: PublishedParameterId,
        local_time: MediaTime,
        value: PropertyValue,
        easing: Option<EasingFunction>,
    ) -> Result<(KeyframeId, ChangeSet), LibraryError> {
        let mut session = self.write_session()?;
        let timeline_id = timeline_for_item(session.project(), item_id)?;
        session
            .transact(
                vec![ProjectInvalidation::Item {
                    timeline_id,
                    item_id,
                }],
                |project| {
                    let invocation = item_module_invocation_mut(project, item_id)?;
                    let track = invocation
                        .automation_tracks
                        .entry(parameter_id)
                        .or_insert_with(|| AutomationTrack {
                            keyframes: Vec::new(),
                        });
                    track.upsert(local_time, value, easing)
                },
            )
            .map_err(LibraryError::Validation)
    }

    pub fn update_module_parameter_keyframe(
        &self,
        item_id: TimelineItemId,
        parameter_id: PublishedParameterId,
        keyframe_id: KeyframeId,
        update: AuthoringKeyframeUpdate,
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
                    let track = item_module_invocation_mut(project, item_id)?
                        .automation_tracks
                        .get_mut(&parameter_id)
                        .ok_or_else(|| {
                            format!("Missing automation for Published parameter {parameter_id}")
                        })?;
                    track.update_keyframe(keyframe_id, update.time, update.value, update.easing)
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub fn remove_module_parameter_keyframe(
        &self,
        item_id: TimelineItemId,
        parameter_id: PublishedParameterId,
        keyframe_id: KeyframeId,
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
                    let invocation = item_module_invocation_mut(project, item_id)?;
                    let remove_track = {
                        let track = invocation
                            .automation_tracks
                            .get_mut(&parameter_id)
                            .ok_or_else(|| {
                                format!("Missing automation for Published parameter {parameter_id}")
                            })?;
                        if !track.remove_keyframe(keyframe_id) {
                            return Err(format!("Missing Automation Keyframe {keyframe_id}"));
                        }
                        track.keyframes.is_empty()
                    };
                    if remove_track {
                        invocation.automation_tracks.remove(&parameter_id);
                    }
                    Ok(())
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::ModuleNodeRequest;
    use crate::model::authoring::{
        ModuleConnection, ModuleConnectionId, ModuleDefinitionSharing, ModulePortAddress,
        PublishedParameter,
    };
    use crate::model::project::{IMAGE_OUTPUT_PORT, PortDataType};

    #[test]
    fn switching_module_automation_to_constant_is_one_undoable_edit() {
        let plugins = PluginManager::default();
        let service = TimelineEditorService::create_default("Parameter mode").expect("service");
        let project = service.snapshot().expect("project");
        let track_id = project.timelines[&project.root_timeline_id].track_order[0];
        drop(project);

        let node = service
            .create_module_node(
                &plugins,
                ModuleNodeRequest::Solid {
                    color: Color::white(),
                },
                1920,
                1080,
            )
            .expect("Solid Node");
        let node_id = node.id;
        let default_value = node
            .properties()
            .get_constant_value("color")
            .cloned()
            .expect("Solid color");
        let parameter_id = PublishedParameterId::new();
        let (mut definition, output_id) =
            ModuleDefinition::new_image("Solid", ModuleDefinitionSharing::Private);
        let output_target = definition
            .output(output_id)
            .expect("Output")
            .target(PortDataType::Image)
            .expect("Image input");
        definition.graph.nodes.insert(node_id, node);
        definition.graph.connections.push(ModuleConnection {
            id: ModuleConnectionId::new(),
            from: ModulePortAddress {
                node_id,
                port: IMAGE_OUTPUT_PORT.to_string(),
            },
            to: output_target,
            order: 0,
            blend_mode: BlendMode::Normal,
        });
        definition.interface.parameters.push(PublishedParameter {
            id: parameter_id,
            name: "Color".to_string(),
            data_type: PortDataType::Color,
            default_value: default_value.clone(),
            target: ModulePortAddress {
                node_id,
                port: "color".to_string(),
            },
        });
        definition.topology_revision += 1;
        let (item_id, instance_id, _) = service
            .create_private_module_item(
                definition,
                ModuleItemPlacement {
                    track_id,
                    name: "Solid".to_string(),
                    output_id,
                    interval: TimelineInterval::new(
                        MediaTime::zero(),
                        MediaTime::new(5, 1).expect("duration"),
                    )
                    .expect("interval"),
                    layer: 0,
                    parameter_overrides: HashMap::new(),
                    input_bindings: HashMap::new(),
                },
            )
            .expect("Node Clip");
        service
            .upsert_module_parameter_keyframe(
                item_id,
                parameter_id,
                MediaTime::new(1, 1).expect("time"),
                default_value.clone(),
                None,
            )
            .expect("keyframe");
        let before = service.snapshot().expect("automated");
        let revision = service.revision().expect("revision");
        let constant = default_value;

        let change = service
            .set_module_parameter_constant(item_id, parameter_id, constant.clone())
            .expect("constant");
        assert_eq!(change.revision.get(), revision.get() + 1);
        assert!(change.invalidations.contains(&ProjectInvalidation::Item {
            timeline_id: service.snapshot().expect("project").root_timeline_id,
            item_id,
        }));
        assert!(
            change
                .invalidations
                .contains(&ProjectInvalidation::ModuleInstance { instance_id })
        );
        let changed = service.snapshot().expect("changed");
        let SourceRef::Module(invocation) = &changed.items[&item_id].source else {
            panic!("expected Module item");
        };
        assert!(!invocation.automation_tracks.contains_key(&parameter_id));
        assert_eq!(
            changed.module_instances[&instance_id]
                .parameter_overrides
                .get(&parameter_id),
            Some(&constant)
        );
        drop(changed);

        service.undo().expect("undo").expect("change");
        assert_eq!(
            service.snapshot().expect("restored").as_ref(),
            before.as_ref()
        );
    }
}
