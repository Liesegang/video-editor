use egui_phosphor::regular as icons;
use library::editor::TimelineEditorService;
use library::model::authoring::{
    AuthoringProject, CompositionInstance, CompositionParameter, CompositionParameterTarget,
    SourceRef, TimelineItem,
};
use library::model::property::PropertyValue;

use crate::state::authoring::AuthoringUiState;

pub(super) fn publication_icon(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    item: &TimelineItem,
    target: CompositionParameterTarget,
    default_value: PropertyValue,
    suggested_name: String,
) {
    let timeline_id = state.active_timeline_id;
    if timeline_id == project.root_timeline_id
        || project
            .tracks
            .get(&item.track_id)
            .is_none_or(|track| track.timeline_id != timeline_id)
    {
        return;
    }
    let existing = project.timelines.get(&timeline_id).and_then(|timeline| {
        timeline
            .published_parameters
            .iter()
            .find(|parameter| parameter.target == target)
    });
    let response = ui
        .add(
            egui::Button::new(icons::LINK_SIMPLE)
                .small()
                .selected(existing.is_some()),
        )
        .on_hover_text(match existing {
            Some(parameter) => format!(
                "Published as '{}'. Right-click to remove the public control.",
                parameter.name
            ),
            None => "Publish this control to each Composition instance".to_string(),
        });
    if response.clicked() && existing.is_none() {
        match service.publish_composition_parameter(
            timeline_id,
            suggested_name,
            target,
            default_value,
        ) {
            Ok((_, _)) => state.status = "Published Composition control".to_string(),
            Err(error) => state.error = Some(error.to_string()),
        }
    }
    if let Some(parameter_id) = existing.map(|parameter| parameter.id) {
        response.context_menu(|ui| {
            if ui
                .button(format!("{} Unpublish control", icons::TRASH))
                .clicked()
            {
                match service.unpublish_composition_parameter(timeline_id, parameter_id) {
                    Ok((cleared, _)) => {
                        state.status = format!(
                            "Unpublished Composition control and cleared {cleared} instance override(s)"
                        );
                    }
                    Err(error) => state.error = Some(error.to_string()),
                }
                ui.close();
            }
        });
    }
}

pub(super) fn instance_parameters(
    ui: &mut egui::Ui,
    project: &AuthoringProject,
    state: &mut AuthoringUiState,
    service: &TimelineEditorService,
    item: &TimelineItem,
    instance: &CompositionInstance,
) {
    let Some(timeline) = project.timelines.get(&instance.timeline_id) else {
        return;
    };
    ui.separator();
    egui::CollapsingHeader::new("Instance controls")
        .default_open(true)
        .show(ui, |ui| {
            if timeline.published_parameters.is_empty() {
                ui.weak("Open this composition and publish the Text or Transform controls you want to reuse.");
                return;
            }
            for parameter in &timeline.published_parameters {
                let overridden = instance.parameter_overrides.contains_key(&parameter.id);
                let initial = instance
                    .parameter_overrides
                    .get(&parameter.id)
                    .cloned()
                    .unwrap_or_else(|| definition_value(project, parameter));
                let model_value = initial.clone();
                let draft_key = format!("composition:{}", parameter.id);
                let (finished, edited_value, reset) = ui
                    .horizontal(|ui| {
                        let label = ui.add_sized(
                            [112.0, 20.0],
                            egui::Label::new(parameter.name.as_str()).sense(egui::Sense::click()),
                        );
                        let (finished, edited_value) = {
                            let value = state
                                .inspector
                                .property_values
                                .entry(draft_key)
                                .or_insert(initial);
                            let finished = super::property_control(ui, value, None, "", 0.1);
                            (finished, value.clone())
                        };
                        let mut reset = false;
                        if overridden {
                            reset = ui
                                .small_button(icons::ARROW_COUNTER_CLOCKWISE)
                                .on_hover_text("Use the current definition value")
                                .clicked();
                            label.context_menu(|ui| {
                                if ui.button("Reset to definition").clicked() {
                                    reset = true;
                                    ui.close();
                                }
                            });
                        } else {
                            ui.label(egui::RichText::new("Definition").weak().small());
                        }
                        (finished, edited_value, reset)
                    })
                    .inner;
                if finished && edited_value != model_value {
                    match service.set_composition_parameter_override(
                        item.id,
                        parameter.id,
                        edited_value,
                    ) {
                        Ok(_) => state.status = format!("Overrode {}", parameter.name),
                        Err(error) => state.error = Some(error.to_string()),
                    }
                }
                if reset {
                    match service.clear_composition_parameter_override(item.id, parameter.id) {
                        Ok(_) => state.status = format!("Reset {} to definition", parameter.name),
                        Err(error) => state.error = Some(error.to_string()),
                    }
                }
            }
            ui.weak("Overrides belong only to this placement; sibling instances keep their own values.");
        });
}

fn definition_value(project: &AuthoringProject, parameter: &CompositionParameter) -> PropertyValue {
    let Some(item) = project.items.get(&parameter.target.item_id()) else {
        return parameter.default_value.clone();
    };
    match &parameter.target {
        CompositionParameterTarget::TextContent { .. } => match &item.source {
            SourceRef::Text { text } => PropertyValue::String(text.clone()),
            _ => parameter.default_value.clone(),
        },
        CompositionParameterTarget::ItemProperty { property_key, .. } => item
            .authored_properties
            .get(property_key)
            .and_then(|property| property.value())
            .cloned()
            .unwrap_or_else(|| parameter.default_value.clone()),
    }
}
