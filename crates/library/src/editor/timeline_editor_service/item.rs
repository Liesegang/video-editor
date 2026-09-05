use super::*;

impl TimelineEditorService {
    pub fn move_item(
        &self,
        item_id: TimelineItemId,
        track_id: TimelineTrackId,
        start: MediaTime,
        layer: i64,
    ) -> Result<ChangeSet, LibraryError> {
        let plan = self
            .plan_current_timeline_edit(TimelineEditOperation::MoveItem {
                item_id,
                track_id,
                start,
                layer,
            })
            .map_err(LibraryError::from)?;
        self.commit_edit_plan(&plan).map_err(LibraryError::from)
    }

    /// Moves the selected clips as one exact-time, one-history-entry edit.
    /// Their time offsets and relative layer order are preserved while the
    /// primary clip anchors the destination Track and insertion layer.
    pub fn move_items(
        &self,
        item_ids: &[TimelineItemId],
        primary_item_id: TimelineItemId,
        track_id: TimelineTrackId,
        start: MediaTime,
        layer: i64,
    ) -> Result<ChangeSet, LibraryError> {
        let plan = self
            .plan_current_group_move(item_ids, primary_item_id, track_id, start, layer)
            .map_err(LibraryError::from)?;
        self.commit_edit_plan(&plan).map_err(LibraryError::from)
    }

    /// Trims a placement while preserving the local source time at every
    /// surviving Timeline instant.
    pub fn trim_item(
        &self,
        item_id: TimelineItemId,
        interval: TimelineInterval,
    ) -> Result<ChangeSet, LibraryError> {
        let plan = self
            .plan_current_timeline_edit(TimelineEditOperation::TrimItem { item_id, interval })
            .map_err(LibraryError::from)?;
        self.commit_edit_plan(&plan).map_err(LibraryError::from)
    }

    pub fn split_item(
        &self,
        item_id: TimelineItemId,
        timeline_time: MediaTime,
    ) -> Result<(TimelineItemId, ChangeSet), LibraryError> {
        let mut session = self.write_session()?;
        let timeline_id = timeline_for_item(session.project(), item_id)?;
        session
            .transact(
                vec![ProjectInvalidation::TimelineStructure { timeline_id }],
                |project| {
                    let right_id = split_item(project, item_id, timeline_time)?;
                    let track_id = project
                        .items
                        .get(&right_id)
                        .ok_or_else(|| format!("Missing split Timeline item {right_id}"))?
                        .track_id;
                    let layer = project
                        .items
                        .get(&item_id)
                        .ok_or_else(|| format!("Missing Timeline item {item_id}"))?
                        .layer
                        .saturating_add(1);
                    place_item_at_layer(project, right_id, track_id, layer)?;
                    Ok(right_id)
                },
            )
            .map_err(LibraryError::Validation)
    }

    pub fn duplicate_item(
        &self,
        item_id: TimelineItemId,
        start: MediaTime,
        layer: i64,
    ) -> Result<(TimelineItemId, ChangeSet), LibraryError> {
        let mut session = self.write_session()?;
        let timeline_id = timeline_for_item(session.project(), item_id)?;
        session
            .transact(
                vec![ProjectInvalidation::TimelineStructure { timeline_id }],
                |project| {
                    let duplicate_id = duplicate_item(project, item_id, start, layer)?;
                    let track_id = project
                        .items
                        .get(&duplicate_id)
                        .ok_or_else(|| format!("Missing duplicate Timeline item {duplicate_id}"))?
                        .track_id;
                    place_item_at_layer(project, duplicate_id, track_id, layer)?;
                    Ok(duplicate_id)
                },
            )
            .map_err(LibraryError::Validation)
    }

    pub fn delete_item(&self, item_id: TimelineItemId) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let timeline_id = timeline_for_item(session.project(), item_id)?;
        session
            .transact(
                vec![ProjectInvalidation::TimelineStructure { timeline_id }],
                |project| {
                    let dependencies = item_input_dependencies(project, item_id);
                    if !dependencies.is_empty() {
                        return Err(format!(
                            "Timeline item {item_id} is referenced by {}; remap inputs or call delete_item_cascade",
                            dependency_summary(&dependencies)
                        ));
                    }
                    let track_id = project
                        .items
                        .get(&item_id)
                        .ok_or_else(|| format!("Missing Timeline item {item_id}"))?
                        .track_id;
                    delete_unreferenced_item(project, item_id)?;
                    normalize_track_layers(project, track_id)
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub fn item_input_dependencies(
        &self,
        item_id: TimelineItemId,
    ) -> Result<Vec<TimelineItemDependency>, LibraryError> {
        let project = self.read_session()?;
        if !project.project().items.contains_key(&item_id) {
            return Err(LibraryError::Validation(format!(
                "Missing Timeline item {item_id}"
            )));
        }
        Ok(item_input_dependencies(project.project(), item_id))
    }

    /// Explicit destructive cascade. Every Node Clip, Module Attachment, or
    /// Transition whose input depends on the removed item/path is removed too.
    pub fn delete_item_cascade(&self, item_id: TimelineItemId) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let timeline_id = timeline_for_item(session.project(), item_id)?;
        session
            .transact(
                vec![ProjectInvalidation::TimelineStructure { timeline_id }],
                |project| {
                    delete_item_and_dependents(
                        project,
                        item_id,
                        &mut std::collections::HashSet::new(),
                    )?;
                    normalize_all_attachment_orders(project);
                    let track_ids = project.tracks.keys().copied().collect::<Vec<_>>();
                    for track_id in track_ids {
                        normalize_track_layers(project, track_id)?;
                    }
                    Ok(())
                },
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }
}

fn duplicate_source(
    project: &mut AuthoringProject,
    source: &SourceRef,
) -> Result<SourceRef, String> {
    let mut source = source.clone();
    if let SourceRef::Module(invocation) = &mut source {
        let original = project
            .module_instances
            .get(&invocation.instance_id)
            .cloned()
            .ok_or_else(|| format!("Missing Module instance {}", invocation.instance_id))?;
        let definition = project
            .module_definitions
            .get(&original.definition_id)
            .cloned()
            .ok_or_else(|| format!("Missing Module definition {}", original.definition_id))?;
        if matches!(
            definition.sharing,
            crate::model::authoring::ModuleDefinitionSharing::Private
        ) {
            project
                .module_definitions
                .get_mut(&original.definition_id)
                .ok_or_else(|| format!("Missing Module definition {}", original.definition_id))?
                .sharing = crate::model::authoring::ModuleDefinitionSharing::SharedLocal;
        }
        let instance_id = ModuleInstanceId::new();
        project.module_instances.insert(
            instance_id,
            ModuleInstance {
                id: instance_id,
                ..original
            },
        );
        invocation.instance_id = instance_id;
    }
    Ok(source)
}

fn duplicate_item_attachments(
    project: &mut AuthoringProject,
    source_item_id: TimelineItemId,
    target_item_id: TimelineItemId,
) -> Result<(), String> {
    let originals = project
        .attachments
        .values()
        .filter(|attachment| {
            attachment.owner
                == (AttachmentOwner::Item {
                    item_id: source_item_id,
                })
        })
        .cloned()
        .collect::<Vec<_>>();
    for mut attachment in originals {
        attachment.id = AttachmentId::new();
        attachment.owner = AttachmentOwner::Item {
            item_id: target_item_id,
        };
        if let AttachmentProcessor::Module(invocation) = &mut attachment.processor {
            let original = project
                .module_instances
                .get(&invocation.instance_id)
                .cloned()
                .ok_or_else(|| format!("Missing Module instance {}", invocation.instance_id))?;
            let definition = project
                .module_definitions
                .get(&original.definition_id)
                .cloned()
                .ok_or_else(|| format!("Missing Module definition {}", original.definition_id))?;
            if matches!(
                definition.sharing,
                crate::model::authoring::ModuleDefinitionSharing::Private
            ) {
                project
                    .module_definitions
                    .get_mut(&original.definition_id)
                    .ok_or_else(|| format!("Missing Module definition {}", original.definition_id))?
                    .sharing = crate::model::authoring::ModuleDefinitionSharing::SharedLocal;
            }
            let instance_id = ModuleInstanceId::new();
            project.module_instances.insert(
                instance_id,
                ModuleInstance {
                    id: instance_id,
                    ..original
                },
            );
            invocation.instance_id = instance_id;
        }
        project.attachments.insert(attachment.id, attachment);
    }
    Ok(())
}

fn split_item(
    project: &mut AuthoringProject,
    item_id: TimelineItemId,
    timeline_time: MediaTime,
) -> Result<TimelineItemId, String> {
    let original = project
        .items
        .get(&item_id)
        .cloned()
        .ok_or_else(|| format!("Missing Timeline item {item_id}"))?;
    let end = original.interval.end()?;
    if timeline_time <= original.interval.start || timeline_time >= end {
        return Err("Split time must be strictly inside the Timeline item".to_string());
    }
    let right_id = TimelineItemId::new();
    let right_source = duplicate_source(project, &original.source)?;
    let right_source_start = original
        .time_map
        .local_time(original.interval, timeline_time)?;
    let left_duration = timeline_time.checked_sub(original.interval.start)?;
    let right_duration = end.checked_sub(timeline_time)?;
    project
        .items
        .get_mut(&item_id)
        .ok_or_else(|| format!("Missing Timeline item {item_id}"))?
        .interval
        .duration = left_duration;
    project.items.insert(
        right_id,
        TimelineItem {
            id: right_id,
            source: right_source,
            interval: TimelineInterval::new(timeline_time, right_duration)?,
            time_map: TimeMap {
                source_start: right_source_start,
                ..original.time_map
            },
            ..original
        },
    );
    duplicate_item_attachments(project, item_id, right_id)?;
    Ok(right_id)
}

fn duplicate_item(
    project: &mut AuthoringProject,
    item_id: TimelineItemId,
    start: MediaTime,
    layer: i64,
) -> Result<TimelineItemId, String> {
    let original = project
        .items
        .get(&item_id)
        .cloned()
        .ok_or_else(|| format!("Missing Timeline item {item_id}"))?;
    let duplicate_id = TimelineItemId::new();
    let source = duplicate_source(project, &original.source)?;
    project.items.insert(
        duplicate_id,
        TimelineItem {
            id: duplicate_id,
            source,
            interval: TimelineInterval::new(start, original.interval.duration)?,
            layer,
            ..original
        },
    );
    duplicate_item_attachments(project, item_id, duplicate_id)?;
    Ok(duplicate_id)
}

fn delete_unreferenced_item(
    project: &mut AuthoringProject,
    item_id: TimelineItemId,
) -> Result<(), String> {
    let removed = project
        .items
        .remove(&item_id)
        .ok_or_else(|| format!("Missing Timeline item {item_id}"))?;
    if let SourceRef::Module(invocation) = removed.source {
        remove_instance_and_private_definition(project, invocation.instance_id);
    }
    for item in project.items.values_mut() {
        if item.parent == Some(item_id) {
            item.parent = None;
        }
    }
    let attachment_ids = project
        .attachments
        .values()
        .filter_map(|attachment| {
            (attachment.owner == AttachmentOwner::Item { item_id }).then_some(attachment.id)
        })
        .collect::<Vec<_>>();
    for attachment_id in attachment_ids {
        if let Some(attachment) = project.attachments.remove(&attachment_id)
            && let AttachmentProcessor::Module(invocation) = attachment.processor
        {
            remove_instance_and_private_definition(project, invocation.instance_id);
        }
    }
    Ok(())
}

fn delete_item_and_dependents(
    project: &mut AuthoringProject,
    item_id: TimelineItemId,
    deleting: &mut std::collections::HashSet<TimelineItemId>,
) -> Result<(), String> {
    if !project.items.contains_key(&item_id) {
        return Ok(());
    }
    if !deleting.insert(item_id) {
        return Err(format!(
            "Deletion dependency cycle reaches Timeline item {item_id}"
        ));
    }
    let mut removed_transitions = std::collections::HashSet::new();
    for dependency in item_input_dependencies(project, item_id) {
        match dependency {
            TimelineItemDependency::TransitionParticipant { transition_id } => {
                if removed_transitions.insert(transition_id)
                    && project.transitions.contains_key(&transition_id)
                {
                    super::transition::remove_transition_and_owned_module(project, transition_id)?;
                }
            }
            TimelineItemDependency::ModuleInput {
                host: ModuleInputHost::Item(dependent_item_id),
                ..
            } => {
                delete_item_and_dependents(project, dependent_item_id, deleting)?;
            }
            TimelineItemDependency::ModuleInput {
                host: ModuleInputHost::Attachment(attachment_id),
                ..
            } => {
                if let Some(attachment) = project.attachments.remove(&attachment_id)
                    && let AttachmentProcessor::Module(invocation) = attachment.processor
                {
                    remove_instance_and_private_definition(project, invocation.instance_id);
                }
            }
            TimelineItemDependency::ModuleInput {
                host: ModuleInputHost::Transition(transition_id),
                ..
            } => {
                if removed_transitions.insert(transition_id)
                    && project.transitions.contains_key(&transition_id)
                {
                    super::transition::remove_transition_and_owned_module(project, transition_id)?;
                }
            }
            TimelineItemDependency::TransitionInstancePath { .. } => {
                project.remove_transition_module_overrides_through_item(item_id);
            }
        }
    }
    delete_unreferenced_item(project, item_id)?;
    deleting.remove(&item_id);
    Ok(())
}

fn item_input_dependencies(
    project: &AuthoringProject,
    item_id: TimelineItemId,
) -> Vec<TimelineItemDependency> {
    let mut dependencies = Vec::new();
    for transition in project.transitions.values() {
        if transition.from_item_id == item_id || transition.to_item_id == item_id {
            dependencies.push(TimelineItemDependency::TransitionParticipant {
                transition_id: transition.id,
            });
        }
    }
    for item in project.items.values() {
        if let SourceRef::Module(invocation) = &item.source {
            for (input_id, binding) in &invocation.input_bindings {
                if binding_references_item(binding, item_id) {
                    dependencies.push(TimelineItemDependency::ModuleInput {
                        host: ModuleInputHost::Item(item.id),
                        input_id: *input_id,
                    });
                }
            }
        }
    }
    for attachment in project.attachments.values() {
        if let AttachmentProcessor::Module(invocation) = &attachment.processor {
            for (input_id, binding) in &invocation.input_bindings {
                if binding_references_item(binding, item_id) {
                    dependencies.push(TimelineItemDependency::ModuleInput {
                        host: ModuleInputHost::Attachment(attachment.id),
                        input_id: *input_id,
                    });
                }
            }
        }
    }
    project.for_each_transition_module_input_binding(|transition_id, input_id, binding| {
        let dependency = TimelineItemDependency::ModuleInput {
            host: ModuleInputHost::Transition(transition_id),
            input_id,
        };
        if binding_references_item(binding, item_id) && !dependencies.contains(&dependency) {
            dependencies.push(dependency);
        }
    });
    for (owner_item_id, controls) in project.transition_module_instance_override_records() {
        if controls.target.composition_items.contains(&item_id) {
            let dependency = TimelineItemDependency::TransitionInstancePath {
                owner_item_id,
                transition_id: controls.target.transition_id,
            };
            if !dependencies.contains(&dependency) {
                dependencies.push(dependency);
            }
        }
    }
    dependencies
}

fn dependency_summary(dependencies: &[TimelineItemDependency]) -> String {
    dependencies
        .iter()
        .map(|dependency| match dependency {
            TimelineItemDependency::TransitionParticipant { transition_id } => {
                format!("Transition {transition_id} participant")
            }
            TimelineItemDependency::ModuleInput {
                host: ModuleInputHost::Item(item_id),
                input_id,
            } => {
                format!("Node Clip {item_id} input {input_id}")
            }
            TimelineItemDependency::ModuleInput {
                host: ModuleInputHost::Attachment(attachment_id),
                input_id,
            } => {
                format!("Attachment {attachment_id} input {input_id}")
            }
            TimelineItemDependency::ModuleInput {
                host: ModuleInputHost::Transition(transition_id),
                input_id,
            } => {
                format!("Transition {transition_id} input {input_id}")
            }
            TimelineItemDependency::TransitionInstancePath {
                owner_item_id,
                transition_id,
            } => {
                format!(
                    "Transition {transition_id} instance path owned by Composition {owner_item_id}"
                )
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn binding_references_item(binding: &MediaInputBinding, item_id: TimelineItemId) -> bool {
    let MediaInputBinding::TimelineItemOutput {
        locator,
        item_id: source_item_id,
        ..
    } = binding;
    *source_item_id == item_id
        || matches!(
            locator,
            InstanceLocator::Exact(path) if path.composition_items.contains(&item_id)
        )
}
