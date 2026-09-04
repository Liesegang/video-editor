//! Validation and lookup for concrete nested Transition Module controls.

use std::collections::HashSet;

use super::super::{
    AuthoringProject, EffectiveTransitionModuleControls, InstancePath, MediaInputBinding,
    ModuleHostContract, ModuleInstanceId, PublishedMediaInputId, SourceRef, TransitionId,
    TransitionModuleInstanceOverrides, TransitionModuleInstanceTarget,
    TransitionModulePlacementTarget, TransitionModuleProcessor,
};
use super::item_placement::ItemPlacementOverlay;
use super::validation::{validate_parameter_value, validate_typed_automation};

/// Every persisted control store for one Transition Module instance.
///
/// Definition controls and concrete nested-placement differences must be
/// edited together when their Published Interface changes. Keeping that
/// traversal here prevents editor services from knowing where sparse records
/// are persisted.
pub(crate) enum TransitionModuleControlsMut<'a> {
    Definition(&'a mut TransitionModuleProcessor),
    Instance(&'a mut TransitionModuleInstanceOverrides),
}

pub(super) fn validate_transition_module_instance_overrides(
    project: &AuthoringProject,
    placements: &ItemPlacementOverlay<'_>,
) -> Result<(), String> {
    let mut targets = HashSet::new();
    for (owner_item_id, controls) in project.transition_module_instance_override_records() {
        if controls.is_empty() {
            return Err(format!(
                "Composition item {owner_item_id} has an empty Transition Module instance override"
            ));
        }
        let target = controls
            .target
            .concrete(project.root_timeline_id, owner_item_id);
        let timeline_id = project.validate_instance_path(&target.instance_path, placements)?;
        let transition = project
            .transitions
            .get(&target.transition_id)
            .ok_or_else(|| {
                "Transition Module instance override targets a missing Transition".to_string()
            })?;
        if transition.timeline_id != timeline_id {
            return Err(format!(
                "Transition {} does not belong to its concrete InstancePath",
                transition.id
            ));
        }
        let module = transition.processor.module_processor().ok_or_else(|| {
            format!(
                "Transition {} instance override targets a non-Module processor",
                transition.id
            )
        })?;
        if module.instance_id != target.module_instance_id {
            return Err(format!(
                "Transition {} instance override has a stale Module instance",
                transition.id
            ));
        }
        if !targets.insert(target.clone()) {
            return Err(format!(
                "Transition {} repeats a concrete instance override",
                transition.id
            ));
        }
        let instance = project
            .module_instances
            .get(&target.module_instance_id)
            .ok_or_else(|| {
                "Transition instance override has a missing Module instance".to_string()
            })?;
        let definition = project
            .module_definitions
            .get(&instance.definition_id)
            .ok_or_else(|| {
                "Transition instance override has a missing Module definition".to_string()
            })?;
        let ModuleHostContract::Transition(contract) = &definition.host_contract else {
            return Err(
                "Transition instance override selects a general-purpose Module".to_string(),
            );
        };

        for (parameter_id, value) in &controls.parameter_overrides {
            if *parameter_id == contract.progress_parameter_id {
                return Err(format!(
                    "Transition {} cannot override host-owned Progress",
                    transition.id
                ));
            }
            let parameter = definition
                .interface
                .parameters
                .iter()
                .find(|parameter| parameter.id == *parameter_id)
                .ok_or_else(|| {
                    "Transition instance overrides an unpublished parameter".to_string()
                })?;
            validate_parameter_value(parameter, value)?;
        }
        for (input_id, binding) in &controls.input_bindings {
            if *input_id == contract.from_input_id || *input_id == contract.to_input_id {
                return Err(format!(
                    "Transition {} cannot override host-owned A/B inputs",
                    transition.id
                ));
            }
            let input = definition
                .interface
                .media_inputs
                .iter()
                .find(|input| input.id == *input_id)
                .ok_or_else(|| {
                    "Transition instance binds an unpublished media input".to_string()
                })?;
            contract.validate_additional_media_input(input.data_type)?;
            if let Some(binding) = binding {
                project.validate_media_binding(None, timeline_id, input, binding, placements)?;
            }
        }
        for input in &definition.interface.media_inputs {
            let protected = input.id == contract.from_input_id || input.id == contract.to_input_id;
            let is_bound = controls.input_bindings.get(&input.id).map_or_else(
                || module.input_bindings.contains_key(&input.id),
                Option::is_some,
            );
            if input.required && !protected && !is_bound {
                return Err(format!(
                    "Transition {} instance leaves required media input {} unbound",
                    transition.id, input.id
                ));
            }
        }
        for (parameter_id, automation) in &controls.automation_tracks {
            if *parameter_id == contract.progress_parameter_id {
                return Err(format!(
                    "Transition {} cannot automate host-owned Progress",
                    transition.id
                ));
            }
            let parameter = definition
                .interface
                .parameters
                .iter()
                .find(|parameter| parameter.id == *parameter_id)
                .ok_or_else(|| {
                    "Transition instance automates an unpublished parameter".to_string()
                })?;
            if let Some(automation) = automation {
                validate_typed_automation(
                    automation,
                    parameter.data_type,
                    &format!(
                        "Transition {} instance automation for {}",
                        transition.id, parameter.id
                    ),
                    Some(transition.duration),
                )?;
            }
        }
    }
    Ok(())
}

impl AuthoringProject {
    pub fn resolve_transition_module_instance_target(
        &self,
        instance_path: &InstancePath,
        transition_id: super::super::TransitionId,
    ) -> Result<TransitionModuleInstanceTarget, String> {
        let timeline_id =
            self.validate_instance_path(instance_path, &ItemPlacementOverlay::empty())?;
        let transition = self
            .transitions
            .get(&transition_id)
            .ok_or_else(|| format!("Missing Transition {transition_id}"))?;
        if transition.timeline_id != timeline_id {
            return Err(format!(
                "Transition {transition_id} does not belong to InstancePath Timeline {timeline_id}"
            ));
        }
        let module = transition
            .processor
            .module_processor()
            .ok_or_else(|| format!("Transition {transition_id} does not use a Module processor"))?;
        Ok(TransitionModuleInstanceTarget {
            instance_path: instance_path.clone(),
            transition_id,
            module_instance_id: module.instance_id,
        })
    }

    pub fn transition_module_instance_overrides(
        &self,
        target: &TransitionModuleInstanceTarget,
    ) -> Result<Option<&TransitionModuleInstanceOverrides>, String> {
        let Some((owner_id, relative)) = self.placement_target(target)? else {
            return Ok(None);
        };
        let owner = self
            .items
            .get(&owner_id)
            .ok_or_else(|| format!("Missing Composition owner item {owner_id}"))?;
        let SourceRef::Composition(instance) = &owner.source else {
            return Err(format!(
                "Transition override owner {owner_id} is not a Composition"
            ));
        };
        Ok(instance
            .transition_module_overrides
            .iter()
            .find(|controls| controls.target == relative))
    }

    pub fn effective_transition_module_controls(
        &self,
        target: &TransitionModuleInstanceTarget,
    ) -> Result<EffectiveTransitionModuleControls, String> {
        let resolved = self.resolve_transition_module_instance_target(
            &target.instance_path,
            target.transition_id,
        )?;
        if &resolved != target {
            return Err("Transition Module instance target is stale".to_string());
        }
        let transition = self
            .transitions
            .get(&target.transition_id)
            .ok_or_else(|| format!("Missing Transition {}", target.transition_id))?;
        let module = transition
            .processor
            .module_processor()
            .ok_or_else(|| format!("Transition {} does not use a Module", target.transition_id))?;
        let instance = self
            .module_instances
            .get(&target.module_instance_id)
            .ok_or_else(|| format!("Missing Module instance {}", target.module_instance_id))?;
        let mut effective = EffectiveTransitionModuleControls {
            target: target.clone(),
            parameter_overrides: instance.parameter_overrides.clone(),
            input_bindings: module.input_bindings.clone(),
            automation_tracks: module.automation_tracks.clone(),
        };
        if let Some(controls) = self.transition_module_instance_overrides(target)? {
            effective
                .parameter_overrides
                .extend(controls.parameter_overrides.clone());
            apply_sparse_overrides(&mut effective.input_bindings, &controls.input_bindings);
            apply_sparse_overrides(
                &mut effective.automation_tracks,
                &controls.automation_tracks,
            );
        }
        Ok(effective)
    }

    pub(crate) fn edit_transition_module_instance_overrides<T>(
        &mut self,
        target: &TransitionModuleInstanceTarget,
        edit: impl FnOnce(&mut TransitionModuleInstanceOverrides) -> Result<T, String>,
    ) -> Result<T, String> {
        let Some((owner_id, relative)) = self.placement_target(target)? else {
            return Err("Root Timeline Transition controls are definition-scoped".to_string());
        };
        let owner = self
            .items
            .get_mut(&owner_id)
            .ok_or_else(|| format!("Missing Composition owner item {owner_id}"))?;
        let SourceRef::Composition(instance) = &mut owner.source else {
            return Err(format!(
                "Transition override owner {owner_id} is not a Composition"
            ));
        };
        let index = instance
            .transition_module_overrides
            .iter()
            .position(|controls| controls.target == relative)
            .unwrap_or_else(|| {
                instance
                    .transition_module_overrides
                    .push(TransitionModuleInstanceOverrides::new(relative));
                instance.transition_module_overrides.len() - 1
            });
        let result = edit(&mut instance.transition_module_overrides[index])?;
        if instance.transition_module_overrides[index].is_empty() {
            instance.transition_module_overrides.remove(index);
        }
        Ok(result)
    }

    pub(crate) fn remove_transition_module_instance_overrides(
        &mut self,
        transition_id: super::super::TransitionId,
    ) {
        for item in self.items.values_mut() {
            if let SourceRef::Composition(instance) = &mut item.source {
                instance
                    .transition_module_overrides
                    .retain(|controls| controls.target.transition_id != transition_id);
            }
        }
    }

    /// Removes concrete controls whose relative target path passes through an
    /// item being cascade-deleted from a nested Timeline definition.
    pub(crate) fn remove_transition_module_overrides_through_item(
        &mut self,
        item_id: super::super::TimelineItemId,
    ) {
        for item in self.items.values_mut() {
            if let SourceRef::Composition(instance) = &mut item.source {
                instance
                    .transition_module_overrides
                    .retain(|controls| !controls.target.composition_items.contains(&item_id));
            }
        }
    }

    pub(crate) fn transition_module_instance_override_records(
        &self,
    ) -> Vec<(
        super::super::TimelineItemId,
        &TransitionModuleInstanceOverrides,
    )> {
        self.items
            .values()
            .filter_map(|item| match &item.source {
                SourceRef::Composition(instance) => Some((item.id, instance)),
                _ => None,
            })
            .flat_map(|(item_id, instance)| {
                instance
                    .transition_module_overrides
                    .iter()
                    .map(move |controls| (item_id, controls))
            })
            .collect()
    }

    pub(crate) fn for_each_transition_module_input_binding(
        &self,
        mut visit: impl FnMut(TransitionId, PublishedMediaInputId, &MediaInputBinding),
    ) {
        for transition in self.transitions.values() {
            if let Some(module) = transition.processor.module_processor() {
                for (input_id, binding) in &module.input_bindings {
                    visit(transition.id, *input_id, binding);
                }
            }
        }
        for (_, controls) in self.transition_module_instance_override_records() {
            for (input_id, binding) in &controls.input_bindings {
                if let Some(binding) = binding {
                    visit(controls.target.transition_id, *input_id, binding);
                }
            }
        }
    }

    pub(crate) fn for_each_affected_transition_module_controls_mut(
        &mut self,
        affected: &HashSet<ModuleInstanceId>,
        mut edit: impl FnMut(TransitionId, TransitionModuleControlsMut<'_>),
    ) {
        for transition in self.transitions.values_mut() {
            if let Some(module) = transition.processor.module_processor_mut()
                && affected.contains(&module.instance_id)
            {
                edit(
                    transition.id,
                    TransitionModuleControlsMut::Definition(module),
                );
            }
        }
        for item in self.items.values_mut() {
            let SourceRef::Composition(instance) = &mut item.source else {
                continue;
            };
            instance.transition_module_overrides.retain_mut(|controls| {
                if affected.contains(&controls.target.module_instance_id) {
                    edit(
                        controls.target.transition_id,
                        TransitionModuleControlsMut::Instance(controls),
                    );
                }
                !controls.is_empty()
            });
        }
    }

    fn placement_target(
        &self,
        target: &TransitionModuleInstanceTarget,
    ) -> Result<
        Option<(
            super::super::TimelineItemId,
            TransitionModulePlacementTarget,
        )>,
        String,
    > {
        let resolved = self.resolve_transition_module_instance_target(
            &target.instance_path,
            target.transition_id,
        )?;
        if &resolved != target {
            return Err("Transition Module instance target is stale".to_string());
        }
        let Some((&owner_item_id, tail)) = target.instance_path.composition_items.split_first()
        else {
            return Ok(None);
        };
        let owner = self
            .items
            .get(&owner_item_id)
            .ok_or_else(|| format!("Missing Composition owner item {owner_item_id}"))?;
        let owner_timeline_id = self
            .tracks
            .get(&owner.track_id)
            .ok_or_else(|| format!("Composition owner {owner_item_id} has no Track"))?
            .timeline_id;
        if owner_timeline_id != self.root_timeline_id {
            return Err("Transition instance overrides must be owned by a root-Timeline Composition placement".to_string());
        }
        Ok(Some((
            owner_item_id,
            TransitionModulePlacementTarget {
                composition_items: tail.to_vec(),
                transition_id: target.transition_id,
                module_instance_id: target.module_instance_id,
            },
        )))
    }
}

fn apply_sparse_overrides<K, V>(
    target: &mut std::collections::HashMap<K, V>,
    overrides: &std::collections::HashMap<K, Option<V>>,
) where
    K: Copy + Eq + std::hash::Hash,
    V: Clone,
{
    for (key, value) in overrides {
        if let Some(value) = value {
            target.insert(*key, value.clone());
        } else {
            target.remove(key);
        }
    }
}
