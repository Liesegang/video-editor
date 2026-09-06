//! Instance values and Timeline-owned automation for published Module parameters.

use super::super::module_parameter_owner::transition_id;
use super::super::transition_parameter_automation::{
    clear_transition_parameter_override_in_project, edit_transition_parameter_track_in_project,
    set_transition_parameter_constant_in_project,
};
use super::super::*;
use crate::model::authoring::TransitionModuleInstanceOverrides;

#[derive(Clone, PartialEq, Debug)]
pub struct ResolvedModuleParameter {
    pub instance_id: ModuleInstanceId,
    pub definition_id: ModuleDefinitionId,
    pub base_value: PropertyValue,
    pub value: PropertyValue,
    pub automation: Option<AutomationTrack>,
    /// Whether Reset changes this exact owner scope. For a nested Transition
    /// this includes a local value, key track, or explicit inherited-track mask.
    pub has_resettable_override: bool,
}

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
                |project| set_instance_parameter_value(project, instance_id, parameter_id, value),
            )
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    /// Switches a published parameter to a constant at the owner's exact
    /// scope in one undoable edit. For a nested Transition placement this is
    /// a sparse override that explicitly masks inherited automation.
    pub fn set_module_parameter_constant(
        &self,
        owner: &ModuleParameterOwner,
        parameter_id: PublishedParameterId,
        value: PropertyValue,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let instance_id = owner
            .instance_id(session.project())
            .map_err(LibraryError::Validation)?;
        let mut invalidations = owner.invalidations(session.project())?;
        if needs_direct_module_instance_invalidation(owner) {
            invalidations.push(ProjectInvalidation::ModuleInstance { instance_id });
        }
        session
            .transact(invalidations, |project| {
                set_module_parameter_constant_in_project(
                    project,
                    owner,
                    instance_id,
                    parameter_id,
                    value,
                )
            })
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    /// Removes the exact owner's constant override. Invocations then follow
    /// the published default; nested Transition placements inherit their
    /// parent value and automation again.
    pub fn clear_module_parameter_override(
        &self,
        owner: &ModuleParameterOwner,
        parameter_id: PublishedParameterId,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let instance_id = owner
            .instance_id(session.project())
            .map_err(LibraryError::Validation)?;
        let mut invalidations = owner.invalidations(session.project())?;
        if needs_direct_module_instance_invalidation(owner) {
            invalidations.push(ProjectInvalidation::ModuleInstance { instance_id });
        }
        session
            .transact(invalidations, |project| {
                clear_module_parameter_override_in_project(
                    project,
                    owner,
                    instance_id,
                    parameter_id,
                )
            })
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub fn upsert_module_parameter_keyframe(
        &self,
        owner: &ModuleParameterOwner,
        parameter_id: PublishedParameterId,
        local_time: MediaTime,
        value: PropertyValue,
        easing: Option<EasingFunction>,
    ) -> Result<(KeyframeId, ChangeSet), LibraryError> {
        let insertion_id = KeyframeId::new();
        let mut session = self.write_session()?;
        require_module_parameter_automation(session.project(), owner, parameter_id)?;
        let invalidations = owner.invalidations(session.project())?;
        session
            .transact(invalidations, |project| {
                upsert_parameter_keyframe(
                    project,
                    owner,
                    parameter_id,
                    insertion_id,
                    local_time,
                    value,
                    easing,
                )
            })
            .map_err(LibraryError::Validation)
    }

    /// Projects an Inspector gesture without writing Project state or Undo
    /// history. Uses the same value/keyframe mutation as the release command,
    /// retaining Timeline automation ownership and instance-local constants.
    pub fn project_module_parameter_value(
        project: &AuthoringProject,
        owner: &ModuleParameterOwner,
        instance_id: ModuleInstanceId,
        parameter_id: PublishedParameterId,
        value: PropertyValue,
        target: AuthoringPropertyValueTarget,
    ) -> Result<AuthoringProject, LibraryError> {
        let mut projected = project.clone();
        apply_module_parameter_value_to_project(
            &mut projected,
            owner,
            instance_id,
            parameter_id,
            value,
            target,
        )
        .map_err(LibraryError::Validation)?;
        Ok(projected)
    }

    /// Applies the exact typed value target used by transient projection in
    /// one authoring transaction.
    pub fn apply_module_parameter_value(
        &self,
        owner: &ModuleParameterOwner,
        instance_id: ModuleInstanceId,
        parameter_id: PublishedParameterId,
        value: PropertyValue,
        target: AuthoringPropertyValueTarget,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        let mut invalidations = owner.invalidations(session.project())?;
        if matches!(target, AuthoringPropertyValueTarget::Constant)
            && needs_direct_module_instance_invalidation(owner)
        {
            invalidations.push(ProjectInvalidation::ModuleInstance { instance_id });
        }
        session
            .transact(invalidations, |project| {
                apply_module_parameter_value_to_project(
                    project,
                    owner,
                    instance_id,
                    parameter_id,
                    value,
                    target,
                )
            })
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub fn remove_module_parameter_keyframe(
        &self,
        owner: &ModuleParameterOwner,
        parameter_id: PublishedParameterId,
        keyframe_id: KeyframeId,
    ) -> Result<ChangeSet, LibraryError> {
        let mut session = self.write_session()?;
        require_module_parameter_automation(session.project(), owner, parameter_id)?;
        let invalidations = owner.invalidations(session.project())?;
        session
            .transact(invalidations, |project| {
                edit_module_parameter_track_in_project(
                    project,
                    owner,
                    parameter_id,
                    false,
                    |track| {
                        if !track.remove_keyframe(keyframe_id) {
                            return Err(format!("Missing Automation Keyframe {keyframe_id}"));
                        }
                        Ok(())
                    },
                )
            })
            .map(|(_, changes)| changes)
            .map_err(LibraryError::Validation)
    }

    pub fn resolve_module_parameter(
        project: &AuthoringProject,
        owner: &ModuleParameterOwner,
        parameter_id: PublishedParameterId,
        local_time: MediaTime,
    ) -> Result<ResolvedModuleParameter, LibraryError> {
        let controls =
            module_parameter_controls(project, owner).map_err(LibraryError::Validation)?;
        let instance_id = controls.instance_id;
        let definition_id = controls.definition_id;
        let definition = project
            .module_definitions
            .get(&definition_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!("Missing Module definition {definition_id}"))
            })?;
        let parameter = definition
            .interface
            .parameters
            .iter()
            .find(|parameter| parameter.id == parameter_id)
            .ok_or_else(|| {
                LibraryError::Validation(format!("Missing Published parameter {parameter_id}"))
            })?;
        let base_value = controls
            .parameter_override(parameter_id)
            .cloned()
            .unwrap_or_else(|| parameter.default_value.clone());
        let automation = controls.automation_track(parameter_id).cloned();
        let has_resettable_override = controls.has_resettable_override(parameter_id);
        let value = automation
            .as_ref()
            .map(|track| track.evaluate_at(local_time))
            .transpose()?
            .unwrap_or_else(|| base_value.clone());
        Ok(ResolvedModuleParameter {
            instance_id,
            definition_id,
            base_value,
            value,
            automation,
            has_resettable_override,
        })
    }
}

pub(in crate::editor::timeline_editor_service) struct ModuleParameterControlsView<'a> {
    instance_id: ModuleInstanceId,
    definition_id: ModuleDefinitionId,
    parameter_overrides: &'a HashMap<PublishedParameterId, PropertyValue>,
    automation_tracks: &'a HashMap<PublishedParameterId, AutomationTrack>,
    sparse: Option<&'a TransitionModuleInstanceOverrides>,
    sparse_scope: bool,
}

impl ModuleParameterControlsView<'_> {
    pub(in crate::editor::timeline_editor_service) fn parameter_override(
        &self,
        parameter_id: PublishedParameterId,
    ) -> Option<&PropertyValue> {
        self.sparse
            .and_then(|sparse| sparse.parameter_overrides.get(&parameter_id))
            .or_else(|| self.parameter_overrides.get(&parameter_id))
    }

    pub(in crate::editor::timeline_editor_service) fn automation_track(
        &self,
        parameter_id: PublishedParameterId,
    ) -> Option<&AutomationTrack> {
        match self
            .sparse
            .and_then(|sparse| sparse.automation_tracks.get(&parameter_id))
        {
            Some(Some(track)) => Some(track),
            Some(None) => None,
            None => self.automation_tracks.get(&parameter_id),
        }
    }

    fn has_resettable_override(&self, parameter_id: PublishedParameterId) -> bool {
        if self.sparse_scope {
            self.sparse.is_some_and(|sparse| {
                sparse.parameter_overrides.contains_key(&parameter_id)
                    || sparse.automation_tracks.contains_key(&parameter_id)
            })
        } else {
            self.parameter_overrides.contains_key(&parameter_id)
        }
    }
}

pub(in crate::editor::timeline_editor_service) fn module_parameter_controls<'a>(
    project: &'a AuthoringProject,
    owner: &ModuleParameterOwner,
) -> Result<ModuleParameterControlsView<'a>, String> {
    match owner {
        ModuleParameterOwner::Invocation(owner) => {
            let invocation = owner.invocation(project)?;
            let instance = project
                .module_instances
                .get(&invocation.instance_id)
                .ok_or_else(|| format!("Missing Module instance {}", invocation.instance_id))?;
            Ok(ModuleParameterControlsView {
                instance_id: instance.id,
                definition_id: instance.definition_id,
                parameter_overrides: &instance.parameter_overrides,
                automation_tracks: &invocation.automation_tracks,
                sparse: None,
                sparse_scope: false,
            })
        }
        ModuleParameterOwner::Transition(owner) => {
            let transition_id = transition_id(owner);
            let transition = project
                .transitions
                .get(&transition_id)
                .ok_or_else(|| format!("Missing Transition {transition_id}"))?;
            let processor = transition
                .processor
                .module_processor()
                .ok_or_else(|| format!("Transition {transition_id} does not use a Module"))?;
            let instance = project
                .module_instances
                .get(&processor.instance_id)
                .ok_or_else(|| format!("Missing Module instance {}", processor.instance_id))?;
            let (sparse, sparse_scope) = match owner {
                TransitionAutomationOwner::Definition(_) => (None, false),
                TransitionAutomationOwner::Instance { instance_path, .. } => {
                    let target = project
                        .resolve_transition_module_instance_target(instance_path, transition_id)?;
                    if target.module_instance_id != processor.instance_id {
                        return Err("Transition Module instance target is stale".to_string());
                    }
                    (
                        project.transition_module_instance_overrides(&target)?,
                        !instance_path.composition_items.is_empty(),
                    )
                }
            };
            Ok(ModuleParameterControlsView {
                instance_id: instance.id,
                definition_id: instance.definition_id,
                parameter_overrides: &instance.parameter_overrides,
                automation_tracks: &processor.automation_tracks,
                sparse,
                sparse_scope,
            })
        }
    }
}

fn set_instance_parameter_value(
    project: &mut AuthoringProject,
    instance_id: ModuleInstanceId,
    parameter_id: PublishedParameterId,
    value: PropertyValue,
) -> Result<(), String> {
    let instance = project
        .module_instances
        .get_mut(&instance_id)
        .ok_or_else(|| format!("Missing Module instance {instance_id}"))?;
    let definition = project
        .module_definitions
        .get(&instance.definition_id)
        .ok_or_else(|| format!("Missing Module definition {}", instance.definition_id))?;
    definition.validate_parameter_value(parameter_id, &value)?;
    instance.parameter_overrides.insert(parameter_id, value);
    definition.validate_parameter_overrides(&instance.parameter_overrides)
}

pub(in crate::editor::timeline_editor_service) fn set_module_parameter_override_in_project(
    project: &mut AuthoringProject,
    owner: &ModuleParameterOwner,
    expected_instance_id: ModuleInstanceId,
    parameter_id: PublishedParameterId,
    value: PropertyValue,
) -> Result<(), String> {
    if owner.instance_id(project)? != expected_instance_id {
        return Err("Module parameter owner changed Module instance during the edit".to_string());
    }
    match owner {
        ModuleParameterOwner::Invocation(_)
        | ModuleParameterOwner::Transition(TransitionAutomationOwner::Definition(_)) => {
            set_instance_parameter_value(project, expected_instance_id, parameter_id, value)
        }
        ModuleParameterOwner::Transition(TransitionAutomationOwner::Instance {
            instance_path,
            ..
        }) if instance_path.composition_items.is_empty() => {
            set_instance_parameter_value(project, expected_instance_id, parameter_id, value)
        }
        ModuleParameterOwner::Transition(TransitionAutomationOwner::Instance {
            transition_id,
            instance_path,
        }) => {
            let target =
                project.resolve_transition_module_instance_target(instance_path, *transition_id)?;
            let definition_id = project
                .module_instances
                .get(&expected_instance_id)
                .ok_or_else(|| format!("Missing Module instance {expected_instance_id}"))?
                .definition_id;
            project
                .module_definitions
                .get(&definition_id)
                .ok_or_else(|| format!("Missing Module definition {definition_id}"))?
                .validate_parameter_value(parameter_id, &value)?;
            project.edit_transition_module_instance_overrides(&target, |controls| {
                controls.parameter_overrides.insert(parameter_id, value);
                Ok(())
            })
        }
    }
}

pub(in crate::editor::timeline_editor_service) fn upsert_parameter_keyframe(
    project: &mut AuthoringProject,
    owner: &ModuleParameterOwner,
    parameter_id: PublishedParameterId,
    insertion_id: KeyframeId,
    local_time: MediaTime,
    value: PropertyValue,
    easing: Option<EasingFunction>,
) -> Result<KeyframeId, String> {
    let definition = require_module_parameter_automation(project, owner, parameter_id)
        .map_err(|error| error.to_string())?;
    definition.validate_parameter_value(parameter_id, &value)?;
    if local_time.is_negative() {
        return Err("Automation Keyframe time must be non-negative".to_string());
    }
    edit_module_parameter_track_in_project(project, owner, parameter_id, true, |track| {
        track.upsert(insertion_id, local_time, value, easing)
    })
}

fn apply_module_parameter_value_to_project(
    project: &mut AuthoringProject,
    owner: &ModuleParameterOwner,
    instance_id: ModuleInstanceId,
    parameter_id: PublishedParameterId,
    value: PropertyValue,
    target: AuthoringPropertyValueTarget,
) -> Result<(), String> {
    let actual_instance_id = owner.instance_id(project)?;
    if actual_instance_id != instance_id {
        return Err("Module automation owner changed Module instance during the edit".to_string());
    }
    match target {
        AuthoringPropertyValueTarget::Constant => {
            let resolved = TimelineEditorService::resolve_module_parameter(
                project,
                owner,
                parameter_id,
                MediaTime::zero(),
            )
            .map_err(|error| error.to_string())?;
            if resolved.automation.is_some() {
                return Err(format!(
                    "Published parameter {parameter_id} is controlled by Timeline automation"
                ));
            }
            set_module_parameter_constant_in_project(
                project,
                owner,
                instance_id,
                parameter_id,
                value,
            )
        }
        AuthoringPropertyValueTarget::Keyframe {
            local_time,
            insertion_id,
        } => upsert_parameter_keyframe(
            project,
            owner,
            parameter_id,
            insertion_id,
            local_time,
            value,
            None,
        )
        .map(|_| ()),
    }
}

pub(in crate::editor::timeline_editor_service) fn require_module_parameter_automation<'a>(
    project: &'a AuthoringProject,
    owner: &ModuleParameterOwner,
    parameter_id: PublishedParameterId,
) -> Result<&'a ModuleDefinition, LibraryError> {
    let instance_id = owner
        .instance_id(project)
        .map_err(LibraryError::Validation)?;
    if let ModuleParameterOwner::Transition(transition_owner) = owner {
        let transition_id = transition_id(transition_owner);
        let (_, _, contract) = super::super::transition_module_controls::transition_module_context(
            project,
            transition_id,
        )?;
        super::super::transition_module_controls::require_editable_parameter(
            transition_id,
            &contract,
            parameter_id,
        )?;
    }
    let instance = project.module_instances.get(&instance_id).ok_or_else(|| {
        LibraryError::Validation(format!("Missing Module instance {}", instance_id))
    })?;
    let definition = project
        .module_definitions
        .get(&instance.definition_id)
        .ok_or_else(|| {
            LibraryError::Validation(format!(
                "Missing Module definition {}",
                instance.definition_id
            ))
        })?;
    definition
        .require_parameter_automation(parameter_id)
        .map_err(LibraryError::Validation)?;
    Ok(definition)
}

fn needs_direct_module_instance_invalidation(owner: &ModuleParameterOwner) -> bool {
    matches!(owner, ModuleParameterOwner::Invocation(_))
}

fn set_module_parameter_constant_in_project(
    project: &mut AuthoringProject,
    owner: &ModuleParameterOwner,
    expected_instance_id: ModuleInstanceId,
    parameter_id: PublishedParameterId,
    value: PropertyValue,
) -> Result<(), String> {
    if owner.instance_id(project)? != expected_instance_id {
        return Err("Module automation owner changed Module instance during the edit".to_string());
    }
    let definition_id = project
        .module_instances
        .get(&expected_instance_id)
        .ok_or_else(|| format!("Missing Module instance {expected_instance_id}"))?
        .definition_id;
    project
        .module_definitions
        .get(&definition_id)
        .ok_or_else(|| format!("Missing Module definition {definition_id}"))?
        .validate_parameter_value(parameter_id, &value)?;
    match owner {
        ModuleParameterOwner::Invocation(owner) => {
            owner
                .invocation_mut(project)?
                .automation_tracks
                .remove(&parameter_id);
            set_instance_parameter_value(project, expected_instance_id, parameter_id, value)
        }
        ModuleParameterOwner::Transition(owner) => {
            set_transition_parameter_constant_in_project(project, owner, parameter_id, value)
        }
    }
}

fn clear_module_parameter_override_in_project(
    project: &mut AuthoringProject,
    owner: &ModuleParameterOwner,
    expected_instance_id: ModuleInstanceId,
    parameter_id: PublishedParameterId,
) -> Result<(), String> {
    if owner.instance_id(project)? != expected_instance_id {
        return Err("Module automation owner changed Module instance during the edit".to_string());
    }
    match owner {
        ModuleParameterOwner::Invocation(_) => project
            .module_instances
            .get_mut(&expected_instance_id)
            .ok_or_else(|| format!("Missing Module instance {expected_instance_id}"))?
            .parameter_overrides
            .remove(&parameter_id)
            .map(|_| ())
            .ok_or_else(|| {
                format!("Module instance {expected_instance_id} has no override for {parameter_id}")
            }),
        ModuleParameterOwner::Transition(owner) => {
            clear_transition_parameter_override_in_project(project, owner, parameter_id)
        }
    }
}

pub(in crate::editor::timeline_editor_service) fn edit_module_parameter_track_in_project<T>(
    project: &mut AuthoringProject,
    owner: &ModuleParameterOwner,
    parameter_id: PublishedParameterId,
    create: bool,
    edit: impl FnOnce(&mut AutomationTrack) -> Result<T, String>,
) -> Result<T, String> {
    match owner {
        ModuleParameterOwner::Invocation(owner) => {
            let invocation = owner.invocation_mut(project)?;
            if create {
                let track = invocation
                    .automation_tracks
                    .entry(parameter_id)
                    .or_insert_with(|| AutomationTrack {
                        keyframes: Vec::new(),
                    });
                edit(track)
            } else {
                let track = invocation
                    .automation_tracks
                    .get_mut(&parameter_id)
                    .ok_or_else(|| {
                        format!("Missing automation for Published parameter {parameter_id}")
                    })?;
                let result = edit(track)?;
                if track.keyframes.is_empty() {
                    invocation.automation_tracks.remove(&parameter_id);
                }
                Ok(result)
            }
        }
        ModuleParameterOwner::Transition(owner) => {
            edit_transition_parameter_track_in_project(project, owner, parameter_id, edit)
        }
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
                &ModuleParameterOwner::Invocation(ModuleAutomationOwner::Item(item_id)),
                parameter_id,
                MediaTime::new(1, 1).expect("time"),
                default_value.clone(),
                None,
            )
            .expect("keyframe");
        let insertion_source = service.snapshot().expect("insertion source");
        let insertion_revision = service.revision().expect("insertion revision");
        let insertion_id = KeyframeId::new();
        let target = AuthoringPropertyValueTarget::Keyframe {
            local_time: MediaTime::new(2, 1).expect("new key time"),
            insertion_id,
        };
        let projected = TimelineEditorService::project_module_parameter_value(
            &insertion_source,
            &ModuleParameterOwner::Invocation(ModuleAutomationOwner::Item(item_id)),
            instance_id,
            parameter_id,
            default_value.clone(),
            target,
        )
        .expect("project stable key identity");
        assert_eq!(
            service.revision().expect("unchanged revision"),
            insertion_revision
        );
        let SourceRef::Module(projected_invocation) = &projected.items[&item_id].source else {
            panic!("expected projected Module item")
        };
        assert_eq!(
            projected_invocation.automation_tracks[&parameter_id]
                .keyframes
                .last()
                .expect("projected key")
                .id,
            insertion_id
        );
        service
            .apply_module_parameter_value(
                &ModuleParameterOwner::Invocation(ModuleAutomationOwner::Item(item_id)),
                instance_id,
                parameter_id,
                default_value.clone(),
                target,
            )
            .expect("commit projected key identity");
        assert_eq!(service.snapshot().expect("committed").as_ref(), &projected);
        service
            .undo()
            .expect("undo insertion")
            .expect("insertion change");
        assert_eq!(
            service.snapshot().expect("restored insertion source"),
            insertion_source
        );

        let first_id = match &insertion_source.items[&item_id].source {
            SourceRef::Module(invocation) => {
                invocation.automation_tracks[&parameter_id].keyframes[0].id
            }
            _ => panic!("expected Module item"),
        };
        let collision = AuthoringPropertyValueTarget::Keyframe {
            local_time: MediaTime::new(2, 1).expect("collision time"),
            insertion_id: first_id,
        };
        let collision_revision = service.revision().expect("collision baseline revision");
        TimelineEditorService::project_module_parameter_value(
            &insertion_source,
            &ModuleParameterOwner::Invocation(ModuleAutomationOwner::Item(item_id)),
            instance_id,
            parameter_id,
            default_value.clone(),
            collision,
        )
        .expect_err("projection rejects an identity collision");
        service
            .apply_module_parameter_value(
                &ModuleParameterOwner::Invocation(ModuleAutomationOwner::Item(item_id)),
                instance_id,
                parameter_id,
                default_value.clone(),
                collision,
            )
            .expect_err("commit rejects an identity collision");
        assert_eq!(
            service.revision().expect("collision revision"),
            collision_revision
        );
        assert_eq!(
            service.snapshot().expect("collision snapshot"),
            insertion_source
        );

        let before = service.snapshot().expect("automated");
        let revision = service.revision().expect("revision");
        let constant = default_value;

        let change = service
            .set_module_parameter_constant(
                &ModuleParameterOwner::Invocation(ModuleAutomationOwner::Item(item_id)),
                parameter_id,
                constant.clone(),
            )
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
