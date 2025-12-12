use egui::{epaint::StrokeKind, Ui};
use egui_phosphor::regular as icons;
use library::model::project::project::Project;
use library::model::project::TrackClip;
use library::model::project::TrackClipKind;
use library::service::project_service::ProjectService;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::{action::HistoryManager, model::ui_types::TimelineClip, state::context::EditorContext};

const EDGE_DRAG_WIDTH: f32 = 5.0;

#[allow(clippy::too_many_arguments)]
pub fn draw_clips(
    ui_content: &mut Ui,
    content_rect_for_clip_area: egui::Rect,
    editor_context: &mut EditorContext,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    current_tracks: &[library::model::project::Track],
    all_clips: &[(Uuid, TrackClip)],
    project: &Arc<RwLock<Project>>,
    pixels_per_unit: f32,
    row_height: f32,
    track_spacing: f32,
    composition_fps: f64,
) -> bool {
    let mut clicked_on_entity = false;

    for track_in_all_entities in current_tracks {
        let clip_track_index = current_tracks
            .iter()
            .position(|t| t.id == track_in_all_entities.id)
            .map(|idx| idx as f32)
            .unwrap_or(0.0);

        for (entity_track_id, entity) in all_clips
            .iter()
            .filter(|(t_id, _)| *t_id == track_in_all_entities.id)
        {
            // Determine Color based on kind
            let clip_color = match entity.kind {
                TrackClipKind::Video => egui::Color32::from_rgb(100, 150, 255), // Blue
                TrackClipKind::Audio => egui::Color32::from_rgb(100, 255, 150), // Green
                TrackClipKind::Image => egui::Color32::from_rgb(255, 100, 150), // Pink
                TrackClipKind::Composition => egui::Color32::from_rgb(255, 150, 255), // Magenta
                TrackClipKind::Text => egui::Color32::from_rgb(255, 200, 100),  // Orange/Yellow
                _ => egui::Color32::GRAY,
            };

            let gc = TimelineClip {
                id: entity.id,
                name: entity.kind.to_string(), // Use Display impl
                track_id: *entity_track_id,
                in_frame: entity.in_frame,   // u64
                out_frame: entity.out_frame, // u64
                timeline_duration_frames: entity.out_frame.saturating_sub(entity.in_frame), // u64
                source_begin_frame: entity.source_begin_frame, // u64
                duration_frame: entity.duration_frame, // Option<u64>
                color: clip_color,
                position: [
                    entity.properties.get_f32("position_x").unwrap_or(960.0),
                    entity.properties.get_f32("position_y").unwrap_or(540.0),
                ],
                scale_x: entity.properties.get_f32("scale_x").unwrap_or(100.0),
                scale_y: entity.properties.get_f32("scale_y").unwrap_or(100.0),
                anchor_x: entity.properties.get_f32("anchor_x").unwrap_or(0.0),
                anchor_y: entity.properties.get_f32("anchor_y").unwrap_or(0.0),
                opacity: entity.properties.get_f32("opacity").unwrap_or(100.0),
                rotation: entity.properties.get_f32("rotation").unwrap_or(0.0),
                asset_id: None, // We don't have asset_id stored on clip yet
                width: None,
                height: None,
            };

            let initial_x = content_rect_for_clip_area.min.x
                + (gc.in_frame as f32 / composition_fps as f32) * pixels_per_unit
                - editor_context.timeline.scroll_offset.x;
            let initial_y = content_rect_for_clip_area.min.y
                + editor_context.timeline.scroll_offset.y
                + clip_track_index * (row_height + track_spacing);

            let width =
                (gc.timeline_duration_frames as f32 / composition_fps as f32) * pixels_per_unit;
            // Prevent negative/zero width rects which might panic or cause issues
            let safe_width = width.max(1.0);

            let initial_clip_rect = egui::Rect::from_min_size(
                egui::pos2(initial_x, initial_y),
                egui::vec2(safe_width, row_height),
            );

            // --- Interaction for clips ---
            // Define clip_resp using the initial_clip_rect for hit detection
            let clip_resp = ui_content.interact(
                initial_clip_rect,
                egui::Id::new(gc.id),
                egui::Sense::click_and_drag(),
            );

            clip_resp.context_menu(|ui| {
                if ui.button(format!("{} Remove", icons::TRASH)).clicked() {
                    if let Some(comp_id) = editor_context.selection.composition_id {
                        if let Err(e) =
                            project_service.remove_clip_from_track(comp_id, gc.track_id, gc.id)
                        {
                            log::error!("Failed to remove entity: {:?}", e);
                        } else {
                            editor_context.selection.entity_id = None;
                            let current_state =
                                project_service.get_project().read().unwrap().clone();
                            history_manager.push_project_state(current_state);
                            ui.ctx().request_repaint();
                            ui.close();
                        }
                    }
                }
            });

            // Create edge responses
            let left_edge_rect = egui::Rect::from_min_size(
                egui::pos2(initial_clip_rect.min.x, initial_clip_rect.min.y),
                egui::vec2(EDGE_DRAG_WIDTH, initial_clip_rect.height()),
            );
            let left_edge_resp = ui_content.interact(
                left_edge_rect,
                egui::Id::new(gc.id).with("left_edge"),
                egui::Sense::drag(),
            );

            let right_edge_rect = egui::Rect::from_min_size(
                egui::pos2(
                    initial_clip_rect.max.x - EDGE_DRAG_WIDTH,
                    initial_clip_rect.min.y,
                ),
                egui::vec2(EDGE_DRAG_WIDTH, initial_clip_rect.height()),
            );
            let right_edge_resp = ui_content.interact(
                right_edge_rect,
                egui::Id::new(gc.id).with("right_edge"),
                egui::Sense::drag(),
            );

            // Handle edge dragging (resize)
            if left_edge_resp.drag_started() || right_edge_resp.drag_started() {
                editor_context.interaction.is_resizing_entity = true;
                editor_context.selection.entity_id = Some(gc.id);
                editor_context.selection.track_id = Some(gc.track_id);
            }

            if editor_context.interaction.is_resizing_entity
                && editor_context.selection.entity_id == Some(gc.id)
            {
                let mut new_in_frame = gc.in_frame;
                let mut new_out_frame = gc.out_frame;

                // Source constraints
                let source_max_out_frame = if let Some(duration) = gc.duration_frame {
                    gc.source_begin_frame.saturating_add(duration)
                } else {
                    u64::MAX
                };

                let delta_x = if left_edge_resp.dragged() {
                    left_edge_resp.drag_delta().x
                } else if right_edge_resp.dragged() {
                    right_edge_resp.drag_delta().x
                } else {
                    0.0
                };

                let dt_frames_f32 = delta_x / pixels_per_unit * composition_fps as f32;
                let dt_frames = dt_frames_f32.round() as i64;

                if left_edge_resp.dragged() {
                    new_in_frame = ((new_in_frame as i64 + dt_frames).max(0) as u64)
                        .max(gc.source_begin_frame) // Cannot go before source begin frame
                        .min(new_out_frame.saturating_sub(1)); // Minimum 1 frame duration
                } else if right_edge_resp.dragged() {
                    new_out_frame =
                        ((new_out_frame as i64 + dt_frames).max(new_in_frame as i64 + 1) as u64) // Minimum 1 frame duration
                            .min(source_max_out_frame); // Cannot go beyond source duration
                }

                // Update if there's an actual change
                if new_in_frame != gc.in_frame || new_out_frame != gc.out_frame {
                    if let (Some(comp_id), Some(track_id)) = (
                        editor_context.selection.composition_id,
                        editor_context.selection.track_id,
                    ) {
                        project_service
                            .update_clip_time(comp_id, track_id, gc.id, new_in_frame, new_out_frame)
                            .ok();
                    }
                }
            }

            if left_edge_resp.drag_stopped() || right_edge_resp.drag_stopped() {
                editor_context.interaction.is_resizing_entity = false;
                let current_state = project_service.get_project().read().unwrap().clone();
                history_manager.push_project_state(current_state);
            }

            // Calculate display position (potentially adjusted for drag preview)
            let mut display_x = initial_x;
            let mut display_y = initial_y;

            // Adjust position for dragged entity preview
            if editor_context.selection.entity_id == Some(gc.id) && clip_resp.dragged() {
                display_x += clip_resp.drag_delta().x;

                if let Some(hovered_track_id) =
                    editor_context.interaction.dragged_entity_hovered_track_id
                {
                    if let Some(hovered_track_index) =
                        current_tracks.iter().position(|t| t.id == hovered_track_id)
                    {
                        display_y = content_rect_for_clip_area.min.y
                            + editor_context.timeline.scroll_offset.y
                            + hovered_track_index as f32 * (row_height + track_spacing);
                    }
                }
            }

            let drawing_clip_rect = egui::Rect::from_min_size(
                egui::pos2(display_x, display_y),
                egui::vec2(safe_width, row_height),
            );

            // --- Drawing for clips (always) ---
            let is_sel_entity = editor_context.selection.entity_id == Some(gc.id);
            let color = gc.color;
            let transparent_color =
                egui::Color32::from_rgba_premultiplied(color.r(), color.g(), color.b(), 150);

            let painter = ui_content.painter_at(content_rect_for_clip_area);
            painter.rect_filled(drawing_clip_rect, 4.0, transparent_color);
            if is_sel_entity {
                painter.rect_stroke(
                    drawing_clip_rect,
                    4.0,
                    egui::Stroke::new(2.0, egui::Color32::WHITE),
                    StrokeKind::Middle,
                );
            }
            painter.text(
                drawing_clip_rect.center(),
                egui::Align2::CENTER_CENTER,
                &gc.name,
                egui::FontId::default(),
                egui::Color32::BLACK,
            );
            // --- End Drawing for clips ---

            // Cursor feedback
            if left_edge_resp.hovered() || right_edge_resp.hovered() {
                ui_content
                    .ctx()
                    .set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if !editor_context.interaction.is_resizing_entity && clip_resp.clicked() {
                editor_context.selection.entity_id = Some(gc.id);
                editor_context.selection.track_id = Some(gc.track_id);
                clicked_on_entity = true;
            }

            if !editor_context.interaction.is_resizing_entity && clip_resp.drag_started() {
                editor_context.selection.entity_id = Some(gc.id);
                editor_context.selection.track_id = Some(gc.track_id);
                editor_context.interaction.dragged_entity_original_track_id = Some(gc.track_id);
                editor_context.interaction.dragged_entity_hovered_track_id = Some(gc.track_id);
                editor_context.interaction.dragged_entity_has_moved = false;
            }
            if !editor_context.interaction.is_resizing_entity
                && clip_resp.dragged()
                && editor_context.selection.entity_id == Some(gc.id)
            {
                if clip_resp.drag_delta().length_sq() > 0.0 {
                    editor_context.interaction.dragged_entity_has_moved = true;
                }

                let dt_frames_f32 =
                    clip_resp.drag_delta().x / pixels_per_unit * composition_fps as f32;
                let dt_frames = dt_frames_f32.round() as i64;

                if let Some(comp_id) = editor_context.selection.composition_id {
                    if let Some(track_id) = editor_context.selection.track_id {
                        let new_in_frame = (gc.in_frame as i64 + dt_frames).max(0) as u64;
                        let new_out_frame =
                            (gc.out_frame as i64 + dt_frames).max(new_in_frame as i64) as u64;

                        project_service
                            .update_clip_time(comp_id, track_id, gc.id, new_in_frame, new_out_frame)
                            .ok();

                        let new_source_begin_frame =
                            (gc.source_begin_frame as i64 + dt_frames).max(0) as u64;
                        project_service
                            .update_clip_source_frames(
                                comp_id,
                                track_id,
                                gc.id,
                                new_source_begin_frame,
                                gc.duration_frame,
                            )
                            .ok();
                    }
                }

                // Handle vertical movement (track change detection)
                if let Some(mouse_pos) = ui_content.ctx().pointer_latest_pos() {
                    let current_y_in_clip_area = mouse_pos.y
                        - content_rect_for_clip_area.min.y
                        - editor_context.timeline.scroll_offset.y;

                    let hovered_track_index =
                        (current_y_in_clip_area / (row_height + track_spacing)).floor() as usize;

                    if let Some(comp_id) = editor_context.selection.composition_id {
                        if let Ok(proj_read) = project.read() {
                            if let Some(comp) =
                                proj_read.compositions.iter().find(|c| c.id == comp_id)
                            {
                                if let Some(hovered_track) = comp.tracks.get(hovered_track_index) {
                                    if editor_context.interaction.dragged_entity_hovered_track_id
                                        != Some(hovered_track.id)
                                    {
                                        editor_context
                                            .interaction
                                            .dragged_entity_hovered_track_id =
                                            Some(hovered_track.id);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if !editor_context.interaction.is_resizing_entity
                && clip_resp.drag_stopped()
                && editor_context.selection.entity_id == Some(gc.id)
            {
                let mut moved_track = false;
                if let (Some(original_track_id), Some(hovered_track_id), Some(comp_id)) = (
                    editor_context.interaction.dragged_entity_original_track_id,
                    editor_context.interaction.dragged_entity_hovered_track_id,
                    editor_context.selection.composition_id,
                ) {
                    if original_track_id != hovered_track_id {
                        // Move entity to new track
                        if let Err(e) = project_service.move_clip_to_track(
                            comp_id,
                            original_track_id,
                            hovered_track_id,
                            gc.id,
                        ) {
                            log::error!("Failed to move entity to new track: {:?}", e);
                        } else {
                            editor_context.selection.track_id = Some(hovered_track_id);
                            moved_track = true;
                        }
                    }
                }

                if moved_track || editor_context.interaction.dragged_entity_has_moved {
                    let current_state = project_service.get_project().read().unwrap().clone();
                    history_manager.push_project_state(current_state);
                }

                editor_context.interaction.dragged_entity_original_track_id = None;
                editor_context.interaction.dragged_entity_hovered_track_id = None;
            }
        }
    }

    clicked_on_entity
}
