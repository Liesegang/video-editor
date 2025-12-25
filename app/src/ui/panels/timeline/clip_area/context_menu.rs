use egui::Ui;
use library::model::project::project::Project;
use library::model::project::{Node, TrackClip};
use library::EditorService as ProjectService;
use std::sync::{Arc, RwLock};

use crate::{action::HistoryManager, state::context::EditorContext};

pub fn handle_context_menu(
    ui: &mut Ui,
    response: &egui::Response,
    content_rect: egui::Rect,
    editor_context: &mut EditorContext,
    project: &Arc<RwLock<Project>>,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    pixels_per_unit: f32,
    composition_fps: f64,
    num_tracks: usize,
    row_height: f32,
    track_spacing: f32,
) {
    // Capture right-click position BEFORE the context menu opens/draws
    if response.hovered() && ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary))
    {
        if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
            editor_context.interaction.context_menu_open_pos = Some(pos);
        }
    }

    // Context Menu for adding Text/Shape
    response.context_menu(|ui| {
        let mut drop_in_frame =
            (editor_context.timeline.current_time * composition_fps as f32).round() as u64;
        let mut drop_track_index_opt = None;

        let mut comp_width = 1920;
        let mut comp_height = 1080;
        if let Some(comp_id) = editor_context.selection.composition_id {
            if let Ok(proj_read) = project.read() {
                if let Some(comp) = proj_read.compositions.iter().find(|c| c.id == comp_id) {
                    comp_width = comp.width;
                    comp_height = comp.height;
                }
            }
        }

        // Try to recover clicked position
        if let Some(pos) = editor_context.interaction.context_menu_open_pos {
            let local_x = pos.x - content_rect.min.x + editor_context.timeline.scroll_offset.x;
            let time_at_click = (local_x / pixels_per_unit).max(0.0);
            drop_in_frame = (time_at_click * composition_fps as f32).round() as u64;

            let local_y = pos.y - content_rect.min.y + editor_context.timeline.scroll_offset.y;
            let track_idx = (local_y / (row_height + track_spacing)).floor() as isize;
            if track_idx >= 0 && track_idx < num_tracks as isize {
                drop_track_index_opt = Some(track_idx as usize);
            }
        }

        if ui.button("Add Text Layer").clicked() {
            let duration_sec = 5.0;
            let duration_frames = (duration_sec * composition_fps).round() as u64;
            let drop_out_frame = drop_in_frame + duration_frames;

            let text_clip = TrackClip::create_text(
                "this is sample text",
                drop_in_frame,
                drop_out_frame,
                comp_width as u32,
                comp_height as u32,
                composition_fps,
            );

            add_clip_to_best_track(
                project,
                editor_context,
                drop_track_index_opt,
                text_clip,
                drop_in_frame,
                drop_out_frame,
                project_service,
                history_manager,
            );
            ui.close();
        }

        if ui.button("Add Shape Layer").clicked() {
            let duration_sec = 5.0;
            let duration_frames = (duration_sec * composition_fps).round() as u64;
            let drop_out_frame = drop_in_frame + duration_frames;

            let shape_clip = TrackClip::create_shape(
                drop_in_frame,
                drop_out_frame,
                comp_width as u32,
                comp_height as u32,
                composition_fps,
            );

            add_clip_to_best_track(
                project,
                editor_context,
                drop_track_index_opt,
                shape_clip,
                drop_in_frame,
                drop_out_frame,
                project_service,
                history_manager,
            );
            ui.close();
        }

        if ui.button("Add SkSL Layer").clicked() {
            let duration_sec = 5.0;
            let duration_frames = (duration_sec * composition_fps).round() as u64;
            let drop_out_frame = drop_in_frame + duration_frames;

            let sksl_clip = TrackClip::create_sksl(
                drop_in_frame,
                drop_out_frame,
                comp_width as u32,
                comp_height as u32,
                composition_fps,
            );

            add_clip_to_best_track(
                project,
                editor_context,
                drop_track_index_opt,
                sksl_clip,
                drop_in_frame,
                drop_out_frame,
                project_service,
                history_manager,
            );
            ui.close();
        }
    });
}

fn add_clip_to_best_track(
    project: &Arc<RwLock<Project>>,
    editor_context: &EditorContext,
    drop_track_index_opt: Option<usize>,
    clip: TrackClip,
    in_frame: u64,
    out_frame: u64,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
) {
    let mut track_id_opt = None;
    if let Ok(proj_read) = project.read() {
        if let Some(comp_id) = editor_context.selection.composition_id {
            if let Some(comp) = proj_read.compositions.iter().find(|c| c.id == comp_id) {
                // Get root track and find tracks by flattening
                let root_track_id = comp.root_track_id;

                // If we have a calculated track index, use flattened display to find the track
                if let Some(idx) = drop_track_index_opt {
                    let root_ids = vec![root_track_id];
                    let display_rows = super::super::utils::flatten::flatten_tracks_to_rows(
                        &proj_read,
                        &root_ids,
                        &editor_context.timeline.expanded_tracks,
                    );
                    if let Some(row) = display_rows.get(idx) {
                        track_id_opt = Some(row.track_id());
                    }
                }

                // Fallback to root track if not found
                if track_id_opt.is_none() {
                    // Use the root track itself or find first child track
                    if let Some(root_track) = proj_read.get_track(root_track_id) {
                        // If root track has child tracks, use the first one; otherwise use root
                        for child_id in &root_track.child_ids {
                            if let Some(Node::Track(_)) = proj_read.get_node(*child_id) {
                                track_id_opt = Some(*child_id);
                                break;
                            }
                        }
                        // If no child tracks, use root track itself
                        if track_id_opt.is_none() {
                            track_id_opt = Some(root_track_id);
                        }
                    }
                }
            }
        }
    }

    if let Some(track_id) = track_id_opt {
        if let Some(comp_id) = editor_context.selection.composition_id {
            if let Err(e) = project_service
                .add_clip_to_track(comp_id, track_id, clip, in_frame, out_frame, None)
            {
                log::error!("Failed to add clip: {}", e);
            } else {
                let current_state = project_service.get_project().read().unwrap().clone();
                history_manager.push_project_state(current_state);
            }
        }
    }
}
