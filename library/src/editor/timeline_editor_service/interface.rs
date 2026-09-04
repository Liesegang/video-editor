use std::collections::HashSet;

use super::module::{
    bump_interface_version, module_definition_mut, private_definition_for_instance,
};
use super::*;
use crate::model::authoring::{
    ModuleDefinitionSharing, ModulePortAddress, PublishedMediaInput, PublishedParameter,
    property_value_type,
};
use crate::model::project::{PortDataType, PortDirection};

/// One atomic edit to a Module's only externally addressable surface.
#[derive(Clone, PartialEq, Debug)]
pub enum ModuleInterfaceCommand {
    PublishParameter {
        name: String,
        default_value: PropertyValue,
        target: ModulePortAddress,
    },
    RenameParameter {
        parameter_id: PublishedParameterId,
        name: String,
    },
    UnpublishParameter {
        parameter_id: PublishedParameterId,
    },
    PublishMediaInput {
        name: String,
        target: ModulePortAddress,
        required: bool,
        primary: bool,
    },
    RenameMediaInput {
        input_id: PublishedMediaInputId,
        name: String,
    },
    /// Moves the existing primary input's stable Published Interface ID to a
    /// different internal media port. Invocation bindings continue to address
    /// `input_id`; only the Module-internal target changes.
    RetargetPrimaryMediaInput {
        input_id: PublishedMediaInputId,
        target: ModulePortAddress,
    },
    UnpublishMediaInput {
        input_id: PublishedMediaInputId,
    },
}

#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct ModuleInterfaceEditImpact {
    pub removed_parameter_overrides: usize,
    pub removed_automation_tracks: usize,
    pub removed_media_input_bindings: usize,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ModuleInterfaceEditResult {
    PublishedParameter(PublishedParameterId),
    PublishedMediaInput(PublishedMediaInputId),
    Updated,
    Unpublished(ModuleInterfaceEditImpact),
}

impl TimelineEditorService {
    /// Ordinary Node Editor path. Shared-local and reusable definitions are
    /// copy-on-write before the interface edit, so sibling instances retain
    /// both their public IDs and authored values.
    pub fn edit_instance_module_interface(
        &self,
        instance_id: ModuleInstanceId,
        command: ModuleInterfaceCommand,
    ) -> Result<(ModuleInterfaceEditResult, ModuleDefinitionId, ChangeSet), LibraryError> {
        let mut session = self.write_session()?;
        session
            .transact(
                vec![ProjectInvalidation::ModuleInstance { instance_id }],
                |project| {
                    let definition_id = private_definition_for_instance(project, instance_id)?;
                    let result =
                        apply_interface_command(project, definition_id, &[instance_id], command)?;
                    Ok((result, definition_id))
                },
            )
            .map(|((result, definition_id), changes)| (result, definition_id, changes))
            .map_err(LibraryError::Validation)
    }

    /// Explicit template edit. UI should show `affected_instance_count` from
    /// the returned value before committing a confirmed shared edit.
    pub fn edit_shared_module_interface(
        &self,
        definition_id: ModuleDefinitionId,
        command: ModuleInterfaceCommand,
    ) -> Result<SharedModuleEdit<ModuleInterfaceEditResult>, LibraryError> {
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
            ModuleDefinitionSharing::ReusableTemplate(_)
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
        let (value, changes) = session
            .transact(
                vec![ProjectInvalidation::ModuleDefinition { definition_id }],
                |project| apply_interface_command(project, definition_id, &instance_ids, command),
            )
            .map_err(LibraryError::Validation)?;
        Ok(SharedModuleEdit {
            value,
            affected_instance_count,
            changes,
        })
    }
}

#[derive(Clone, Copy)]
enum InterfaceCleanup {
    None,
    Parameter(PublishedParameterId),
    MediaInput(PublishedMediaInputId),
}

fn apply_interface_command(
    project: &mut AuthoringProject,
    definition_id: ModuleDefinitionId,
    affected_instances: &[ModuleInstanceId],
    command: ModuleInterfaceCommand,
) -> Result<ModuleInterfaceEditResult, String> {
    let (result, cleanup) = {
        let definition = module_definition_mut(project, definition_id)?;
        apply_definition_interface_command(definition, command)?
    };
    let impact = cleanup_interface_dependents(project, affected_instances, cleanup)?;
    if matches!(cleanup, InterfaceCleanup::None) {
        Ok(result)
    } else {
        Ok(ModuleInterfaceEditResult::Unpublished(impact))
    }
}

fn apply_definition_interface_command(
    definition: &mut ModuleDefinition,
    command: ModuleInterfaceCommand,
) -> Result<(ModuleInterfaceEditResult, InterfaceCleanup), String> {
    let result = match command {
        ModuleInterfaceCommand::PublishParameter {
            name,
            default_value,
            target,
        } => {
            let port = definition
                .graph
                .port_definition(&target, PortDirection::Input)?;
            if !port.data_type.is_property_value_family()
                || !port.data_type.accepts(property_value_type(&default_value))
            {
                return Err("Published parameter default does not match its target".to_string());
            }
            let parameter_id = PublishedParameterId::new();
            definition.interface.parameters.push(PublishedParameter {
                id: parameter_id,
                name,
                data_type: port.data_type,
                default_value,
                target,
            });
            (
                ModuleInterfaceEditResult::PublishedParameter(parameter_id),
                InterfaceCleanup::None,
            )
        }
        ModuleInterfaceCommand::RenameParameter { parameter_id, name } => {
            definition
                .interface
                .parameters
                .iter_mut()
                .find(|parameter| parameter.id == parameter_id)
                .ok_or_else(|| format!("Missing Published parameter {parameter_id}"))?
                .name = name;
            (ModuleInterfaceEditResult::Updated, InterfaceCleanup::None)
        }
        ModuleInterfaceCommand::UnpublishParameter { parameter_id } => {
            remove_by_id(
                &mut definition.interface.parameters,
                |entry| entry.id == parameter_id,
                || format!("Missing Published parameter {parameter_id}"),
            )?;
            (
                ModuleInterfaceEditResult::Updated,
                InterfaceCleanup::Parameter(parameter_id),
            )
        }
        ModuleInterfaceCommand::PublishMediaInput {
            name,
            target,
            required,
            primary,
        } => {
            let port = definition
                .graph
                .port_definition(&target, PortDirection::Input)?;
            require_media_type(port.data_type, "Published media input")?;
            let input_id = PublishedMediaInputId::new();
            definition.interface.media_inputs.push(PublishedMediaInput {
                id: input_id,
                name,
                data_type: port.data_type,
                target,
                required,
                primary,
            });
            (
                ModuleInterfaceEditResult::PublishedMediaInput(input_id),
                InterfaceCleanup::None,
            )
        }
        ModuleInterfaceCommand::RenameMediaInput { input_id, name } => {
            definition
                .interface
                .media_inputs
                .iter_mut()
                .find(|input| input.id == input_id)
                .ok_or_else(|| format!("Missing Published media input {input_id}"))?
                .name = name;
            (ModuleInterfaceEditResult::Updated, InterfaceCleanup::None)
        }
        ModuleInterfaceCommand::RetargetPrimaryMediaInput { input_id, target } => {
            let port = definition
                .graph
                .port_definition(&target, PortDirection::Input)?;
            require_media_type(port.data_type, "Primary Published media input")?;
            let input_index = definition
                .interface
                .media_inputs
                .iter()
                .position(|input| input.id == input_id)
                .ok_or_else(|| format!("Missing Published media input {input_id}"))?;
            let input = &definition.interface.media_inputs[input_index];
            if !input.primary {
                return Err(format!(
                    "Published media input {input_id} is not the primary input"
                ));
            }
            if input.target == target {
                return Err(format!(
                    "Published media input {input_id} already targets {}:{}",
                    target.node_id, target.port
                ));
            }
            if input.data_type != port.data_type {
                return Err(format!(
                    "Primary Published media input {input_id} cannot change from {:?} to {:?}",
                    input.data_type, port.data_type
                ));
            }
            if definition
                .graph
                .connections
                .iter()
                .any(|connection| connection.to == target)
            {
                return Err(format!(
                    "Primary Published media input target {}:{} is driven by a Module connection",
                    target.node_id, target.port
                ));
            }
            let target_is_published_elsewhere = definition
                .interface
                .parameters
                .iter()
                .any(|entry| entry.target == target)
                || definition
                    .interface
                    .media_inputs
                    .iter()
                    .any(|entry| entry.id != input_id && entry.target == target)
                || definition
                    .interface
                    .actions
                    .iter()
                    .any(|entry| entry.target == target);
            if target_is_published_elsewhere {
                return Err(format!(
                    "Primary Published media input target {}:{} is already published",
                    target.node_id, target.port
                ));
            }
            definition.interface.media_inputs[input_index].target = target;
            (ModuleInterfaceEditResult::Updated, InterfaceCleanup::None)
        }
        ModuleInterfaceCommand::UnpublishMediaInput { input_id } => {
            remove_by_id(
                &mut definition.interface.media_inputs,
                |entry| entry.id == input_id,
                || format!("Missing Published media input {input_id}"),
            )?;
            (
                ModuleInterfaceEditResult::Updated,
                InterfaceCleanup::MediaInput(input_id),
            )
        }
    };
    bump_interface_version(definition)?;
    Ok(result)
}

fn cleanup_interface_dependents(
    project: &mut AuthoringProject,
    affected_instances: &[ModuleInstanceId],
    cleanup: InterfaceCleanup,
) -> Result<ModuleInterfaceEditImpact, String> {
    let affected = affected_instances.iter().copied().collect::<HashSet<_>>();
    let mut impact = ModuleInterfaceEditImpact::default();
    match cleanup {
        InterfaceCleanup::None => {}
        InterfaceCleanup::Parameter(parameter_id) => {
            for instance_id in &affected {
                let instance = project
                    .module_instances
                    .get_mut(instance_id)
                    .ok_or_else(|| format!("Missing Module instance {instance_id}"))?;
                impact.removed_parameter_overrides +=
                    usize::from(instance.parameter_overrides.remove(&parameter_id).is_some());
            }
            for_each_affected_invocation_mut(project, &affected, |invocation| {
                impact.removed_automation_tracks +=
                    usize::from(invocation.automation_tracks.remove(&parameter_id).is_some());
                Ok(())
            })?;
        }
        InterfaceCleanup::MediaInput(input_id) => {
            for_each_affected_invocation_mut(project, &affected, |invocation| {
                impact.removed_media_input_bindings +=
                    usize::from(invocation.input_bindings.remove(&input_id).is_some());
                Ok(())
            })?;
        }
    }
    Ok(impact)
}

fn for_each_affected_invocation_mut(
    project: &mut AuthoringProject,
    affected: &HashSet<ModuleInstanceId>,
    mut edit: impl FnMut(&mut ModuleInvocation) -> Result<(), String>,
) -> Result<(), String> {
    for item in project.items.values_mut() {
        if let SourceRef::Module(invocation) = &mut item.source
            && affected.contains(&invocation.instance_id)
        {
            edit(invocation)?;
        }
    }
    for attachment in project.attachments.values_mut() {
        if let AttachmentProcessor::Module(invocation) = &mut attachment.processor
            && affected.contains(&invocation.instance_id)
        {
            edit(invocation)?;
        }
    }
    Ok(())
}

fn require_media_type(data_type: PortDataType, label: &str) -> Result<(), String> {
    matches!(data_type, PortDataType::Image | PortDataType::Audio)
        .then_some(())
        .ok_or_else(|| format!("{label} must address an Image or Audio port"))
}

fn remove_by_id<T>(
    entries: &mut Vec<T>,
    matches: impl Fn(&T) -> bool,
    missing: impl FnOnce() -> String,
) -> Result<(), String> {
    let before = entries.len();
    entries.retain(|entry| !matches(entry));
    (entries.len() != before).then_some(()).ok_or_else(missing)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::model::authoring::{
        ModuleConnection, ModuleDefinitionSharing, ModuleTemplateOrigin, TimelineInterval,
    };
    use crate::model::node::Node;
    use crate::model::project::{IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT, MERGE_SOUNDS_PORT};

    struct InterfaceFixture {
        definition: ModuleDefinition,
        primary_input_id: PublishedMediaInputId,
        output_id: ModuleOutputId,
        original_input: ModulePortAddress,
        replacement_input: ModulePortAddress,
        audio_input: ModulePortAddress,
    }

    fn interface_fixture(connect_replacement_input: bool) -> InterfaceFixture {
        let (mut definition, output_id) = ModuleDefinition::new_image(
            "Published Interface fixture",
            ModuleDefinitionSharing::ReusableTemplate(ModuleTemplateOrigin::Project),
        );
        let original = Node::new_merge("Original");
        let replacement = Node::new_merge("Replacement");
        let audio = Node::new_sound_merge("Audio");
        let original_input = ModulePortAddress {
            node_id: original.id,
            port: MERGE_IMAGES_PORT.to_string(),
        };
        let replacement_input = ModulePortAddress {
            node_id: replacement.id,
            port: MERGE_IMAGES_PORT.to_string(),
        };
        let audio_input = ModulePortAddress {
            node_id: audio.id,
            port: MERGE_SOUNDS_PORT.to_string(),
        };
        let primary_input_id = PublishedMediaInputId::new();
        if connect_replacement_input {
            definition.graph.connections.push(ModuleConnection {
                id: ModuleConnectionId::new(),
                from: ModulePortAddress {
                    node_id: original.id,
                    port: crate::model::project::IMAGE_OUTPUT_PORT.to_string(),
                },
                to: replacement_input.clone(),
                order: 0,
                blend_mode: BlendMode::Normal,
            });
        }
        definition.graph.nodes.extend([
            (original.id, original),
            (replacement.id, replacement),
            (audio.id, audio),
        ]);
        definition.interface.media_inputs.push(PublishedMediaInput {
            id: primary_input_id,
            name: "Host image".to_string(),
            data_type: PortDataType::Image,
            target: original_input.clone(),
            required: false,
            primary: true,
        });
        InterfaceFixture {
            definition,
            primary_input_id,
            output_id,
            original_input,
            replacement_input,
            audio_input,
        }
    }

    fn place_fixture(
        service: &TimelineEditorService,
        fixture: &InterfaceFixture,
    ) -> ModuleInstanceId {
        let project = service.snapshot().expect("snapshot");
        let track_id = project.timelines[&project.root_timeline_id].track_order[0];
        drop(project);
        service
            .add_module_definition(fixture.definition.clone())
            .expect("definition");
        service
            .place_module_item(
                fixture.definition.id,
                ModuleItemPlacement {
                    track_id,
                    name: "Node Clip".to_string(),
                    output_id: fixture.output_id,
                    interval: TimelineInterval::new(
                        MediaTime::new(0, 1).expect("start"),
                        MediaTime::new(1, 1).expect("duration"),
                    )
                    .expect("interval"),
                    layer: 0,
                    parameter_overrides: HashMap::new(),
                    input_bindings: HashMap::new(),
                },
            )
            .expect("placement")
            .1
    }

    #[test]
    fn primary_input_retarget_keeps_public_id_is_instance_local_and_undoes_atomically() {
        let service = TimelineEditorService::create_default("Primary input").expect("service");
        let fixture = interface_fixture(false);
        let reusable_definition_id = fixture.definition.id;
        let instance_id = place_fixture(&service, &fixture);
        let before = service.snapshot().expect("before");

        let (result, private_definition_id, _) = service
            .edit_instance_module_interface(
                instance_id,
                ModuleInterfaceCommand::RetargetPrimaryMediaInput {
                    input_id: fixture.primary_input_id,
                    target: fixture.replacement_input.clone(),
                },
            )
            .expect("retarget primary input");
        assert_eq!(result, ModuleInterfaceEditResult::Updated);
        assert_ne!(private_definition_id, reusable_definition_id);

        let changed = service.snapshot().expect("changed");
        let private_primary = changed.module_definitions[&private_definition_id]
            .interface
            .media_inputs
            .iter()
            .find(|input| input.primary)
            .expect("private primary input");
        assert_eq!(private_primary.id, fixture.primary_input_id);
        assert_eq!(private_primary.target, fixture.replacement_input);
        assert_eq!(
            changed.module_definitions[&reusable_definition_id]
                .interface
                .media_inputs[0]
                .target,
            fixture.original_input,
            "ordinary editing must not mutate the reusable definition"
        );

        assert!(service.undo().expect("undo").is_some());
        assert_eq!(
            service.snapshot().expect("after undo").as_ref(),
            before.as_ref(),
            "copy-on-write and retarget must be one undo step"
        );
    }

    #[test]
    fn interface_retarget_rejects_connected_and_incompatible_media_ports_without_mutation() {
        let service = TimelineEditorService::create_default("Rejected retarget").expect("service");
        let fixture = interface_fixture(true);
        let instance_id = place_fixture(&service, &fixture);
        let before = service.snapshot().expect("before");
        let revision_before = service.revision().expect("revision");

        assert!(
            service
                .edit_instance_module_interface(
                    instance_id,
                    ModuleInterfaceCommand::RetargetPrimaryMediaInput {
                        input_id: fixture.primary_input_id,
                        target: fixture.replacement_input.clone(),
                    },
                )
                .is_err(),
            "an internal connection and a Published input may not own the same port"
        );
        assert!(
            service
                .edit_instance_module_interface(
                    instance_id,
                    ModuleInterfaceCommand::RetargetPrimaryMediaInput {
                        input_id: fixture.primary_input_id,
                        target: fixture.audio_input.clone(),
                    },
                )
                .is_err(),
            "a stable Image input may not silently become Audio"
        );
        assert!(
            service
                .edit_instance_module_interface(
                    instance_id,
                    ModuleInterfaceCommand::RetargetPrimaryMediaInput {
                        input_id: fixture.primary_input_id,
                        target: ModulePortAddress {
                            node_id: fixture.replacement_input.node_id,
                            port: IMAGE_OUTPUT_PORT.to_string(),
                        },
                    },
                )
                .is_err(),
            "an output port may not become a Published input target"
        );
        assert_eq!(service.snapshot().expect("after").as_ref(), before.as_ref());
        assert_eq!(service.revision().expect("revision"), revision_before);
    }
}
