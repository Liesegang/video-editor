//! Asset-owned source-color authoring for the Timeline/Preview Clip facade.

use egui::{RichText, Ui};
use library::editor::project_service::{
    AssetSourceColorInspector, AssetSourceColorInspectorInterpretation,
};
use library::model::asset::{SourceColorAssumption, SourceColorDescription};
use library::model::project::NodeContainer;
use library::EditorService;
use uuid::Uuid;

use crate::action::HistoryManager;

pub(super) fn render(
    ui: &mut Ui,
    clip_id: Uuid,
    project_service: &mut EditorService,
    history_manager: &mut HistoryManager,
    needs_refresh: &mut bool,
) {
    let inspectors =
        match project_service.asset_source_color_inspectors(NodeContainer::Clip(clip_id)) {
            Ok(inspectors) => inspectors,
            Err(error) => {
                let response = ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("Source color cannot be inspected: {error}"),
                );
                crate::qa::register_component_with_metadata(
                    format!("inspector.semantic.source_color:{clip_id}.error"),
                    "asset_source_color_error",
                    response.rect,
                    true,
                    Some(serde_json::json!({
                        "clip_id": clip_id,
                        "message": error.to_string(),
                        "fail_closed": true,
                    })),
                );
                return;
            }
        };
    for inspector in inspectors {
        render_asset(
            ui,
            clip_id,
            &inspector,
            project_service,
            history_manager,
            needs_refresh,
        );
    }
}

fn render_asset(
    ui: &mut Ui,
    clip_id: Uuid,
    inspector: &AssetSourceColorInspector,
    project_service: &mut EditorService,
    history_manager: &mut HistoryManager,
    needs_refresh: &mut bool,
) {
    let response = egui::CollapsingHeader::new(format!("Source Color · {}", inspector.asset_name))
        .id_salt(("asset_source_color", inspector.asset_id))
        .default_open(true)
        .show(ui, |ui| {
            let status = interpretation_label(&inspector.interpretation);
            ui.add(egui::Label::new(RichText::new(&status).strong()).selectable(false));

            if let Some(diagnostic) = &inspector.diagnostic {
                ui.colored_label(ui.visuals().warn_fg_color, diagnostic);
            }
            if inspector.assignment_list_complete {
                render_exact_assignment(
                    ui,
                    inspector,
                    project_service,
                    history_manager,
                    needs_refresh,
                );
            } else {
                render_unavailable_assignment(
                    ui,
                    inspector,
                    project_service,
                    history_manager,
                    needs_refresh,
                );
            }
            render_reprobe(
                ui,
                inspector,
                project_service,
                history_manager,
                needs_refresh,
            );
            render_last_action_diagnostic(ui, inspector.asset_id);
        });
    crate::qa::register_component_with_metadata(
        format!(
            "inspector.semantic.source_color:{clip_id}:{}",
            inspector.asset_id
        ),
        "asset_source_color",
        response.header_response.rect,
        true,
        Some(serde_json::json!({
            "clip_id": clip_id,
            "asset_id": inspector.asset_id,
            "source_node_ids": inspector.source_node_ids,
            "interpretation": status_key(&inspector.interpretation),
            "assignment_list_complete": inspector.assignment_list_complete,
            "candidate_count": inspector.assignable_color_spaces.len(),
            "fail_closed_diagnostic": inspector.diagnostic,
        })),
    );
}

fn render_exact_assignment(
    ui: &mut Ui,
    inspector: &AssetSourceColorInspector,
    project_service: &mut EditorService,
    history_manager: &mut HistoryManager,
    needs_refresh: &mut bool,
) {
    const AUTOMATIC: &str = "Automatic · decoded metadata";
    const MALFORMED: &str = "Malformed assignment · repair required";
    let mut selected = match &inspector.interpretation {
        AssetSourceColorInspectorInterpretation::Assigned { color_space, .. } => {
            color_space.clone()
        }
        AssetSourceColorInspectorInterpretation::Automatic(_) => AUTOMATIC.to_string(),
        AssetSourceColorInspectorInterpretation::AuthoredDescription(_) => {
            "Authored CICP/profile override".to_string()
        }
        AssetSourceColorInspectorInterpretation::Malformed { .. } => MALFORMED.to_string(),
    };
    let previous = selected.clone();
    egui::ComboBox::from_id_salt(("asset_source_space", inspector.asset_id))
        .selected_text(&selected)
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut selected, AUTOMATIC.to_string(), AUTOMATIC);
            for color_space in &inspector.assignable_color_spaces {
                ui.selectable_value(&mut selected, color_space.clone(), color_space);
            }
        });
    if selected != previous {
        let result = if selected == AUTOMATIC {
            project_service.use_detected_asset_source_color(inspector.asset_id)
        } else {
            project_service.assign_asset_source_color_space(inspector.asset_id, &selected)
        };
        finish_action(
            ui,
            inspector.asset_id,
            result,
            project_service,
            history_manager,
            needs_refresh,
        );
    }
}

fn render_unavailable_assignment(
    ui: &mut Ui,
    inspector: &AssetSourceColorInspector,
    project_service: &mut EditorService,
    history_manager: &mut HistoryManager,
    needs_refresh: &mut bool,
) {
    let current = match &inspector.interpretation {
        AssetSourceColorInspectorInterpretation::Assigned { color_space, .. } => color_space,
        AssetSourceColorInspectorInterpretation::Automatic(_) => "Automatic · decoded metadata",
        AssetSourceColorInspectorInterpretation::AuthoredDescription(_) => {
            "Authored CICP/profile override"
        }
        AssetSourceColorInspectorInterpretation::Malformed { .. } => "Malformed assignment",
    };
    ui.add_enabled_ui(false, |ui| {
        egui::ComboBox::from_id_salt(("asset_source_space_unavailable", inspector.asset_id))
            .selected_text(current)
            .show_ui(ui, |ui| {
                ui.label(current);
            });
    });
    if !matches!(
        inspector.interpretation,
        AssetSourceColorInspectorInterpretation::Automatic(_)
    ) && ui.button("Use decoded metadata").clicked()
    {
        finish_action(
            ui,
            inspector.asset_id,
            project_service.use_detected_asset_source_color(inspector.asset_id),
            project_service,
            history_manager,
            needs_refresh,
        );
    }
}

fn render_reprobe(
    ui: &mut Ui,
    inspector: &AssetSourceColorInspector,
    project_service: &mut EditorService,
    history_manager: &mut HistoryManager,
    needs_refresh: &mut bool,
) {
    if ui.button("Re-probe linked media metadata").clicked() {
        match project_service.refresh_asset_source_color_metadata(inspector.asset_id) {
            Ok(refresh) if refresh.changed => {
                clear_last_action_diagnostic(ui, inspector.asset_id);
                push_history(project_service, history_manager);
                *needs_refresh = true;
            }
            Ok(refresh) => set_last_action_diagnostic(
                ui,
                inspector.asset_id,
                refresh
                    .diagnostic
                    .unwrap_or_else(|| "Source metadata is already current".to_string()),
            ),
            Err(error) => set_last_action_diagnostic(ui, inspector.asset_id, error.to_string()),
        }
    }
}

fn finish_action(
    ui: &mut Ui,
    asset_id: Uuid,
    result: Result<(), library::LibraryError>,
    project_service: &EditorService,
    history_manager: &mut HistoryManager,
    needs_refresh: &mut bool,
) {
    match result {
        Ok(()) => {
            clear_last_action_diagnostic(ui, asset_id);
            push_history(project_service, history_manager);
            *needs_refresh = true;
        }
        Err(error) => set_last_action_diagnostic(ui, asset_id, error.to_string()),
    }
}

fn push_history(project_service: &EditorService, history_manager: &mut HistoryManager) {
    match project_service.get_project().read() {
        Ok(project) => history_manager.push_project_state(project.clone()),
        Err(error) => log::error!("Failed to capture source-color Inspector history: {error}"),
    }
}

fn interpretation_label(interpretation: &AssetSourceColorInspectorInterpretation) -> String {
    match interpretation {
        AssetSourceColorInspectorInterpretation::Automatic(description) => {
            format!("Automatic · {}", description_label(description))
        }
        AssetSourceColorInspectorInterpretation::AuthoredDescription(description) => {
            format!("Authored override · {}", description_label(description))
        }
        AssetSourceColorInspectorInterpretation::Assigned {
            color_space,
            exact_active_config,
        } => format!(
            "Assigned · {color_space}{}",
            if *exact_active_config {
                " · exact Project config"
            } else {
                " · stale config"
            }
        ),
        AssetSourceColorInspectorInterpretation::Malformed { detail } => {
            format!("Malformed assignment · {detail}")
        }
    }
}

fn description_label(description: &SourceColorDescription) -> String {
    if description.is_empty() {
        return "no persisted metadata".to_string();
    }
    let mut fields = Vec::new();
    if let Some(assumption) = &description.assumption {
        fields.push(match assumption {
            SourceColorAssumption::UntaggedYuvBt709LimitedV1 => {
                "persisted untagged YUV ≤8-bit → BT.709 limited assumption".to_string()
            }
        });
    }
    for (label, value) in [
        (
            "primaries",
            description
                .primaries
                .as_ref()
                .map(|value| format!("{value:?}")),
        ),
        (
            "transfer",
            description
                .transfer
                .as_ref()
                .map(|value| format!("{value:?}")),
        ),
        (
            "matrix",
            description
                .matrix
                .as_ref()
                .map(|value| format!("{value:?}")),
        ),
        (
            "range",
            description.range.as_ref().map(|value| format!("{value:?}")),
        ),
        (
            "profile",
            description
                .profile
                .as_ref()
                .map(|value| format!("{value:?}")),
        ),
    ] {
        if let Some(value) = value {
            fields.push(format!("{label} {value}"));
        }
    }
    if let Some(bit_depth) = description.bit_depth {
        fields.push(format!("{bit_depth}-bit"));
    }
    fields.join(" · ")
}

fn status_key(interpretation: &AssetSourceColorInspectorInterpretation) -> &'static str {
    match interpretation {
        AssetSourceColorInspectorInterpretation::Automatic(_) => "automatic",
        AssetSourceColorInspectorInterpretation::AuthoredDescription(_) => "authored_description",
        AssetSourceColorInspectorInterpretation::Assigned { .. } => "assigned",
        AssetSourceColorInspectorInterpretation::Malformed { .. } => "malformed",
    }
}

fn diagnostic_id(asset_id: Uuid) -> egui::Id {
    egui::Id::new(("asset_source_color_action_diagnostic", asset_id))
}

fn set_last_action_diagnostic(ui: &Ui, asset_id: Uuid, message: String) {
    ui.data_mut(|data| data.insert_temp(diagnostic_id(asset_id), message));
}

fn clear_last_action_diagnostic(ui: &Ui, asset_id: Uuid) {
    ui.data_mut(|data| data.remove::<String>(diagnostic_id(asset_id)));
}

fn render_last_action_diagnostic(ui: &mut Ui, asset_id: Uuid) {
    if let Some(message) = ui.data(|data| data.get_temp::<String>(diagnostic_id(asset_id))) {
        ui.colored_label(ui.visuals().error_fg_color, message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::asset::{SourceColorRange, SourceMatrixCoefficients};

    #[test]
    fn automatic_assumption_is_visible_in_plain_language() {
        let description = SourceColorDescription {
            assumption: Some(SourceColorAssumption::UntaggedYuvBt709LimitedV1),
            matrix: Some(SourceMatrixCoefficients::Bt709),
            range: Some(SourceColorRange::Limited),
            bit_depth: Some(8),
            ..SourceColorDescription::default()
        };
        let label = description_label(&description);
        assert!(label.contains("untagged YUV"));
        assert!(label.contains("BT.709 limited"));
        assert!(label.contains("8-bit"));
    }
}
