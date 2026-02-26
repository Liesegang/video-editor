use egui::Ui;
use library::project::node::Node;
use library::project::project::Project;
use library::project::source::SourceData;
use library::EditorService as ProjectService;
use std::sync::{Arc, RwLock};

use crate::widgets::context_menu::{show_context_menu, ContextMenuBuilder};
use crate::{command::history::HistoryManager, context::context::EditorContext};

use super::super::geometry::TimelineGeometry;

#[derive(Clone)]
enum LayerAreaAction {
    AddTextLayer,
    AddShapeLayer,
    AddSkSLLayer,
}

pub(super) fn handle_context_menu(
    ui: &mut Ui,
    response: &egui::Response,
    content_rect: egui::Rect,
    editor_context: &mut EditorContext,
    project: &Arc<RwLock<Project>>,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    geo: &TimelineGeometry,
    num_tracks: usize,
) {
    let pixels_per_unit = geo.pixels_per_unit;
    let composition_fps = geo.composition_fps;
    let row_height = geo.row_height;
    let track_spacing = geo.track_spacing;
    // Capture right-click position BEFORE the context menu opens/draws
    if response.hovered() && ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary))
    {
        if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
            editor_context.interaction.timeline.context_menu_open_pos = Some(pos);
        }
    }

    // Context Menu for adding Text/Shape/SkSL
    response.context_menu(|ui| {
        let mut drop_in_frame =
            (editor_context.timeline.current_time * composition_fps as f32).round() as u64;
        let mut drop_track_index_opt = None;

        let mut comp_width = 1920;
        let mut comp_height = 1080;
        if let Some(comp_id) = editor_context.selection.composition_id {
            if let Ok(proj_read) = project.read() {
                if let Some(comp) = proj_read.get_composition(comp_id) {
                    comp_width = comp.width;
                    comp_height = comp.height;
                }
            }
        }

        // Try to recover clicked position
        if let Some(pos) = editor_context.interaction.timeline.context_menu_open_pos {
            let (frame, row_idx) = super::super::utils::pos_to_timeline_location(
                pos,
                content_rect,
                editor_context.timeline.scroll_offset,
                pixels_per_unit,
                composition_fps,
                row_height,
                track_spacing,
            );
            drop_in_frame = frame;
            if row_idx < num_tracks {
                drop_track_index_opt = Some(row_idx);
            }
        }

        let menu = ContextMenuBuilder::new()
            .action("Add Text Layer", LayerAreaAction::AddTextLayer)
            .action("Add Shape Layer", LayerAreaAction::AddShapeLayer)
            .action("Add SkSL Layer", LayerAreaAction::AddSkSLLayer)
            .build();

        if let Some(action) = show_context_menu(ui, &menu) {
            let duration_sec: f64 = 5.0;
            let duration_frames = (duration_sec * composition_fps as f64).round() as u64;
            let drop_out_frame = drop_in_frame + duration_frames;

            let source_result = match action {
                LayerAreaAction::AddTextLayer => project_service.build_text_source(
                    "this is sample text",
                    drop_in_frame,
                    drop_out_frame,
                    comp_width as u32,
                    comp_height as u32,
                    composition_fps,
                ),
                LayerAreaAction::AddShapeLayer => project_service.build_shape_source(
                    drop_in_frame,
                    drop_out_frame,
                    comp_width as u32,
                    comp_height as u32,
                    composition_fps,
                ),
                LayerAreaAction::AddSkSLLayer => project_service.build_sksl_source(
                    drop_in_frame,
                    drop_out_frame,
                    comp_width as u32,
                    comp_height as u32,
                    composition_fps,
                ),
            };

            if let Ok(source) = source_result {
                add_source_to_best_track(
                    project,
                    editor_context,
                    drop_track_index_opt,
                    source,
                    drop_in_frame,
                    drop_out_frame,
                    project_service,
                    history_manager,
                );
            }
        }
    });
}

fn add_source_to_best_track(
    project: &Arc<RwLock<Project>>,
    editor_context: &mut EditorContext,
    drop_track_index_opt: Option<usize>,
    source: SourceData,
    in_frame: u64,
    out_frame: u64,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
) {
    let mut track_id_opt = None;
    if let Ok(proj_read) = project.read() {
        if let Some(comp_id) = editor_context.selection.composition_id {
            if let Some(comp) = proj_read.get_composition(comp_id) {
                let root_ids: Vec<uuid::Uuid> = comp.child_ids.clone();

                // If we have a calculated track index, use flattened display to find the track
                if let Some(idx) = drop_track_index_opt {
                    let display_rows = super::super::utils::flatten::flatten_tracks_to_rows(
                        &proj_read,
                        &root_ids,
                        &editor_context.timeline.expanded_tracks,
                    );
                    if let Some(row) = display_rows.get(idx) {
                        track_id_opt = Some(row.track_id());
                    }
                }

                // Fallback: find first child track in composition
                if track_id_opt.is_none() {
                    for child_id in &comp.child_ids {
                        if let Some(Node::Track(_)) = proj_read.get_node(*child_id) {
                            track_id_opt = Some(*child_id);
                            break;
                        }
                    }
                }
            }
        }
    }

    if let Some(comp_id) = editor_context.selection.composition_id {
        // If no child track was found, create one
        if track_id_opt.is_none() {
            match project_service.add_track(comp_id, "New Track") {
                Ok(new_track_id) => {
                    track_id_opt = Some(new_track_id);
                    editor_context.timeline.expanded_tracks.insert(new_track_id);
                }
                Err(e) => {
                    log::error!("Failed to create track for layer: {}", e);
                }
            }
        }

        if let Some(track_id) = track_id_opt {
            if let Err(e) = project_service
                .add_layer_to_track(comp_id, track_id, source, in_frame, out_frame, None)
            {
                log::error!("Failed to add layer: {}", e);
            } else {
                let current_state = project_service.with_project(|p| p.clone());
                history_manager.push_project_state(current_state);
            }
        }
    }
}
