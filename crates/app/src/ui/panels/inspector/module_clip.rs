//! Inspector controls for one bounded Node Clip invocation.
//!
//! The Timeline owns parameter automation and external media bindings. The
//! Module Definition supplies the finite graph and its published interface.

use library::editor::TimelineEditorService;
use library::model::authoring::{
    AuthoringProject, ModuleDefinition, ModuleInvocation, PublishedParameter, TimelineItem,
};
use library::plugin::PluginManager;

use crate::state::authoring::AuthoringUiState;
use crate::ui::module_parameter_editor::{
    edit_node_clip_parameter, ModuleParameterContext, ModuleParameterEditorOutcome,
    ModuleParameterRowInteraction,
};

use super::property_authoring::{property_row, PropertyRowSpec};
use super::{item_local_time, mode_action_label, value_provenance};

pub(super) fn module_parameters(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    plugins: &PluginManager,
    item: &TimelineItem,
    invocation: &ModuleInvocation,
) {
    let Some(instance) = project.module_instances.get(&invocation.instance_id) else {
        return;
    };
    let Some(definition) = project.module_definitions.get(&instance.definition_id) else {
        return;
    };
    let structured_ensemble = match service.node_clip_text_ensemble_stack(item.id) {
        Ok(stack) => stack,
        Err(error) => {
            state.error = Some(error.to_string());
            None
        }
    };
    let structured_appearance = match service.node_clip_appearance_stack(item.id) {
        Ok(stack) => stack,
        Err(error) => {
            state.error = Some(error.to_string());
            None
        }
    };
    let structured_parameter_ids = structured_ensemble
        .iter()
        .flat_map(|stack| &stack.operations)
        .flat_map(|operation| operation.parameter_ids.iter().copied())
        .chain(
            structured_appearance
                .iter()
                .flat_map(|stack| &stack.operations)
                .flat_map(|operation| operation.parameter_ids.iter().copied()),
        )
        .collect::<std::collections::HashSet<_>>();
    let context = ModuleParameterContext {
        project,
        service,
        plugins,
        item,
        invocation,
        instance,
        definition,
    };
    ui.separator();
    egui::CollapsingHeader::new("Node Clip parameters")
        .default_open(true)
        .show(ui, |ui| {
            if definition.interface.parameters.is_empty() {
                ui.weak("Publish a Node input to expose a reusable control here.");
            }
            for parameter in definition
                .interface
                .parameters
                .iter()
                .filter(|parameter| !structured_parameter_ids.contains(&parameter.id))
            {
                published_parameter_row(ui, state, &context, parameter);
            }
        });
    if let Some(stack) = structured_appearance.as_ref() {
        super::appearance::node_clip_appearance_section(ui, state, &context, stack);
    }
    if let Some(stack) = structured_ensemble.as_ref() {
        super::text_ensemble::node_clip_text_ensemble_section(ui, state, &context, stack);
    }
    module_media_inputs(ui, project, state, service, item, invocation, definition);
}

pub(super) fn published_parameter_row(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    context: &ModuleParameterContext<'_>,
    parameter: &PublishedParameter,
) -> egui::Response {
    let local_time = item_local_time(context.project, state, context.item);
    let automation = context.invocation.automation_tracks.get(&parameter.id);
    let outcome = edit_node_clip_parameter(
        &mut state.inspector,
        context,
        parameter,
        local_time,
        |row| {
            let result = property_row(
                ui,
                row.value,
                &context.project.palette,
                PropertyRowSpec {
                    control_id: &format!(
                        "module_instance:{}:{}",
                        context.instance.id, parameter.id
                    ),
                    label: &parameter.name,
                    definition: row.definition,
                    suffix: "",
                    speed: 0.1,
                    mode_state: row.mode_state,
                    allow_keyframe: row.allow_keyframe,
                    keyframe_disabled_reason: row.keyframe_disabled_reason,
                    allow_expression: false,
                    pending_keyframe: row.pending_keyframe,
                },
            );
            ModuleParameterRowInteraction {
                response: result.response,
                changed: result.changed,
                finished: result.finished,
                mode_action: result.mode_action,
            }
        },
    );
    let ModuleParameterEditorOutcome {
        response,
        mode_action,
        error,
    } = outcome;
    if let Some(error) = error {
        state.error = Some(error);
    }
    if let Some(action) = mode_action {
        state.status = format!("{}: {}", parameter.name, mode_action_label(action));
    }
    value_provenance(
        ui,
        automation.is_some(),
        context
            .instance
            .parameter_overrides
            .contains_key(&parameter.id),
    );
    response
}

fn module_media_inputs(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    item: &TimelineItem,
    invocation: &ModuleInvocation,
    definition: &ModuleDefinition,
) {
    if definition.interface.media_inputs.is_empty() {
        return;
    }
    let Some(host_track) = project.tracks.get(&item.track_id) else {
        return;
    };

    egui::CollapsingHeader::new("Node Clip inputs")
        .default_open(true)
        .show(ui, |ui| {
            for input in &definition.interface.media_inputs {
                let control_id = format!("module_item:{}:module_input:{}", item.id, input.id);
                let action = crate::ui::module_media_input::media_input_picker(
                    ui,
                    crate::ui::module_media_input::MediaInputPicker {
                        control_id: &control_id,
                        project,
                        timeline_id: host_track.timeline_id,
                        input,
                        current: invocation.input_bindings.get(&input.id),
                        excluded_items: std::slice::from_ref(&item.id),
                        required_coverage: None,
                        can_inherit: false,
                    },
                );
                match action {
                    Some(crate::ui::module_media_input::MediaInputPickerAction::Bind(binding)) => {
                        match service.bind_module_input(item.id, input.id, binding) {
                            Ok(_) => state.status = format!("Bound {}", input.name),
                            Err(error) => state.error = Some(error.to_string()),
                        }
                    }
                    Some(crate::ui::module_media_input::MediaInputPickerAction::Unbind) => {
                        match service.unbind_module_input(item.id, input.id) {
                            Ok(_) => state.status = format!("Unbound {}", input.name),
                            Err(error) => state.error = Some(error.to_string()),
                        }
                    }
                    Some(crate::ui::module_media_input::MediaInputPickerAction::Inherit) | None => {
                    }
                }
            }
            ui.weak("Inputs reference clip outputs, not internal Node UUIDs.");
        });
}

#[cfg(test)]
mod tests {
    use crate::ui::property_metadata::published_parameter_keyframe_capability;
    use library::editor::ParticleNodeClipFactory;

    #[test]
    fn particle_inspector_offers_only_runtime_supported_keyframes() {
        let particle = ParticleNodeClipFactory::create("Particle").expect("Particle Node Clip");

        let (rate_allowed, rate_reason) = published_parameter_keyframe_capability(
            &particle.definition,
            particle.parameters.emission_rate,
        );
        assert!(!rate_allowed);
        assert!(rate_reason.is_some_and(|reason| reason.contains("fixed-step")));

        assert_eq!(
            published_parameter_keyframe_capability(
                &particle.definition,
                particle.parameters.color,
            ),
            (true, None)
        );
    }
}
