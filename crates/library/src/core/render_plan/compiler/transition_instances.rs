//! Compilation of sparse, concrete nested Transition Module controls.

use std::collections::HashMap;
use std::sync::Arc;

use crate::model::authoring::{AuthoringProject, MediaInputBinding, ModuleDefinitionId};

use super::super::{
    CompiledModuleDefinition, CompiledModuleInvocation, CompiledTransitionInstanceControls,
    DependencyIndex, ModuleHost, TimelineInstanceRangeDependency,
};

pub(super) fn compile_transition_instance_controls(
    project: &AuthoringProject,
    definitions: &HashMap<ModuleDefinitionId, Arc<CompiledModuleDefinition>>,
    invocations: &[CompiledModuleInvocation],
    dependencies: &mut DependencyIndex,
) -> Result<
    HashMap<
        crate::model::authoring::TransitionModuleInstanceTarget,
        CompiledTransitionInstanceControls,
    >,
    String,
> {
    let mut compiled = HashMap::new();
    for (owner_item_id, controls) in project.transition_module_instance_override_records() {
        let target = controls
            .target
            .concrete(project.root_timeline_id, owner_item_id);
        let effective = project.effective_transition_module_controls(&target)?;
        let transition = project
            .transitions
            .get(&target.transition_id)
            .ok_or_else(|| {
                "Concrete Transition controls target a missing Transition".to_string()
            })?;
        let host = ModuleHost::Transition {
            timeline_id: transition.timeline_id,
            transition_id: transition.id,
        };
        let base_index = dependencies.invocation_indices.get(&host).ok_or_else(|| {
            format!(
                "Transition {} instance controls have no base invocation",
                transition.id
            )
        })?;
        let base = invocations.get(*base_index).ok_or_else(|| {
            format!(
                "Transition {} base invocation index is invalid",
                transition.id
            )
        })?;
        if base.instance_id != target.module_instance_id {
            return Err(format!(
                "Transition {} instance controls reference a stale Module instance",
                transition.id
            ));
        }
        let definition = definitions.get(&base.definition_id).ok_or_else(|| {
            format!(
                "Transition {} instance controls have no compiled Module definition",
                transition.id
            )
        })?;
        let output = definition.outputs.get(&base.output_id).ok_or_else(|| {
            format!(
                "Transition {} instance controls select a missing compiled Output",
                transition.id
            )
        })?;
        let interval = transition.interval()?;
        dependencies.transition_instance_ranges.insert(
            target.clone(),
            TimelineInstanceRangeDependency {
                target: target.clone(),
                timeline_id: transition.timeline_id,
                start: interval.start,
                duration: interval.duration,
            },
        );
        dependencies
            .definition_transition_instances
            .entry(base.definition_id)
            .or_default()
            .push(target.clone());
        dependencies
            .instance_transition_instances
            .entry(base.instance_id)
            .or_default()
            .push(target.clone());
        for binding in controls
            .input_bindings
            .iter()
            .filter(|(input_id, _)| output.reachable_media_inputs.contains(input_id))
            .filter_map(|(_, binding)| binding.as_ref())
        {
            let MediaInputBinding::TimelineItemOutput { item_id, .. } = binding;
            dependencies
                .transition_instance_media_consumers
                .entry(*item_id)
                .or_default()
                .push(target.clone());
        }
        let instance_controls = CompiledTransitionInstanceControls {
            target: target.clone(),
            parameter_overrides: effective.parameter_overrides,
            input_bindings: effective.input_bindings,
            automation_tracks: effective.automation_tracks,
        };
        if compiled.insert(target, instance_controls).is_some() {
            return Err(format!(
                "Transition {} repeats concrete instance controls",
                transition.id
            ));
        }
    }
    Ok(compiled)
}
