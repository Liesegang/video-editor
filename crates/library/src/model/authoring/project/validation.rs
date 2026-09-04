use std::collections::{HashMap, HashSet};

use crate::model::property::{PropertyMap, PropertyValue};

use super::super::{
    AttachmentOwner, AttachmentStage, AuthoringProject, AutomatableParameter, AutomationTrack,
    BuiltinEffectInstance, CompositionParameter, DurationPolicy, MediaInputBinding, MediaTime,
    ModuleInvocation, ProcessorParameterContract, PublishedParameter, TextEnsembleOperation,
    TimelineItem, TimelineItemId, TransitionMediaType, property_value_type,
    text_ensemble_direct_contract_is_compatible,
};

pub(super) fn validate_text_ensemble_operations(
    operations: &[TextEnsembleOperation],
    item_id: TimelineItemId,
) -> Result<(), String> {
    let mut ids = HashSet::new();
    let mut decorator_phase = false;
    for operation in operations {
        if operation.id.is_nil() || !ids.insert(operation.id) {
            return Err(format!(
                "Timeline item {item_id} repeats or omits a Text Ensemble operation ID"
            ));
        }
        if operation.operation.component_id.trim().is_empty()
            || operation.operation.version.trim().is_empty()
        {
            return Err(format!(
                "Text Ensemble operation {} has an incomplete identity",
                operation.id
            ));
        }
        let supported = matches!(
            (
                operation.operation.category.as_str(),
                operation.operation.operation.as_str(),
            ),
            (
                crate::plugin::EFFECTOR_CATEGORY,
                crate::plugin::EFFECTOR_APPLY_OPERATION
            ) | (
                crate::plugin::DECORATOR_CATEGORY,
                crate::plugin::DECORATOR_APPLY_OPERATION
            )
        );
        if !supported {
            return Err(format!(
                "Text Ensemble operation {} is not an Effector or Decorator",
                operation.id
            ));
        }
        match operation.operation.category.as_str() {
            crate::plugin::DECORATOR_CATEGORY => decorator_phase = true,
            crate::plugin::EFFECTOR_CATEGORY if decorator_phase => {
                return Err(format!(
                    "Text Ensemble operation {} places an Effector after the Decorator phase",
                    operation.id
                ));
            }
            _ => {}
        }
        if !text_ensemble_direct_contract_is_compatible(&operation.declared_ports) {
            return Err(format!(
                "Text Ensemble operation {} requires unsupported media inputs",
                operation.id
            ));
        }
        let declared_properties = operation
            .declared_ports
            .iter()
            .filter_map(|port| port.key.strip_prefix(crate::plugin::PROPERTY_PORT_PREFIX))
            .collect::<HashSet<_>>();
        let authored_properties = operation
            .properties
            .iter()
            .map(|(key, _)| key.as_str())
            .collect::<HashSet<_>>();
        if declared_properties != authored_properties {
            return Err(format!(
                "Text Ensemble operation {} properties do not match its declared ports",
                operation.id
            ));
        }
        validate_authored_properties(
            &operation.properties,
            &format!("Text Ensemble operation {}", operation.id),
        )?;
    }
    Ok(())
}

pub(super) fn validate_composition_parameter_value(
    parameter: &CompositionParameter,
    value: &PropertyValue,
) -> Result<(), String> {
    if parameter.data_type.accepts(property_value_type(value)) {
        Ok(())
    } else {
        Err(format!(
            "Composition parameter {} has an incompatible value",
            parameter.id
        ))
    }
}

pub(super) fn validate_parameter_value(
    parameter: &PublishedParameter,
    value: &PropertyValue,
) -> Result<(), String> {
    if parameter.data_type.accepts(property_value_type(value)) {
        Ok(())
    } else {
        Err(format!(
            "Published parameter {} has an incompatible value",
            parameter.id
        ))
    }
}

pub(super) fn validate_authored_properties(
    properties: &PropertyMap,
    owner: &str,
) -> Result<(), String> {
    for (key, property) in properties.iter() {
        if key.trim().is_empty() || property.evaluator.trim().is_empty() {
            return Err(format!("{owner} has an invalid authored Property"));
        }
        match property.evaluator.as_str() {
            "constant" if property.value().is_none() => {
                return Err(format!("{owner} Property '{key}' has no constant value"));
            }
            "keyframe" => {
                let raw_count = match property.properties.get("keyframes") {
                    Some(PropertyValue::Array(values)) => values.len(),
                    _ => {
                        return Err(format!("{owner} Property '{key}' has no Keyframe array"));
                    }
                };
                let keyframes = property.keyframes();
                if keyframes.is_empty() || keyframes.len() != raw_count {
                    return Err(format!("{owner} Property '{key}' has invalid Keyframes"));
                }
                let mut ids = HashSet::new();
                let mut previous = None;
                for keyframe in keyframes {
                    let time = keyframe.time.into_inner();
                    if !time.is_finite()
                        || time < 0.0
                        || !ids.insert(keyframe.id)
                        || previous.is_some_and(|previous| previous >= time)
                    {
                        return Err(format!("{owner} Property '{key}' has invalid Keyframes"));
                    }
                    previous = Some(time);
                }
            }
            "expression" if property.expression_text().is_none() || property.value().is_none() => {
                return Err(format!(
                    "{owner} Property '{key}' has an incomplete expression"
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

pub(super) fn validate_automation(
    track: &AutomationTrack,
    parameter: &PublishedParameter,
) -> Result<(), String> {
    validate_typed_automation(
        track,
        parameter.data_type,
        &format!("Automation for {}", parameter.id),
        None,
    )
}

pub(super) fn validate_typed_automation(
    track: &AutomationTrack,
    data_type: crate::model::project::PortDataType,
    owner: &str,
    maximum_time: Option<MediaTime>,
) -> Result<(), String> {
    if track.keyframes.is_empty() {
        return Err(format!("{owner} has no Keyframes"));
    }
    let mut ids = HashSet::new();
    let mut previous = None;
    for keyframe in &track.keyframes {
        if keyframe.time.is_negative()
            || maximum_time.is_some_and(|maximum| keyframe.time > maximum)
            || !ids.insert(keyframe.id)
            || previous.is_some_and(|time| time >= keyframe.time)
        {
            return Err(format!("{owner} has invalid Keyframes"));
        }
        if !data_type.accepts(property_value_type(&keyframe.value)) {
            return Err(format!("{owner} has an incompatible Keyframe value"));
        }
        previous = Some(keyframe.time);
    }
    Ok(())
}

pub(super) fn validate_automatable_parameters(
    parameters: &HashMap<String, AutomatableParameter>,
    contracts: &[ProcessorParameterContract],
    owner: &str,
    maximum_automation_time: Option<MediaTime>,
) -> Result<(), String> {
    let mut keys = HashSet::new();
    for contract in contracts {
        if contract.key.trim().is_empty() || !keys.insert(contract.key.as_str()) {
            return Err(format!("{owner} contract has duplicate parameter keys"));
        }
        if !contract
            .data_type
            .accepts(property_value_type(&contract.default_value))
        {
            return Err(format!(
                "{owner} parameter '{}' has an invalid default",
                contract.key
            ));
        }
        let parameter = parameters
            .get(&contract.key)
            .ok_or_else(|| format!("{owner} is missing parameter '{}'", contract.key))?;
        if !contract
            .data_type
            .accepts(property_value_type(&parameter.value))
        {
            return Err(format!(
                "{owner} parameter '{}' has an invalid value",
                contract.key
            ));
        }
        if let Some(automation) = &parameter.automation {
            validate_typed_automation(
                automation,
                contract.data_type,
                &format!("{owner} parameter '{}' automation", contract.key),
                maximum_automation_time,
            )?;
        }
    }
    if parameters.len() != contracts.len() {
        return Err(format!(
            "{owner} has parameters outside its persisted contract"
        ));
    }
    Ok(())
}

pub(super) fn validate_duration_policy(
    item: &TimelineItem,
    nested_duration: MediaTime,
    policy: &DurationPolicy,
) -> Result<(), String> {
    if let DurationPolicy::Responsive {
        intro_end,
        outro_start,
    } = policy
    {
        if intro_end.is_negative() || *intro_end > *outro_start || *outro_start > nested_duration {
            return Err(format!("Item {} has invalid Responsive markers", item.id));
        }
        let minimum = intro_end.checked_add(nested_duration.checked_sub(*outro_start)?)?;
        if item.interval.duration < minimum {
            return Err(format!(
                "Item {} is too short for Responsive timing",
                item.id
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_attachment_stage(
    owner: &AttachmentOwner,
    stage: AttachmentStage,
) -> Result<(), String> {
    owner
        .supports_stage(stage)
        .then_some(())
        .ok_or_else(|| format!("Attachment stage {stage:?} is invalid for {owner:?}"))
}

pub(super) fn validate_builtin_effect(
    effect: &BuiltinEffectInstance,
    stage: AttachmentStage,
) -> Result<(), String> {
    if effect.operation.category.trim().is_empty()
        || effect.operation.component_id.trim().is_empty()
        || effect.operation.operation.trim().is_empty()
        || effect.operation.version.trim().is_empty()
    {
        return Err("Built-in Effect has an incomplete operation identity".to_string());
    }
    if !matches!(
        effect.contract.input_type,
        crate::model::project::PortDataType::Image | crate::model::project::PortDataType::Audio
    ) || effect.contract.input_type != effect.contract.output_type
    {
        return Err("Built-in Effect contract must preserve one media type".to_string());
    }
    if effect.contract.input_type != attachment_media_type(stage)? {
        return Err("Built-in Effect media type is incompatible with its Stage".to_string());
    }
    validate_automatable_parameters(
        &effect.parameters,
        &effect.contract.parameters,
        "Built-in Effect",
        None,
    )
}

pub(super) fn validate_transitions(project: &AuthoringProject) -> Result<(), String> {
    for (transition_id, transition) in &project.transitions {
        if *transition_id != transition.id {
            return Err("Transition map key does not match its ID".to_string());
        }
        let timeline = project
            .timelines
            .get(&transition.timeline_id)
            .ok_or_else(|| format!("Transition {} has no Timeline", transition.id))?;
        if transition.from_item_id == transition.to_item_id {
            return Err(format!(
                "Transition {} must connect two distinct Timeline items",
                transition.id
            ));
        }
        let from = project
            .items
            .get(&transition.from_item_id)
            .ok_or_else(|| format!("Transition {} has a missing from item", transition.id))?;
        let to = project
            .items
            .get(&transition.to_item_id)
            .ok_or_else(|| format!("Transition {} has a missing to item", transition.id))?;
        let from_track = project
            .tracks
            .get(&from.track_id)
            .ok_or_else(|| format!("Transition {} has a missing from Track", transition.id))?;
        let to_track = project
            .tracks
            .get(&to.track_id)
            .ok_or_else(|| format!("Transition {} has a missing to Track", transition.id))?;
        if from_track.timeline_id != timeline.id || to_track.timeline_id != timeline.id {
            return Err(format!(
                "Transition {} crosses a Timeline boundary",
                transition.id
            ));
        }
        if from.track_id != to.track_id {
            return Err(format!(
                "Transition {} must connect items on one Track",
                transition.id
            ));
        }

        let interval = transition
            .interval()
            .map_err(|error| format!("Transition {} has invalid timing: {error}", transition.id))?;
        if interval.end()? > timeline.duration {
            return Err(format!(
                "Transition {} extends beyond its Timeline",
                transition.id
            ));
        }
        let interval_end = interval.end()?;
        if from.interval.start > interval.start || from.interval.end()? < transition.edit_point {
            return Err(format!(
                "Transition {} from item does not own the visible range through its edit point",
                transition.id
            ));
        }
        if to.interval.start > transition.edit_point || to.interval.end()? < interval_end {
            return Err(format!(
                "Transition {} to item does not own the visible range from its edit point",
                transition.id
            ));
        }

        let operation = &transition.processor.operation;
        if operation.category != super::super::TRANSITION_CATEGORY
            || operation.operation != super::super::TRANSITION_APPLY_OPERATION
            || operation.component_id.trim().is_empty()
            || operation.version.trim().is_empty()
        {
            return Err(format!(
                "Transition {} has an invalid processor identity",
                transition.id
            ));
        }
        match operation.component_id.as_str() {
            super::super::CROSS_DISSOLVE_COMPONENT_ID
                if transition.processor.contract.media_type != TransitionMediaType::Image
                    || !transition.processor.contract.parameters.is_empty() =>
            {
                return Err(format!(
                    "Transition {} has an invalid Cross Dissolve contract",
                    transition.id
                ));
            }
            super::super::AUDIO_CROSSFADE_COMPONENT_ID
                if transition.processor.contract.media_type != TransitionMediaType::Audio
                    || !transition.processor.contract.parameters.is_empty() =>
            {
                return Err(format!(
                    "Transition {} has an invalid Audio Crossfade contract",
                    transition.id
                ));
            }
            _ => {}
        }
        let output = transition.processor.contract.media_type.output_kind();
        if !project.item_supports_output(from, output)?
            || !project.item_supports_output(to, output)?
        {
            return Err(format!(
                "Transition {} source items do not provide the required media",
                transition.id
            ));
        }
        validate_automatable_parameters(
            &transition.parameters,
            &transition.processor.contract.parameters,
            &format!("Transition {}", transition.id),
            Some(transition.duration),
        )?;
    }
    Ok(())
}

pub(super) fn attachment_media_type(
    stage: AttachmentStage,
) -> Result<crate::model::project::PortDataType, String> {
    stage.effect_media_type().ok_or_else(|| {
        "ItemTimeMap requires a future Behavior contract, not a media Effect".to_string()
    })
}

pub(super) fn invocation_input_items(invocation: &ModuleInvocation) -> Vec<TimelineItemId> {
    invocation
        .input_bindings
        .values()
        .map(|binding| match binding {
            MediaInputBinding::TimelineItemOutput { item_id, .. } => *item_id,
        })
        .collect()
}
