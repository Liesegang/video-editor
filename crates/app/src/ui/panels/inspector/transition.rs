use egui_phosphor::regular as icons;
use library::editor::TimelineEditorService;
use library::model::authoring::{
    AuthoringProject, MediaTime, Transition, TransitionAlignment, TransitionId, TransitionMediaType,
};
use library::model::property::PropertyValue;
use ordered_float::OrderedFloat;

use crate::state::authoring::AuthoringUiState;
use crate::ui::panels::node_editor::open_transition_document;

pub(super) fn transition_inspector(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    transition_id: TransitionId,
) {
    let Some(transition) = project.transitions.get(&transition_id) else {
        return;
    };
    let processor_name = processor_name(project, transition);
    let editor_tooltip = if transition.processor.module_processor().is_some() {
        "Edit processing logic in Node Editor"
    } else {
        "Customize in Node Editor (creates a private Transition Module)"
    };
    let open_editor = super::section_title(
        ui,
        transition_icon(transition.processor.contract.media_type),
        transition_kind(transition.processor.contract.media_type),
        &processor_name,
        Some((icons::SHARE_NETWORK, editor_tooltip)),
    );
    if let Some(response) = open_editor {
        crate::qa::register_component_with_metadata(
            format!("inspector.transition.open_editor:{transition_id}"),
            "inspector_action",
            response.rect,
            response.enabled(),
            Some(serde_json::json!({
                "transition_id": transition_id,
                "action": "open_node_editor",
                "module_backed": transition.processor.module_processor().is_some(),
            })),
        );
        if response.clicked() {
            if let Err(error) = open_transition_document(project, state, service, transition_id) {
                state.error = Some(error.to_string());
            }
        }
    }

    participant_summary(ui, project, transition);
    ui.separator();
    timing_controls(ui, state, service, transition);
    module_controls(ui, project, state, service, transition);
}

fn participant_summary(ui: &mut egui::Ui, project: &AuthoringProject, transition: &Transition) {
    let from = project
        .items
        .get(&transition.from_item_id)
        .map_or("Missing clip", |item| item.name.as_str());
    let to = project
        .items
        .get(&transition.to_item_id)
        .map_or("Missing clip", |item| item.name.as_str());
    ui.horizontal_wrapped(|ui| {
        ui.weak("Clips");
        ui.label(from);
        ui.label(icons::ARROW_RIGHT);
        ui.label(to);
    });
}

fn timing_controls(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    transition: &Transition,
) {
    egui::CollapsingHeader::new("Timing")
        .default_open(true)
        .show(ui, |ui| {
            duration_control(ui, state, service, transition);
            alignment_control(ui, state, service, transition);
            ui.horizontal(|ui| {
                super::property_label(
                    ui,
                    &format!("transition:{}:edit_point", transition.id),
                    "Edit point",
                );
                ui.label(format!("{:.3} s", transition.edit_point.to_seconds_f64()));
            });
        });
}

fn duration_control(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    transition: &Transition,
) {
    let model_value = PropertyValue::Number(OrderedFloat(transition.duration.to_seconds_f64()));
    let draft_key = format!("transition:{}:duration", transition.id);
    let control_id = draft_key.clone();
    let (finished, edited_value) = ui
        .horizontal(|ui| {
            super::property_label(ui, &control_id, "Duration");
            let value = state
                .inspector
                .property_values
                .entry(draft_key)
                .or_insert_with(|| model_value.clone());
            let finished = super::property_control(ui, &control_id, value, None, " s", 0.01);
            (finished, value.clone())
        })
        .inner;
    if !finished || edited_value == model_value {
        return;
    }
    let PropertyValue::Number(seconds) = edited_value else {
        state.error = Some("Transition duration must be a number".to_string());
        return;
    };
    let duration = match MediaTime::from_seconds_f64(seconds.into_inner(), 1_000_000) {
        Ok(duration) if duration > MediaTime::zero() => duration,
        Ok(_) => {
            state.error = Some("Transition duration must be greater than zero".to_string());
            return;
        }
        Err(error) => {
            state.error = Some(error);
            return;
        }
    };
    match service.set_transition_duration(transition.id, duration) {
        Ok(_) => state.status = "Updated Transition duration".to_string(),
        Err(error) => state.error = Some(error.to_string()),
    }
}

fn alignment_control(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    transition: &Transition,
) {
    let mut alignment = transition.alignment;
    let response = ui
        .horizontal(|ui| {
            super::property_label(
                ui,
                &format!("transition:{}:alignment", transition.id),
                "Alignment",
            );
            egui::ComboBox::from_id_salt(("transition-alignment", transition.id))
                .selected_text(alignment_name(alignment))
                .show_ui(ui, |ui| {
                    for candidate in [
                        TransitionAlignment::StartAtEdit,
                        TransitionAlignment::CenteredOnEdit,
                        TransitionAlignment::EndAtEdit,
                    ] {
                        ui.selectable_value(&mut alignment, candidate, alignment_name(candidate));
                    }
                })
                .response
        })
        .inner;
    crate::qa::register_component_with_metadata(
        format!("inspector.transition.alignment:{}", transition.id),
        "inspector_property_control",
        response.rect,
        response.enabled(),
        Some(serde_json::json!({
            "transition_id": transition.id,
            "alignment": alignment_name(alignment),
        })),
    );
    if alignment != transition.alignment {
        match service.set_transition_alignment(transition.id, alignment) {
            Ok(_) => state.status = "Updated Transition alignment".to_string(),
            Err(error) => state.error = Some(error.to_string()),
        }
    }
}

fn module_controls(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    transition: &Transition,
) {
    let Some(module) = transition.processor.module_processor() else {
        return;
    };
    let Some(instance) = project.module_instances.get(&module.instance_id) else {
        ui.colored_label(ui.visuals().error_fg_color, "Missing Module instance");
        return;
    };
    let Some(definition) = project.module_definitions.get(&instance.definition_id) else {
        ui.colored_label(ui.visuals().error_fg_color, "Missing Module definition");
        return;
    };
    let instance_path = state.active_instance_path.as_ref();
    let concrete_target = match instance_path {
        Some(path) => {
            match project.resolve_transition_module_instance_target(path, transition.id) {
                Ok(target) => Some(target),
                Err(error) => {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                    return;
                }
            }
        }
        None => None,
    };
    let effective_controls = match concrete_target.as_ref() {
        Some(target) => match project.effective_transition_module_controls(target) {
            Ok(controls) => Some(controls),
            Err(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error);
                return;
            }
        },
        None => None,
    };
    let placement_overrides = match concrete_target.as_ref() {
        Some(target) => match project.transition_module_instance_overrides(target) {
            Ok(controls) => controls,
            Err(error) => {
                ui.colored_label(ui.visuals().error_fg_color, error);
                return;
            }
        },
        None => None,
    };
    // All mutations use the successfully resolved target, never the raw UI
    // navigation path. A stale selection is diagnosed above and cannot fall
    // through into definition-scoped edits.
    let edit_instance_path = concrete_target.as_ref().map(|target| &target.instance_path);
    let automation_owner =
        crate::ui::automation_lanes::transition_owner(transition.id, edit_instance_path);
    let Some(service_owner) =
        crate::ui::automation_lanes::transition_service_owner(&automation_owner)
    else {
        ui.colored_label(
            ui.visuals().error_fg_color,
            "Transition automation owner is unavailable",
        );
        return;
    };
    let Some(local_time) = project
        .timelines
        .get(&state.active_timeline_id)
        .and_then(|timeline| {
            MediaTime::from_frame_index(state.timeline.current_frame, timeline.fps).ok()
        })
        .and_then(|time| {
            crate::ui::automation_lanes::local_time_for_timeline(project, &automation_owner, time)
        })
    else {
        ui.colored_label(
            ui.visuals().error_fg_color,
            "Transition local time is unavailable",
        );
        return;
    };
    let progress_parameter_id = definition
        .host_contract
        .transition()
        .map(|contract| contract.progress_parameter_id);
    let parameters = definition
        .interface
        .parameters
        .iter()
        .filter(|parameter| Some(parameter.id) != progress_parameter_id)
        .collect::<Vec<_>>();
    let additional_inputs = definition
        .interface
        .media_inputs
        .iter()
        .filter(|input| !definition.host_contract.protects_media_input(input.id))
        .collect::<Vec<_>>();
    let has_additional_inputs = !additional_inputs.is_empty();

    ui.separator();
    egui::CollapsingHeader::new("Module controls")
        .default_open(true)
        .show(ui, |ui| {
            if parameters.is_empty() && !has_additional_inputs {
                ui.weak(
                    "Publish parameters or clip inputs in the Node Editor to expose them here.",
                );
            }
            for parameter in parameters {
                let nested_scope =
                    edit_instance_path.is_some_and(|path| !path.composition_items.is_empty());
                let placement_static_override = nested_scope
                    && placement_overrides.is_some_and(|controls| {
                        controls.parameter_overrides.contains_key(&parameter.id)
                    });
                let placement_automation_override = nested_scope
                    && placement_overrides.is_some_and(|controls| {
                        controls.automation_tracks.contains_key(&parameter.id)
                    });
                let overridden = if nested_scope {
                    placement_static_override || placement_automation_override
                } else {
                    instance.parameter_overrides.contains_key(&parameter.id)
                };
                let static_value = effective_controls
                    .as_ref()
                    .map(|controls| &controls.parameter_overrides)
                    .unwrap_or(&instance.parameter_overrides)
                    .get(&parameter.id)
                    .cloned()
                    .unwrap_or_else(|| parameter.default_value.clone());
                let automation = effective_controls.as_ref().map_or_else(
                    || module.automation_tracks.get(&parameter.id),
                    |controls| controls.automation_tracks.get(&parameter.id),
                );
                let model_value = automation
                    .and_then(|track| track.evaluate_at(local_time).ok())
                    .unwrap_or(static_value);
                let mode_state = automation.map_or_else(
                    || super::PropertyModeState::constant(local_time.to_seconds_f64()),
                    |track| {
                        super::PropertyModeState::from_keyframe_times(
                            local_time.to_seconds_f64(),
                            track
                                .keyframes
                                .iter()
                                .map(|keyframe| keyframe.time.to_seconds_f64()),
                        )
                    },
                );
                let draft_key = format!(
                    "transition:{}:module:{}:{:?}",
                    transition.id, parameter.id, edit_instance_path
                );
                let control_id = format!(
                    "transition:{}:module_parameter:{}",
                    transition.id, parameter.id
                );
                let (row_result, edited_value, reset) = ui
                    .horizontal(|ui| {
                        let value = state
                            .inspector
                            .property_values
                            .entry(draft_key)
                            .or_insert_with(|| model_value.clone());
                        let row_result = super::property_row(
                            ui,
                            value,
                            super::PropertyRowSpec {
                                control_id: &control_id,
                                label: &parameter.name,
                                definition: None,
                                suffix: "",
                                speed: 0.1,
                                mode_state,
                                allow_expression: false,
                            },
                        );
                        let edited_value = value.clone();
                        let reset = overridden
                            && ui
                                .small_button(icons::ARROW_COUNTER_CLOCKWISE)
                                .on_hover_text("Reset to the inherited value")
                                .clicked();
                        (row_result, edited_value, reset)
                    })
                    .inner;
                if row_result.finished && edited_value != model_value {
                    let result = super::property_authoring::commit_transition_parameter_value(
                        service,
                        &service_owner,
                        parameter.id,
                        automation,
                        edited_value.clone(),
                        local_time,
                    );
                    match result {
                        Ok(_) => state.status = format!("Updated {}", parameter.name),
                        Err(error) => state.error = Some(error),
                    }
                }
                if let Some(action) = row_result.mode_action {
                    let result = super::property_authoring::apply_transition_parameter_mode_action(
                        service,
                        &service_owner,
                        parameter.id,
                        automation,
                        edited_value,
                        local_time,
                        action,
                    );
                    match result {
                        Ok(()) => {
                            state.status =
                                format!("{}: {}", parameter.name, super::mode_action_label(action));
                        }
                        Err(error) => state.error = Some(error),
                    }
                } else if reset {
                    let result = match parameter_reset_action(
                        edit_instance_path.is_some(),
                        placement_static_override,
                        placement_automation_override,
                    ) {
                        TransitionParameterResetAction::ClearConcreteParameter => {
                            let Some(path) = edit_instance_path else {
                                state.error =
                                    Some("Transition instance path is unavailable".to_string());
                                continue;
                            };
                            service.clear_transition_module_instance_parameter(
                                path,
                                transition.id,
                                parameter.id,
                            )
                        }
                        TransitionParameterResetAction::InheritConcreteAutomation => {
                            let Some(path) = edit_instance_path else {
                                state.error =
                                    Some("Transition instance path is unavailable".to_string());
                                continue;
                            };
                            service.inherit_transition_module_instance_parameter_automation(
                                path,
                                transition.id,
                                parameter.id,
                            )
                        }
                        TransitionParameterResetAction::ClearDefinitionParameter => {
                            service.clear_module_parameter_override(instance.id, parameter.id)
                        }
                    };
                    match result {
                        Ok(_) => state.status = format!("Reset {}", parameter.name),
                        Err(error) => state.error = Some(error.to_string()),
                    }
                }
                super::value_provenance(ui, automation.is_some(), overridden);
            }
            if has_additional_inputs {
                ui.add_space(2.0);
                ui.weak("Clip inputs");
            }
            let excluded_items = [transition.from_item_id, transition.to_item_id];
            for input in additional_inputs {
                let can_inherit = edit_instance_path
                    .is_some_and(|path| !path.composition_items.is_empty())
                    && placement_overrides
                        .is_some_and(|controls| controls.input_bindings.contains_key(&input.id));
                let current = effective_controls.as_ref().map_or_else(
                    || module.input_bindings.get(&input.id),
                    |controls| controls.input_bindings.get(&input.id),
                );
                let control_id = format!("transition:{}:module_input:{}", transition.id, input.id);
                let action = crate::ui::module_media_input::media_input_picker(
                    ui,
                    crate::ui::module_media_input::MediaInputPicker {
                        control_id: &control_id,
                        project,
                        timeline_id: transition.timeline_id,
                        input,
                        current,
                        excluded_items: &excluded_items,
                        required_coverage: transition.interval().ok(),
                        can_inherit,
                    },
                );
                match action {
                    Some(crate::ui::module_media_input::MediaInputPickerAction::Bind(binding)) => {
                        let result = if let Some(path) = edit_instance_path {
                            service.bind_transition_module_input_at_instance(
                                path,
                                transition.id,
                                input.id,
                                binding,
                            )
                        } else {
                            service.bind_transition_module_input(transition.id, input.id, binding)
                        };
                        match result {
                            Ok(_) => state.status = format!("Bound {}", input.name),
                            Err(error) => state.error = Some(error.to_string()),
                        }
                    }
                    Some(crate::ui::module_media_input::MediaInputPickerAction::Unbind) => {
                        let result = edit_instance_path.map_or_else(
                            || service.unbind_transition_module_input(transition.id, input.id),
                            |path| {
                                service.unbind_transition_module_input_at_instance(
                                    path,
                                    transition.id,
                                    input.id,
                                )
                            },
                        );
                        match result {
                            Ok(_) => state.status = format!("Unbound {}", input.name),
                            Err(error) => state.error = Some(error.to_string()),
                        }
                    }
                    Some(crate::ui::module_media_input::MediaInputPickerAction::Inherit) => {
                        let Some(path) = edit_instance_path else {
                            continue;
                        };
                        match service.inherit_transition_module_input_at_instance(
                            path,
                            transition.id,
                            input.id,
                        ) {
                            Ok(_) => state.status = format!("Inherited {}", input.name),
                            Err(error) => state.error = Some(error.to_string()),
                        }
                    }
                    None => {}
                }
            }
            ui.weak("Progress is driven by the Transition timing and remains read-only here.");
            if has_additional_inputs {
                ui.weak("Clip bindings target Published inputs; internal Node IDs stay private.");
            }
        });
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TransitionParameterResetAction {
    ClearDefinitionParameter,
    ClearConcreteParameter,
    InheritConcreteAutomation,
}

fn parameter_reset_action(
    has_concrete_path: bool,
    has_static_override: bool,
    has_automation_override: bool,
) -> TransitionParameterResetAction {
    if !has_concrete_path {
        TransitionParameterResetAction::ClearDefinitionParameter
    } else if has_static_override || !has_automation_override {
        TransitionParameterResetAction::ClearConcreteParameter
    } else {
        TransitionParameterResetAction::InheritConcreteAutomation
    }
}

fn processor_name(project: &AuthoringProject, transition: &Transition) -> String {
    if let Some(name) = transition
        .processor
        .module_processor()
        .and_then(|module| project.module_instances.get(&module.instance_id))
        .and_then(|instance| project.module_definitions.get(&instance.definition_id))
        .map(|definition| definition.name.clone())
    {
        return name;
    }
    if transition.processor.is_builtin_cross_dissolve() {
        return "Cross Dissolve".to_string();
    }
    if transition.processor.is_builtin_audio_crossfade() {
        return "Audio Crossfade".to_string();
    }
    transition.processor.operation().map_or_else(
        || "Unavailable processor".to_string(),
        |operation| operation.component_id.clone(),
    )
}

const fn transition_kind(media_type: TransitionMediaType) -> &'static str {
    match media_type {
        TransitionMediaType::Image => "Image Transition",
        TransitionMediaType::Audio => "Audio Transition",
    }
}

const fn transition_icon(media_type: TransitionMediaType) -> &'static str {
    match media_type {
        TransitionMediaType::Image => icons::ARROWS_MERGE,
        TransitionMediaType::Audio => icons::WAVEFORM,
    }
}

const fn alignment_name(alignment: TransitionAlignment) -> &'static str {
    match alignment {
        TransitionAlignment::StartAtEdit => "Start at edit",
        TransitionAlignment::CenteredOnEdit => "Centered on edit",
        TransitionAlignment::EndAtEdit => "End at edit",
    }
}

#[cfg(test)]
mod tests {
    use super::{parameter_reset_action, TransitionParameterResetAction};

    #[test]
    fn automation_only_placement_reset_restores_inherited_track() {
        assert_eq!(
            parameter_reset_action(true, false, true),
            TransitionParameterResetAction::InheritConcreteAutomation
        );
        assert_eq!(
            parameter_reset_action(true, true, true),
            TransitionParameterResetAction::ClearConcreteParameter
        );
        assert_eq!(
            parameter_reset_action(false, false, false),
            TransitionParameterResetAction::ClearDefinitionParameter
        );
    }
}
