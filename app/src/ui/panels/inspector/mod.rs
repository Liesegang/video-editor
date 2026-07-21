use std::sync::{Arc, RwLock};

use egui::Ui;
use library::model::project::Project;
use library::EditorService;

use crate::{action::HistoryManager, state::context::EditorContext};

pub mod action_handler;
mod clip_timing;
mod evaluation;
mod facade;
mod node_inspector;
mod path_effect;
mod presentation;
pub mod properties;
mod property_authoring;
mod property_inference;
mod selection;
mod semantic_clip;

use facade::{render_semantic_graph_facade, FacadeOwnerKind};
use node_inspector::{node_display_type, render_node};
use presentation::{render_multi_selection_notice, render_node_time_source};
use selection::{resolve_selection, InspectorSelection};

#[cfg(test)]
use clip_timing::inspector_timing_drag_config;
#[cfg(test)]
use facade::{
    content_connection_metadata, facade_output_metadata, facade_output_text,
    is_content_flow_connection, native_value_nodes, operation_category, operation_state_label,
    semantic_visual_sources, source_kind, FacadeOutputMode, OPERATION_CATEGORY_SECTIONS,
};
#[cfg(test)]
use node_inspector::{
    canonical_native_property_definitions, plugin_operation_property_definitions,
};
#[cfg(test)]
use property_inference::property_label;
#[cfg(test)]
use selection::connections_for_nodes;

pub fn inspector_panel(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut EditorService,
    project: &Arc<RwLock<Project>>,
) {
    crate::qa::register_component(
        "inspector.scroll_area",
        "inspector_scroll_area",
        ui.available_rect_before_wrap(),
    );
    egui::ScrollArea::vertical()
        .id_salt("inspector_scroll")
        .show(ui, |ui| {
            inspector_panel_content(
                ui,
                editor_context,
                history_manager,
                project_service,
                project,
            );
        });
}

fn inspector_panel_content(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut EditorService,
    project: &Arc<RwLock<Project>>,
) {
    let Some(composition_id) = editor_context.active_composition_id else {
        ui.label("No composition selected.");
        return;
    };
    let selection = match project.read() {
        Ok(project) => {
            resolve_selection(&project, editor_context.selection.primary(), composition_id)
        }
        Err(error) => {
            log::error!("Failed to read Project for Inspector: {error}");
            ui.label("Project is temporarily unavailable.");
            return;
        }
    };

    let Some(selection) = selection else {
        ui.label("The selected Timeline item was not found (it may have been deleted).");
        editor_context.clear_selection();
        return;
    };

    let (fps, resolution) = project_service
        .get_composition(composition_id)
        .map(|composition| (composition.fps, (composition.width, composition.height)))
        .unwrap_or((60.0, (1920, 1080)));
    let global_time = editor_context.timeline.current_time as f64;
    let mut needs_refresh = false;

    render_multi_selection_notice(ui, editor_context);

    match selection {
        InspectorSelection::Composition {
            composition,
            nodes,
            connections,
            semantics,
        } => {
            let heading = ui.heading(format!("Composition: {}", composition.name));
            crate::qa::register_component_with_metadata(
                format!("inspector.owner.composition:{}", composition.id),
                "inspector_owner",
                heading.rect,
                true,
                Some(serde_json::json!({"owner": "composition", "id": composition.id})),
            );
            ui.separator();
            render_semantic_graph_facade(
                ui,
                "Composition Output",
                FacadeOwnerKind::Composition,
                &semantics,
                &nodes,
                &connections,
                composition_id,
                None,
                global_time,
                fps,
                resolution,
                project_service,
                history_manager,
                editor_context,
                &mut needs_refresh,
            );
        }
        InspectorSelection::Track {
            track,
            nodes,
            connections,
            semantics,
        } => {
            let heading = ui.heading(format!("Track: {}", track.name));
            crate::qa::register_component_with_metadata(
                format!("inspector.owner.track:{}", track.id),
                "inspector_owner",
                heading.rect,
                true,
                Some(serde_json::json!({"owner": "track", "id": track.id})),
            );
            ui.separator();
            render_semantic_graph_facade(
                ui,
                "Track Output",
                FacadeOwnerKind::Track,
                &semantics,
                &nodes,
                &connections,
                composition_id,
                Some(track.id),
                global_time,
                fps,
                resolution,
                project_service,
                history_manager,
                editor_context,
                &mut needs_refresh,
            );
        }
        InspectorSelection::Clip { clip, track_id } => {
            let heading = ui.heading(format!("Clip: {}", clip.name));
            crate::qa::register_component_with_metadata(
                format!("inspector.owner.clip:{}", clip.id),
                "inspector_owner",
                heading.rect,
                true,
                Some(serde_json::json!({
                    "owner": "clip",
                    "id": clip.id,
                    "track_id": track_id,
                })),
            );
            ui.separator();
            let local_time = clip.local_time(global_time);
            semantic_clip::render(
                ui,
                &clip,
                local_time,
                fps,
                resolution,
                project_service,
                history_manager,
                editor_context,
                project,
                &mut needs_refresh,
            );
        }
        InspectorSelection::Node {
            node,
            track_id,
            containing_clip,
            time_source,
        } => {
            let heading = ui.heading(format!("Node: {}", node.name));
            crate::qa::register_component_with_metadata(
                format!("inspector.owner.node:{}", node.id),
                "inspector_owner",
                heading.rect,
                true,
                Some(serde_json::json!({
                    "owner": "node",
                    "id": node.id,
                    "node_type": node_display_type(&node),
                })),
            );
            ui.separator();
            if let Some(time_source) = time_source.as_ref() {
                render_node_time_source(ui, node.id, time_source);
            }
            let evaluation_time = containing_clip
                .as_ref()
                .map_or(global_time, |clip| clip.local_time(global_time));
            render_node(
                ui,
                &node,
                composition_id,
                track_id,
                evaluation_time,
                fps,
                resolution,
                project_service,
                history_manager,
                editor_context,
                &mut needs_refresh,
            );
        }
    }

    if needs_refresh {
        ui.ctx().request_repaint();
    }
}

#[cfg(test)]
mod tests;
