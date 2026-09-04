//! Atomic reusable Transition Module assignment form.
//!
//! The form owns only transient Published Interface bindings. The Project is
//! untouched until Apply submits one complete editor-service transaction.

use std::collections::HashMap;

use egui_phosphor::regular as icons;
use library::editor::TimelineEditorService;
use library::model::authoring::{
    AuthoringProject, MediaInputBinding, PublishedMediaInput, PublishedMediaInputId,
};

use crate::state::authoring::AuthoringUiState;
use crate::ui::dialogs::{dialog_button, dialog_footer, DialogButtonRole};
use crate::ui::module_media_input::{media_input_picker, MediaInputPicker, MediaInputPickerAction};
use crate::ui::widgets::modal::Modal;

const DIALOG_WIDTH: f32 = 480.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AssignmentAction {
    Apply,
    Cancel,
}

pub(super) fn show(
    context: &egui::Context,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
) {
    let Some(draft) = state.timeline.transition_module_assignment.as_ref() else {
        return;
    };
    let transition_id = draft.transition_id;
    let definition_id = draft.definition_id;
    let Some(transition) = project.transitions.get(&transition_id) else {
        state.timeline.transition_module_assignment = None;
        state.error = Some(format!("Missing Transition {transition_id}"));
        return;
    };
    let Some(definition) = project.module_definitions.get(&definition_id) else {
        state.timeline.transition_module_assignment = None;
        state.error = Some(format!("Missing Module definition {definition_id}"));
        return;
    };
    let Some(contract) = definition.host_contract.transition() else {
        state.timeline.transition_module_assignment = None;
        state.error = Some("Selected Module is not a Transition Module".to_string());
        return;
    };
    if let Err(error) = contract.validate_definition(definition) {
        state.timeline.transition_module_assignment = None;
        state.error = Some(error);
        return;
    }

    let required_inputs = definition
        .interface
        .media_inputs
        .iter()
        .filter(|input| input.required && !definition.host_contract.protects_media_input(input.id))
        .cloned()
        .collect::<Vec<_>>();
    let excluded_items = [transition.from_item_id, transition.to_item_id];
    let escape_requested = context.input(|input| input.key_pressed(egui::Key::Escape));
    let mut action = None;
    let mut apply_enabled = false;
    let shown = Modal::dialog("Assign Transition Module", DIALOG_WIDTH).show(context, |ui| {
        ui.label(egui::RichText::new(&definition.name).strong().size(15.0));
        ui.add_space(4.0);
        ui.weak("Choose a Timeline clip for each required input.");
        ui.add_space(10.0);

        let Some(draft) = state.timeline.transition_module_assignment.as_mut() else {
            return;
        };
        for input in &required_inputs {
            let control_id = format!("transition.assignment.input:{transition_id}:{}", input.id);
            let picker_action = media_input_picker(
                ui,
                MediaInputPicker {
                    control_id: &control_id,
                    project,
                    timeline_id: transition.timeline_id,
                    input,
                    current: draft.input_bindings.get(&input.id),
                    excluded_items: &excluded_items,
                    required_coverage: transition.interval().ok(),
                    can_inherit: false,
                },
            );
            match picker_action {
                Some(MediaInputPickerAction::Bind(binding)) => {
                    draft.input_bindings.insert(input.id, binding);
                }
                Some(MediaInputPickerAction::Unbind) => {
                    draft.input_bindings.remove(&input.id);
                }
                Some(MediaInputPickerAction::Inherit) | None => {}
            }
        }
        apply_enabled = required_bindings_complete(&required_inputs, &draft.input_bindings);

        let footer = dialog_footer(ui, |ui| {
            let cancel = dialog_button(ui, "Cancel", DialogButtonRole::Secondary);
            crate::qa::register_component_with_metadata(
                format!("transition.assignment.cancel:{transition_id}"),
                "transition_assignment_dialog_button",
                cancel.rect,
                cancel.enabled(),
                Some(serde_json::json!({"role": "secondary"})),
            );
            if cancel.clicked() {
                action = Some(AssignmentAction::Cancel);
            }
            let apply = ui
                .add_enabled_ui(apply_enabled, |ui| {
                    dialog_button(
                        ui,
                        format!("{} Apply", icons::CHECK),
                        DialogButtonRole::Primary,
                    )
                })
                .inner;
            crate::qa::register_component_with_metadata(
                format!("transition.assignment.apply:{transition_id}"),
                "transition_assignment_dialog_button",
                apply.rect,
                apply.enabled(),
                Some(serde_json::json!({"role": "primary"})),
            );
            if apply.clicked() {
                action = Some(AssignmentAction::Apply);
            }
        });
        crate::qa::register_component(
            format!("transition.assignment.footer:{transition_id}"),
            "transition_assignment_dialog_footer",
            footer.response.rect,
        );
    });

    if let Some(shown) = shown {
        let bound_count = state
            .timeline
            .transition_module_assignment
            .as_ref()
            .map_or(0, |draft| draft.input_bindings.len());
        crate::qa::register_component_with_metadata(
            format!("transition.assignment.dialog:{transition_id}:{definition_id}"),
            "transition_assignment_dialog",
            shown.response.rect,
            true,
            Some(serde_json::json!({
                "transition_id": transition_id,
                "definition_id": definition_id,
                "required_input_count": required_inputs.len(),
                "required_input_ids": required_inputs.iter().map(|input| input.id).collect::<Vec<_>>(),
                "bound_input_count": bound_count,
                "apply_enabled": apply_enabled,
                "content_width": DIALOG_WIDTH,
            })),
        );
    }
    if action.is_none() && escape_requested {
        action = Some(AssignmentAction::Cancel);
    }
    match action {
        Some(AssignmentAction::Cancel) => {
            state.timeline.transition_module_assignment = None;
        }
        Some(AssignmentAction::Apply) => apply(state, service),
        None => {}
    }
}

fn required_bindings_complete(
    required_inputs: &[PublishedMediaInput],
    bindings: &HashMap<PublishedMediaInputId, MediaInputBinding>,
) -> bool {
    !required_inputs.is_empty()
        && required_inputs
            .iter()
            .all(|input| bindings.contains_key(&input.id))
}

fn apply(state: &mut AuthoringUiState, service: &TimelineEditorService) {
    let Some(draft) = state.timeline.transition_module_assignment.as_ref() else {
        return;
    };
    let result = service.assign_transition_module_with_controls(
        draft.transition_id,
        draft.definition_id,
        draft.input_bindings.clone(),
        HashMap::new(),
    );
    match result {
        Ok(_) => {
            state.timeline.transition_module_assignment = None;
            state.status = "Applied reusable Transition Module".to_string();
        }
        Err(error) => state.error = Some(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::authoring::{
        InstanceLocator, ItemOutputStage, MediaOutputKind, ModulePortAddress,
    };
    use library::model::project::PortDataType;

    fn input() -> PublishedMediaInput {
        PublishedMediaInput {
            id: PublishedMediaInputId::new(),
            name: "Matte".to_string(),
            data_type: PortDataType::Image,
            target: ModulePortAddress {
                node_id: uuid::Uuid::new_v4(),
                port: "image".to_string(),
            },
            required: true,
            primary: false,
        }
    }

    #[test]
    fn every_required_published_input_must_be_bound_before_apply() {
        let first = input();
        let second = input();
        let mut bindings = HashMap::new();
        assert!(!required_bindings_complete(
            &[first.clone(), second.clone()],
            &bindings
        ));
        for input in [&first, &second] {
            bindings.insert(
                input.id,
                MediaInputBinding::TimelineItemOutput {
                    locator: InstanceLocator::SameTimeline,
                    item_id: library::model::authoring::TimelineItemId::new(),
                    output: MediaOutputKind::Image,
                    stage: ItemOutputStage::PostTransform,
                },
            );
        }
        assert!(required_bindings_complete(&[first, second], &bindings));
    }

    #[test]
    fn a_form_without_required_inputs_never_submits() {
        assert!(!required_bindings_complete(&[], &HashMap::new()));
    }
}
