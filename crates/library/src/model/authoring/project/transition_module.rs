use super::super::{
    AuthoringProject, ModuleHostContract, TRANSITION_APPLY_OPERATION, TRANSITION_CATEGORY,
    Transition, TransitionMediaType, TransitionProcessorImplementation,
};
use super::item_placement::ItemPlacementOverlay;
use super::validation::validate_typed_automation;

pub(super) fn validate_transition_processor(
    project: &AuthoringProject,
    transition: &Transition,
    placements: &ItemPlacementOverlay<'_>,
) -> Result<(), String> {
    match &transition.processor.implementation {
        TransitionProcessorImplementation::Operation(operation) => {
            if operation.category != TRANSITION_CATEGORY
                || operation.operation != TRANSITION_APPLY_OPERATION
                || operation.component_id.trim().is_empty()
                || operation.version.trim().is_empty()
            {
                return Err(format!(
                    "Transition {} has an invalid processor identity",
                    transition.id
                ));
            }
            validate_builtin_contract(transition, operation.component_id.as_str())
        }
        TransitionProcessorImplementation::Module(module) => {
            let instance = project
                .module_instances
                .get(&module.instance_id)
                .ok_or_else(|| {
                    format!(
                        "Transition {} has a missing Module instance {}",
                        transition.id, module.instance_id
                    )
                })?;
            let definition = project
                .module_definitions
                .get(&instance.definition_id)
                .ok_or_else(|| {
                    format!(
                        "Transition {} has a missing Module definition {}",
                        transition.id, instance.definition_id
                    )
                })?;
            let ModuleHostContract::Transition(contract) = &definition.host_contract else {
                return Err(format!(
                    "Transition {} Module definition is not a Transition Module",
                    transition.id
                ));
            };
            if contract.media_type != transition.processor.contract.media_type {
                return Err(format!(
                    "Transition {} Module media type does not match its processor contract",
                    transition.id
                ));
            }
            if !transition.processor.contract.parameters.is_empty()
                || !transition.parameters.is_empty()
            {
                return Err(format!(
                    "Transition {} Module controls must use its Published Interface",
                    transition.id
                ));
            }
            if instance
                .parameter_overrides
                .contains_key(&contract.progress_parameter_id)
            {
                return Err(format!(
                    "Transition {} cannot override its host-owned Progress parameter",
                    transition.id
                ));
            }
            contract.validate_definition(definition)?;
            if module.input_bindings.contains_key(&contract.from_input_id)
                || module.input_bindings.contains_key(&contract.to_input_id)
            {
                return Err(format!(
                    "Transition {} cannot bind its protected A/B inputs",
                    transition.id
                ));
            }
            if module
                .automation_tracks
                .contains_key(&contract.progress_parameter_id)
            {
                return Err(format!(
                    "Transition {} cannot automate its host-owned Progress parameter",
                    transition.id
                ));
            }
            for input_id in module.input_bindings.keys() {
                if !definition
                    .interface
                    .media_inputs
                    .iter()
                    .any(|input| input.id == *input_id)
                {
                    return Err(format!(
                        "Transition {} binds an unknown Published media input",
                        transition.id
                    ));
                }
            }
            for input in &definition.interface.media_inputs {
                let protected =
                    input.id == contract.from_input_id || input.id == contract.to_input_id;
                if input.required && !protected && !module.input_bindings.contains_key(&input.id) {
                    return Err(format!(
                        "Transition {} leaves required media input {} unbound",
                        transition.id, input.id
                    ));
                }
                if let Some(binding) = module.input_bindings.get(&input.id) {
                    project.validate_media_binding(
                        None,
                        transition.timeline_id,
                        input,
                        binding,
                        placements,
                    )?;
                }
            }
            for (parameter_id, track) in &module.automation_tracks {
                let parameter = definition
                    .interface
                    .parameters
                    .iter()
                    .find(|parameter| parameter.id == *parameter_id)
                    .ok_or_else(|| {
                        format!(
                            "Transition {} automates an unknown Published parameter",
                            transition.id
                        )
                    })?;
                validate_typed_automation(
                    track,
                    parameter.data_type,
                    &format!(
                        "Transition {} automation for {}",
                        transition.id, parameter.id
                    ),
                    Some(transition.duration),
                )?;
            }
            Ok(())
        }
    }
}

fn validate_builtin_contract(transition: &Transition, component_id: &str) -> Result<(), String> {
    match component_id {
        super::super::CROSS_DISSOLVE_COMPONENT_ID
            if transition.processor.contract.media_type != TransitionMediaType::Image
                || !transition.processor.contract.parameters.is_empty() =>
        {
            Err(format!(
                "Transition {} has an invalid Cross Dissolve contract",
                transition.id
            ))
        }
        super::super::AUDIO_CROSSFADE_COMPONENT_ID
            if transition.processor.contract.media_type != TransitionMediaType::Audio
                || !transition.processor.contract.parameters.is_empty() =>
        {
            Err(format!(
                "Transition {} has an invalid Audio Crossfade contract",
                transition.id
            ))
        }
        _ => Ok(()),
    }
}
