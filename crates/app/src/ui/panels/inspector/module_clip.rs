//! Inspector controls for one bounded Node Clip invocation.
//!
//! The Timeline owns parameter automation and external media bindings. The
//! Module Definition supplies the finite graph and its published interface.

use library::editor::TimelineEditorService;
use library::model::authoring::{
    AuthoringProject, ModuleDefinition, ModuleInstance, ModuleInvocation, PublishedParameter,
    TimelineItem,
};
use library::plugin::PluginManager;

use crate::state::authoring::AuthoringUiState;
use crate::ui::property_metadata::{
    node_property_definition, published_parameter_keyframe_capability,
};
use crate::ui::widgets::property_mode::PropertyModeState;

use super::property_authoring::{
    apply_module_parameter_mode_action, commit_module_parameter_value, property_row,
    PropertyRowSpec,
};
use super::{item_local_time, mode_action_label, value_provenance};

pub(super) struct ModuleParameterContext<'a> {
    pub(super) project: &'a AuthoringProject,
    pub(super) service: &'a TimelineEditorService,
    pub(super) plugins: &'a PluginManager,
    pub(super) item: &'a TimelineItem,
    pub(super) invocation: &'a ModuleInvocation,
    pub(super) instance: &'a ModuleInstance,
    pub(super) definition: &'a ModuleDefinition,
}

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
) {
    let key = format!("module:{}", parameter.id);
    let base_value = context
        .instance
        .parameter_overrides
        .get(&parameter.id)
        .cloned()
        .unwrap_or_else(|| parameter.default_value.clone());
    let local_time = item_local_time(context.project, state, context.item);
    let local_seconds = local_time
        .as_ref()
        .map_or(0.0, |time| time.to_seconds_f64());
    let automation = context.invocation.automation_tracks.get(&parameter.id);
    let initial = automation
        .and_then(|track| {
            local_time
                .as_ref()
                .ok()
                .and_then(|time| track.evaluate_at(*time).ok())
        })
        .unwrap_or(base_value);
    let model_value = initial.clone();
    let mode_state = automation.map_or_else(
        || PropertyModeState::constant(local_seconds),
        |track| {
            PropertyModeState::from_keyframe_times(
                local_seconds,
                track
                    .keyframes
                    .iter()
                    .map(|keyframe| keyframe.time.to_seconds_f64()),
            )
        },
    );
    let property_definition =
        published_parameter_definition(context.plugins, context.definition, parameter);
    let (allow_keyframe, keyframe_disabled_reason) =
        published_parameter_keyframe_capability(context.definition, parameter.id);
    let (finished, mode_action, edited_value) = {
        let value = state
            .inspector
            .property_values
            .entry(key)
            .or_insert(initial);
        let result = property_row(
            ui,
            value,
            &context.project.palette,
            PropertyRowSpec {
                control_id: &format!("module_instance:{}:{}", context.instance.id, parameter.id),
                label: &parameter.name,
                definition: property_definition.as_ref(),
                suffix: "",
                speed: 0.1,
                mode_state,
                allow_keyframe,
                keyframe_disabled_reason,
                allow_expression: false,
            },
        );
        (result.finished, result.mode_action, value.clone())
    };
    if finished && edited_value != model_value {
        let result = match &local_time {
            Ok(time) => commit_module_parameter_value(
                context.service,
                context.item.id,
                context.instance.id,
                parameter.id,
                automation,
                edited_value.clone(),
                *time,
            ),
            Err(error) => Err(error.clone()),
        };
        if let Err(error) = result {
            state.error = Some(error);
        }
    }
    if let Some(action) = mode_action {
        let result = match &local_time {
            Ok(time) => apply_module_parameter_mode_action(
                context.service,
                context.item.id,
                parameter.id,
                automation,
                edited_value,
                *time,
                action,
            ),
            Err(error) => Err(error.clone()),
        };
        if let Err(error) = result {
            state.error = Some(error);
        } else {
            state.status = format!("{}: {}", parameter.name, mode_action_label(action));
        }
    }
    value_provenance(
        ui,
        automation.is_some(),
        context
            .instance
            .parameter_overrides
            .contains_key(&parameter.id),
    );
}

fn published_parameter_definition(
    plugins: &PluginManager,
    definition: &ModuleDefinition,
    parameter: &library::model::authoring::PublishedParameter,
) -> Option<library::model::property::PropertyDefinition> {
    let node = definition.graph.nodes.get(&parameter.target.node_id)?;
    let property_name = library::plugin::property_name_from_port(&parameter.target.port)
        .unwrap_or(parameter.target.port.as_str());
    node_property_definition(plugins, node, property_name)
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
    use super::*;
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
