use super::*;

impl TimelineEditorService {
    /// Atomically creates a private Module definition and attaches its sole
    /// instance to an authored evaluation stage.
    pub fn create_private_module_attachment(
        &self,
        definition: ModuleDefinition,
        placement: ModuleAttachmentPlacement,
    ) -> Result<(AttachmentId, ModuleInstanceId, ChangeSet), LibraryError> {
        if !matches!(
            definition.sharing,
            crate::model::authoring::ModuleDefinitionSharing::Private
        ) {
            return Err(LibraryError::Validation(
                "A newly owned Module Attachment definition must be Private".to_string(),
            ));
        }
        if placement.definition_id != definition.id {
            return Err(LibraryError::Validation(
                "Module Attachment placement must reference the supplied definition".to_string(),
            ));
        }

        let mut session = self.write_session()?;
        let invalidations = owner_invalidations(session.project(), &placement.owner)?;
        session
            .transact(invalidations, |project| {
                if project.module_definitions.contains_key(&definition.id) {
                    return Err(format!(
                        "Module definition {} already exists",
                        definition.id
                    ));
                }
                let order = project
                    .attachments
                    .values()
                    .filter(|attachment| {
                        attachment.owner == placement.owner && attachment.stage == placement.stage
                    })
                    .count();
                let order = i64::try_from(order)
                    .map_err(|_| "Attachment stack is too large".to_string())?;
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
                let attachment_id = AttachmentId::new();
                project.attachments.insert(
                    attachment_id,
                    Attachment {
                        id: attachment_id,
                        owner: placement.owner,
                        stage: placement.stage,
                        order,
                        enabled: true,
                        bypassed: false,
                        processor: AttachmentProcessor::Module(ModuleInvocation {
                            instance_id,
                            output_id: placement.output_id,
                            input_bindings: placement.input_bindings,
                            automation_tracks: HashMap::new(),
                        }),
                    },
                );
                Ok((attachment_id, instance_id))
            })
            .map(|((attachment_id, instance_id), changes)| (attachment_id, instance_id, changes))
            .map_err(LibraryError::Validation)
    }

    pub fn add_builtin_effect_by_id(
        &self,
        plugins: &PluginManager,
        owner: AttachmentOwner,
        stage: AttachmentStage,
        effect_id: &str,
    ) -> Result<(AttachmentId, ChangeSet), LibraryError> {
        let effect = self.create_builtin_effect(plugins, effect_id)?;
        self.add_builtin_attachment(owner, stage, effect)
    }

    pub fn add_builtin_attachment(
        &self,
        owner: AttachmentOwner,
        stage: AttachmentStage,
        effect: BuiltinEffectInstance,
    ) -> Result<(AttachmentId, ChangeSet), LibraryError> {
        let mut session = self.write_session()?;
        let invalidations = owner_invalidations(session.project(), &owner)?;
        session
            .transact(invalidations, |project| {
                let order = project
                    .attachments
                    .values()
                    .filter(|attachment| attachment.owner == owner && attachment.stage == stage)
                    .count();
                let order = i64::try_from(order)
                    .map_err(|_| "Attachment stack is too large".to_string())?;
                let attachment_id = AttachmentId::new();
                project.attachments.insert(
                    attachment_id,
                    Attachment {
                        id: attachment_id,
                        owner,
                        stage,
                        order,
                        enabled: true,
                        bypassed: false,
                        processor: AttachmentProcessor::BuiltinEffect(effect),
                    },
                );
                Ok(attachment_id)
            })
            .map_err(LibraryError::Validation)
    }

    pub fn attach_module(
        &self,
        placement: ModuleAttachmentPlacement,
    ) -> Result<(AttachmentId, ModuleInstanceId, ChangeSet), LibraryError> {
        let mut session = self.write_session()?;
        let invalidations = owner_invalidations(session.project(), &placement.owner)?;
        session
            .transact(invalidations, |project| {
                let order = project
                    .attachments
                    .values()
                    .filter(|attachment| {
                        attachment.owner == placement.owner && attachment.stage == placement.stage
                    })
                    .count();
                let order = i64::try_from(order)
                    .map_err(|_| "Attachment stack is too large".to_string())?;
                let instance_id = ModuleInstanceId::new();
                project.module_instances.insert(
                    instance_id,
                    ModuleInstance {
                        id: instance_id,
                        definition_id: placement.definition_id,
                        parameter_overrides: placement.parameter_overrides,
                    },
                );
                let attachment_id = AttachmentId::new();
                project.attachments.insert(
                    attachment_id,
                    Attachment {
                        id: attachment_id,
                        owner: placement.owner,
                        stage: placement.stage,
                        order,
                        enabled: true,
                        bypassed: false,
                        processor: AttachmentProcessor::Module(ModuleInvocation {
                            instance_id,
                            output_id: placement.output_id,
                            input_bindings: placement.input_bindings,
                            automation_tracks: HashMap::new(),
                        }),
                    },
                );
                Ok((attachment_id, instance_id))
            })
            .map(|((attachment_id, instance_id), changes)| (attachment_id, instance_id, changes))
            .map_err(LibraryError::Validation)
    }

    /// Binds an additional public media input of a Module Effect to a clip
    /// output. The binding addresses only the stable published input ID;
    /// Module-internal Node IDs never escape into Timeline state.
    pub fn bind_attachment_module_input(
        &self,
        attachment_id: AttachmentId,
        input_id: PublishedMediaInputId,
        binding: MediaInputBinding,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let owner = attachment_owner(session.project(), attachment_id)?;
        let invalidations = owner_invalidations(session.project(), &owner)?;
        session
            .transact(invalidations, |project| {
                attachment_module_invocation_mut(project, attachment_id)?
                    .input_bindings
                    .insert(input_id, binding);
                Ok(())
            })
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub fn unbind_attachment_module_input(
        &self,
        attachment_id: AttachmentId,
        input_id: PublishedMediaInputId,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let owner = attachment_owner(session.project(), attachment_id)?;
        let invalidations = owner_invalidations(session.project(), &owner)?;
        session
            .transact(invalidations, |project| {
                attachment_module_invocation_mut(project, attachment_id)?
                    .input_bindings
                    .remove(&input_id)
                    .map(|_| ())
                    .ok_or_else(|| format!("Published media input {input_id} is not bound"))
            })
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub fn set_builtin_effect_parameter(
        &self,
        attachment_id: AttachmentId,
        key: &str,
        value: PropertyValue,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let owner = session
            .project()
            .attachments
            .get(&attachment_id)
            .ok_or_else(|| LibraryError::Validation(format!("Missing Attachment {attachment_id}")))?
            .owner
            .clone();
        let invalidations = owner_invalidations(session.project(), &owner)?;
        session
            .transact(invalidations, |project| {
                let attachment = project
                    .attachments
                    .get_mut(&attachment_id)
                    .ok_or_else(|| format!("Missing Attachment {attachment_id}"))?;
                let AttachmentProcessor::BuiltinEffect(effect) = &mut attachment.processor else {
                    return Err("Attachment is not a built-in Effect".to_string());
                };
                effect
                    .parameters
                    .get_mut(key)
                    .ok_or_else(|| format!("Built-in Effect has no parameter '{key}'"))?
                    .value = value;
                Ok(())
            })
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    /// Switches a built-in Effect parameter back to a constant in one
    /// undoable edit. Clearing automation and storing its replacement value are
    /// one model operation so Undo never exposes a half-switched mode.
    pub fn set_builtin_effect_parameter_constant(
        &self,
        attachment_id: AttachmentId,
        key: &str,
        value: PropertyValue,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let owner = attachment_owner(session.project(), attachment_id)?;
        let invalidations = owner_invalidations(session.project(), &owner)?;
        session
            .transact(invalidations, |project| {
                let parameter = builtin_effect_parameter_mut(project, attachment_id, key)?;
                parameter.value = value;
                parameter.automation = None;
                Ok(())
            })
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub fn upsert_builtin_effect_parameter_keyframe(
        &self,
        attachment_id: AttachmentId,
        key: &str,
        local_time: MediaTime,
        value: PropertyValue,
        easing: Option<EasingFunction>,
    ) -> Result<(KeyframeId, ChangeSet), LibraryError> {
        let mut session = self.write_session()?;
        let owner = attachment_owner(session.project(), attachment_id)?;
        let invalidations = owner_invalidations(session.project(), &owner)?;
        session
            .transact(invalidations, |project| {
                let parameter = builtin_effect_parameter_mut(project, attachment_id, key)?;
                let track = parameter.automation.get_or_insert_with(|| AutomationTrack {
                    keyframes: Vec::new(),
                });
                track.upsert(local_time, value, easing)
            })
            .map_err(LibraryError::Validation)
    }

    pub fn update_builtin_effect_parameter_keyframe(
        &self,
        attachment_id: AttachmentId,
        key: &str,
        keyframe_id: KeyframeId,
        update: AuthoringKeyframeUpdate,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let owner = attachment_owner(session.project(), attachment_id)?;
        let invalidations = owner_invalidations(session.project(), &owner)?;
        session
            .transact(invalidations, |project| {
                builtin_effect_parameter_mut(project, attachment_id, key)?
                    .automation
                    .as_mut()
                    .ok_or_else(|| format!("Effect parameter '{key}' has no Automation"))?
                    .update_keyframe(keyframe_id, update.time, update.value, update.easing)
            })
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub fn remove_builtin_effect_parameter_keyframe(
        &self,
        attachment_id: AttachmentId,
        key: &str,
        keyframe_id: KeyframeId,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let owner = attachment_owner(session.project(), attachment_id)?;
        let invalidations = owner_invalidations(session.project(), &owner)?;
        session
            .transact(invalidations, |project| {
                let parameter = builtin_effect_parameter_mut(project, attachment_id, key)?;
                let remove_track = {
                    let track = parameter
                        .automation
                        .as_mut()
                        .ok_or_else(|| format!("Effect parameter '{key}' has no Automation"))?;
                    if !track.remove_keyframe(keyframe_id) {
                        return Err(format!("Missing Automation Keyframe {keyframe_id}"));
                    }
                    track.keyframes.is_empty()
                };
                if remove_track {
                    parameter.automation = None;
                }
                Ok(())
            })
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub fn set_attachment_state(
        &self,
        attachment_id: AttachmentId,
        enabled: bool,
        bypassed: bool,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let owner = attachment_owner(session.project(), attachment_id)?;
        let invalidations = owner_invalidations(session.project(), &owner)?;
        session
            .transact(invalidations, |project| {
                let attachment = project
                    .attachments
                    .get_mut(&attachment_id)
                    .ok_or_else(|| format!("Missing Attachment {attachment_id}"))?;
                attachment.enabled = enabled;
                attachment.bypassed = bypassed;
                Ok(())
            })
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub fn remove_attachment(
        &self,
        attachment_id: AttachmentId,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let attachment = session
            .project()
            .attachments
            .get(&attachment_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!("Missing Attachment {attachment_id}"))
            })?;
        let owner = attachment.owner.clone();
        let stage = attachment.stage;
        let invalidations = owner_invalidations(session.project(), &owner)?;
        session
            .transact(invalidations, |project| {
                let removed = project
                    .attachments
                    .remove(&attachment_id)
                    .ok_or_else(|| format!("Missing Attachment {attachment_id}"))?;
                if let AttachmentProcessor::Module(invocation) = removed.processor {
                    remove_instance_and_private_definition(project, invocation.instance_id);
                }
                normalize_attachment_order(project, &owner, stage);
                Ok(())
            })
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    /// Moves one entry within its owner/stage stack. `new_index` is evaluated
    /// against the complete stack and the resulting order is contiguous.
    pub fn reorder_attachment(
        &self,
        attachment_id: AttachmentId,
        new_index: usize,
    ) -> Result<ChangeSet, LibraryError> {
        let stage = self
            .read_session()?
            .project()
            .attachments
            .get(&attachment_id)
            .ok_or_else(|| LibraryError::Validation(format!("Missing Attachment {attachment_id}")))?
            .stage;
        self.move_attachment(attachment_id, stage, new_index)
    }

    /// Atomically moves an Effect to a destination evaluation stage and
    /// insertion slot. `target_index` addresses the destination stack after
    /// the moving entry has been removed, so both same-stage reordering and
    /// cross-stage insertion use the same unambiguous contract.
    ///
    /// Owner/stage compatibility and the processor media contract are checked
    /// by the authoritative Project validation before the transaction commits.
    /// Source and destination orders are normalized in this same undo step.
    pub fn move_attachment(
        &self,
        attachment_id: AttachmentId,
        target_stage: AttachmentStage,
        target_index: usize,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let attachment = session
            .project()
            .attachments
            .get(&attachment_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!("Missing Attachment {attachment_id}"))
            })?;
        let owner = attachment.owner.clone();
        let source_stage = attachment.stage;
        let destination_len = session
            .project()
            .attachments
            .values()
            .filter(|candidate| {
                candidate.owner == owner
                    && candidate.stage == target_stage
                    && candidate.id != attachment_id
            })
            .count();
        if target_index > destination_len {
            return Err(LibraryError::Validation(format!(
                "Attachment insertion index {target_index} is outside destination stack of length {destination_len}"
            )));
        }
        let invalidations = owner_invalidations(session.project(), &owner)?;
        session
            .transact(invalidations, |project| {
                let source_ids =
                    ordered_attachment_ids_excluding(project, &owner, source_stage, attachment_id);
                let mut destination_ids = if source_stage == target_stage {
                    source_ids.clone()
                } else {
                    ordered_attachment_ids_excluding(project, &owner, target_stage, attachment_id)
                };
                destination_ids.insert(target_index, attachment_id);

                if source_stage != target_stage {
                    for (order, id) in source_ids.into_iter().enumerate() {
                        project
                            .attachments
                            .get_mut(&id)
                            .ok_or_else(|| format!("Missing Attachment {id}"))?
                            .order = i64::try_from(order)
                            .map_err(|_| "Attachment stack is too large".to_string())?;
                    }
                }
                project
                    .attachments
                    .get_mut(&attachment_id)
                    .ok_or_else(|| format!("Missing Attachment {attachment_id}"))?
                    .stage = target_stage;
                for (order, id) in destination_ids.into_iter().enumerate() {
                    project
                        .attachments
                        .get_mut(&id)
                        .ok_or_else(|| format!("Missing Attachment {id}"))?
                        .order = i64::try_from(order)
                        .map_err(|_| "Attachment stack is too large".to_string())?;
                }
                Ok(())
            })
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }
}

fn attachment_owner(
    project: &AuthoringProject,
    attachment_id: AttachmentId,
) -> Result<AttachmentOwner, LibraryError> {
    project
        .attachments
        .get(&attachment_id)
        .map(|attachment| attachment.owner.clone())
        .ok_or_else(|| LibraryError::Validation(format!("Missing Attachment {attachment_id}")))
}

fn ordered_attachment_ids_excluding(
    project: &AuthoringProject,
    owner: &AttachmentOwner,
    stage: AttachmentStage,
    excluded: AttachmentId,
) -> Vec<AttachmentId> {
    let mut entries = project
        .attachments
        .values()
        .filter(|candidate| {
            candidate.owner == *owner && candidate.stage == stage && candidate.id != excluded
        })
        .map(|candidate| (candidate.order, candidate.id))
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| *entry);
    entries.into_iter().map(|(_, id)| id).collect()
}

fn attachment_module_invocation_mut(
    project: &mut AuthoringProject,
    attachment_id: AttachmentId,
) -> Result<&mut ModuleInvocation, String> {
    let attachment = project
        .attachments
        .get_mut(&attachment_id)
        .ok_or_else(|| format!("Missing Attachment {attachment_id}"))?;
    let AttachmentProcessor::Module(invocation) = &mut attachment.processor else {
        return Err("Attachment is not a Module Effect".to_string());
    };
    Ok(invocation)
}

fn builtin_effect_parameter_mut<'a>(
    project: &'a mut AuthoringProject,
    attachment_id: AttachmentId,
    key: &str,
) -> Result<&'a mut crate::model::authoring::BuiltinEffectParameter, String> {
    let attachment = project
        .attachments
        .get_mut(&attachment_id)
        .ok_or_else(|| format!("Missing Attachment {attachment_id}"))?;
    let AttachmentProcessor::BuiltinEffect(effect) = &mut attachment.processor else {
        return Err("Attachment is not a built-in Effect".to_string());
    };
    effect
        .parameters
        .get_mut(key)
        .ok_or_else(|| format!("Built-in Effect has no parameter '{key}'"))
}

pub(super) fn owner_invalidations(
    project: &AuthoringProject,
    owner: &AttachmentOwner,
) -> Result<Vec<ProjectInvalidation>, LibraryError> {
    let timeline_id = match owner {
        AttachmentOwner::Timeline { timeline_id } => *timeline_id,
        AttachmentOwner::Track { track_id } => timeline_for_track(project, *track_id)?,
        AttachmentOwner::Item { item_id } => timeline_for_item(project, *item_id)?,
    };
    Ok(vec![ProjectInvalidation::TimelineStructure { timeline_id }])
}

pub(super) fn normalize_attachment_order(
    project: &mut AuthoringProject,
    owner: &AttachmentOwner,
    stage: AttachmentStage,
) {
    let mut stack = project
        .attachments
        .values_mut()
        .filter(|attachment| attachment.owner == *owner && attachment.stage == stage)
        .collect::<Vec<_>>();
    stack.sort_by_key(|attachment| attachment.order);
    for (order, attachment) in stack.into_iter().enumerate() {
        attachment.order = order as i64;
    }
}

pub(super) fn normalize_all_attachment_orders(project: &mut AuthoringProject) {
    let stacks = project
        .attachments
        .values()
        .map(|attachment| (attachment.owner.clone(), attachment.stage))
        .collect::<std::collections::HashSet<_>>();
    for (owner, stage) in stacks {
        normalize_attachment_order(project, &owner, stage);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::authoring::{
        ModuleDefinitionSharing, ModuleOutputId, ModulePortAddress, PublishedMediaInput,
        PublishedMediaInputId,
    };
    use crate::model::project::{MERGE_IMAGES_PORT, PortDataType};

    fn private_image_effect() -> (ModuleDefinition, ModuleOutputId) {
        let (mut definition, output_id) =
            ModuleDefinition::new_image("Node Effect", ModuleDefinitionSharing::Private);
        let target = definition
            .output(output_id)
            .expect("Output terminal")
            .target(PortDataType::Image)
            .expect("Image input");
        definition.interface.media_inputs.push(PublishedMediaInput {
            id: PublishedMediaInputId::new(),
            name: "Input".to_string(),
            data_type: PortDataType::Image,
            target,
            required: true,
            primary: true,
        });
        (definition, output_id)
    }

    #[test]
    fn private_module_attachment_is_one_valid_undoable_transaction() {
        let service = TimelineEditorService::create_default("Attachments").expect("service");
        let timeline_id = service.snapshot().expect("project").root_timeline_id;
        let (definition, output_id) = private_image_effect();
        let definition_id = definition.id;
        let revision = service.revision().expect("revision");
        let (attachment_id, instance_id, changes) = service
            .create_private_module_attachment(
                definition,
                ModuleAttachmentPlacement {
                    owner: AttachmentOwner::Timeline { timeline_id },
                    stage: AttachmentStage::TimelinePostComposite,
                    definition_id,
                    output_id,
                    parameter_overrides: HashMap::new(),
                    input_bindings: HashMap::new(),
                },
            )
            .expect("attachment");

        assert_eq!(changes.revision.get(), revision.get() + 1);
        let project = service.snapshot().expect("project");
        assert!(project.attachments.contains_key(&attachment_id));
        assert_eq!(
            project.module_instances[&instance_id].definition_id,
            definition_id
        );
        project.validate().expect("valid project");
        drop(project);

        service.undo().expect("undo").expect("change");
        let undone = service.snapshot().expect("undone");
        assert!(!undone.attachments.contains_key(&attachment_id));
        assert!(!undone.module_instances.contains_key(&instance_id));
        assert!(!undone.module_definitions.contains_key(&definition_id));
    }

    #[test]
    fn attachment_enabled_and_bypass_state_are_authoritative_and_undoable() {
        let plugins = PluginManager::default();
        let service = TimelineEditorService::create_default("Attachments").expect("service");
        let timeline_id = service.snapshot().expect("project").root_timeline_id;
        let (attachment_id, _) = service
            .add_builtin_effect_by_id(
                &plugins,
                AttachmentOwner::Timeline { timeline_id },
                AttachmentStage::TimelinePostComposite,
                "blur",
            )
            .expect("effect");

        service
            .set_attachment_state(attachment_id, false, true)
            .expect("state");
        let project = service.snapshot().expect("project");
        assert!(!project.attachments[&attachment_id].enabled);
        assert!(project.attachments[&attachment_id].bypassed);
        drop(project);

        service.undo().expect("undo").expect("state change");
        let undone = service.snapshot().expect("project");
        assert!(undone.attachments[&attachment_id].enabled);
        assert!(!undone.attachments[&attachment_id].bypassed);
    }

    #[test]
    fn additional_attachment_input_binding_is_one_undoable_public_interface_edit() {
        use crate::model::authoring::{
            InstanceLocator, ItemOutputStage, MediaOutputKind, SourceRef, TimelineInterval,
        };
        use crate::model::frame::color::Color;

        let service = TimelineEditorService::create_default("Attachments").expect("service");
        let initial = service.snapshot().expect("project");
        let timeline_id = initial.root_timeline_id;
        let track_id = *initial.tracks.keys().next().expect("default Track");
        drop(initial);
        let (source_id, _) = service
            .add_item(
                track_id,
                "Displacement source".to_string(),
                SourceRef::Solid {
                    color: Color::white(),
                },
                TimelineInterval::new(MediaTime::zero(), MediaTime::new(5, 1).expect("duration"))
                    .expect("interval"),
                0,
            )
            .expect("source item");

        let (mut definition, output_id) = private_image_effect();
        let additional_node = Node::new_merge("Displacement Input");
        let additional_node_id = additional_node.id;
        definition
            .graph
            .nodes
            .insert(additional_node_id, additional_node);
        let additional_input_id = PublishedMediaInputId::new();
        definition.interface.media_inputs.push(PublishedMediaInput {
            id: additional_input_id,
            name: "Displacement".to_string(),
            data_type: PortDataType::Image,
            target: ModulePortAddress {
                node_id: additional_node_id,
                port: MERGE_IMAGES_PORT.to_string(),
            },
            required: false,
            primary: false,
        });
        definition.topology_revision += 1;
        definition.interface_version += 1;
        let definition_id = definition.id;
        let (attachment_id, _, _) = service
            .create_private_module_attachment(
                definition,
                ModuleAttachmentPlacement {
                    owner: AttachmentOwner::Timeline { timeline_id },
                    stage: AttachmentStage::TimelinePostComposite,
                    definition_id,
                    output_id,
                    parameter_overrides: HashMap::new(),
                    input_bindings: HashMap::new(),
                },
            )
            .expect("Module Effect");
        let before_revision = service.revision().expect("revision");
        service
            .bind_attachment_module_input(
                attachment_id,
                additional_input_id,
                MediaInputBinding::TimelineItemOutput {
                    locator: InstanceLocator::SameTimeline,
                    item_id: source_id,
                    output: MediaOutputKind::Image,
                    stage: ItemOutputStage::PostTransform,
                },
            )
            .expect("bind input");
        assert_eq!(
            service.revision().expect("revision").get(),
            before_revision.get() + 1
        );
        let bound = service.snapshot().expect("project");
        let AttachmentProcessor::Module(invocation) = &bound.attachments[&attachment_id].processor
        else {
            panic!("expected Module Effect");
        };
        assert!(invocation.input_bindings.contains_key(&additional_input_id));
        drop(bound);

        service.undo().expect("undo").expect("binding edit");
        let undone = service.snapshot().expect("project");
        let AttachmentProcessor::Module(invocation) = &undone.attachments[&attachment_id].processor
        else {
            panic!("expected Module Effect");
        };
        assert!(!invocation.input_bindings.contains_key(&additional_input_id));
    }
}
