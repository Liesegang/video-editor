use super::*;

pub(super) fn module_effect_controls(
    ui: &mut egui::Ui,
    state: &mut AuthoringUiState,
    resources: &EffectStackResources<'_>,
    attachment: &Attachment,
    invocation: &library::model::authoring::ModuleInvocation,
) {
    let Some(instance) = resources
        .project
        .module_instances
        .get(&invocation.instance_id)
    else {
        ui.colored_label(ui.visuals().error_fg_color, "Missing Module instance");
        return;
    };
    let Some(definition) = resources
        .project
        .module_definitions
        .get(&instance.definition_id)
    else {
        ui.colored_label(ui.visuals().error_fg_color, "Missing Module definition");
        return;
    };

    let context = crate::ui::module_parameter_editor::ModuleParameterContext {
        project: resources.project,
        service: resources.service,
        plugins: resources.plugins,
        owner: library::editor::ModuleParameterOwner::Invocation(
            library::editor::ModuleAutomationOwner::Attachment(attachment.id),
        ),
        instance,
        definition,
    };
    for parameter in &definition.interface.parameters {
        super::super::property_authoring::published_parameter_row(
            ui, state, &context, parameter, None, None,
        );
    }

    let additional_inputs = definition
        .interface
        .media_inputs
        .iter()
        .filter(|input| !input.primary)
        .collect::<Vec<_>>();
    if additional_inputs.is_empty() {
        if definition.interface.parameters.is_empty() {
            ui.weak("Open the Node Editor to add or publish controls.");
        }
        return;
    }

    let Some(timeline_id) = attachment_timeline_id(resources.project, &attachment.owner) else {
        ui.colored_label(ui.visuals().error_fg_color, "Effect owner has no Timeline");
        return;
    };
    ui.add_space(2.0);
    ui.weak("Clip inputs");
    for input in additional_inputs {
        let current = invocation.input_bindings.get(&input.id);
        let excluded_items = match &attachment.owner {
            AttachmentOwner::Item { item_id } => std::slice::from_ref(item_id),
            AttachmentOwner::Timeline { .. } | AttachmentOwner::Track { .. } => &[],
        };
        let control_id = format!("attachment:{}:module_input:{}", attachment.id, input.id);
        let action = crate::ui::module_media_input::media_input_picker(
            ui,
            crate::ui::module_media_input::MediaInputPicker {
                control_id: &control_id,
                project: resources.project,
                timeline_id,
                input,
                current,
                excluded_items,
                required_coverage: None,
                can_inherit: false,
            },
        );
        match action {
            Some(crate::ui::module_media_input::MediaInputPickerAction::Bind(binding)) => {
                match resources.service.bind_attachment_module_input(
                    attachment.id,
                    input.id,
                    binding,
                ) {
                    Ok(_) => state.status = format!("Bound {}", input.name),
                    Err(error) => state.error = Some(error.to_string()),
                }
            }
            Some(crate::ui::module_media_input::MediaInputPickerAction::Unbind) => {
                match resources
                    .service
                    .unbind_attachment_module_input(attachment.id, input.id)
                {
                    Ok(_) => state.status = format!("Unbound {}", input.name),
                    Err(error) => state.error = Some(error.to_string()),
                }
            }
            Some(crate::ui::module_media_input::MediaInputPickerAction::Inherit) => {}
            None => {}
        }
    }
    ui.weak("Bindings use published inputs; internal Node IDs remain private.");
}

fn attachment_timeline_id(
    project: &AuthoringProject,
    owner: &AttachmentOwner,
) -> Option<TimelineId> {
    match owner {
        AttachmentOwner::Timeline { timeline_id } => Some(*timeline_id),
        AttachmentOwner::Track { track_id } => {
            project.tracks.get(track_id).map(|track| track.timeline_id)
        }
        AttachmentOwner::Item { item_id } => project
            .items
            .get(item_id)
            .and_then(|item| project.tracks.get(&item.track_id))
            .map(|track| track.timeline_id),
    }
}
