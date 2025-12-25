use egui::Ui;
use egui_phosphor::regular as icons;
use library::model::project::project::Project;
use library::model::project::Track;
use library::EditorService as ProjectService;
use log::error;
use std::sync::{Arc, RwLock};

use crate::{action::HistoryManager, state::context::EditorContext};

pub fn show_track_list(
    ui_content: &mut Ui,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut ProjectService,
    project: &Arc<RwLock<Project>>,
    sidebar_width: f32,
) -> (usize, f32, f32) {
    let row_height = 30.0;
    let track_spacing = 2.0;

    let (track_list_rect, track_list_response) = ui_content.allocate_exact_size(
        egui::vec2(sidebar_width, ui_content.available_height()),
        egui::Sense::click_and_drag(),
    );
    let track_list_painter = ui_content.painter_at(track_list_rect);
    track_list_painter.rect_filled(
        track_list_rect,
        0.0,
        ui_content.style().visuals.window_fill(),
    );

    use std::collections::HashMap;

    let mut current_tracks: Vec<Track> = Vec::new();
    let mut asset_names: HashMap<uuid::Uuid, String> = HashMap::new();
    let selected_composition_id = editor_context.selection.composition_id;

    if let Some(comp_id) = selected_composition_id {
        if let Ok(proj_read) = project.read() {
            if let Some(comp) = proj_read.compositions.iter().find(|c| c.id == comp_id) {
                current_tracks = comp.tracks.clone();
            }
            // Cache asset names for quick lookup
            for asset in &proj_read.assets {
                asset_names.insert(asset.id, asset.name.clone());
            }
        }
    }

    // Flatten tracks based on expanded state using the new row-based flattener
    let display_rows = super::utils::flatten::flatten_tracks_to_rows(
        &current_tracks,
        &editor_context.timeline.expanded_tracks,
    );
    let num_rows = display_rows.len();

    // Iterate over visible rows
    // Calculate Reorder State for Preview
    let mut reorder_state = None;
    if let (Some(dragged_id), Some(hovered_tid)) = (
        editor_context.selection.last_selected_entity_id,
        editor_context.interaction.dragged_entity_hovered_track_id,
    ) {
        if let Some(mouse_pos) = ui_content.ctx().pointer_latest_pos() {
            // We use track_list_rect for Y reference, assuming alignment with clip area
            if let Some((target_index, header_idx)) =
                super::clip_area::clips::calculate_insert_index(
                    mouse_pos.y,
                    track_list_rect.min.y,
                    editor_context.timeline.scroll_offset.y,
                    row_height,
                    track_spacing,
                    &display_rows,
                    &current_tracks,
                    hovered_tid,
                )
            {
                let mut dragged_original_index = 0;
                if let Some(track) = current_tracks.iter().find(|t| t.id == hovered_tid) {
                    if let Some(pos) = track.clips().position(|c| c.id == dragged_id) {
                        dragged_original_index = pos;
                    }
                    reorder_state = Some((
                        dragged_id,
                        hovered_tid,
                        dragged_original_index,
                        target_index,
                        header_idx,
                    ));
                }
            }
        }
    }

    for row in &display_rows {
        let mut visible_row_index = row.visible_row_index() as isize;

        // Apply visual shift based on reorder state
        if let Some((dragged_id, hovered_track_id, original_idx, target_idx, header_idx)) =
            reorder_state
        {
            match row {
                super::utils::flatten::DisplayRow::ClipRow {
                    clip,
                    parent_track,
                    child_index,
                    ..
                } => {
                    if clip.id == dragged_id {
                        visible_row_index = (header_idx + 1 + target_idx) as isize;
                    } else if parent_track.id == hovered_track_id {
                        let idx = *child_index;
                        // Check if same track reordering
                        if let Some(original_track_id) =
                            editor_context.interaction.dragged_entity_original_track_id
                        {
                            if original_track_id == hovered_track_id {
                                // Same track sort
                                let src = original_idx;
                                let dst = target_idx;
                                if src < dst {
                                    // Moving down: Items between src and dst shift up
                                    if idx > src && idx <= dst {
                                        visible_row_index -= 1;
                                    }
                                } else {
                                    // Moving up: Items between dst and src shift down
                                    if idx < src && idx >= dst {
                                        visible_row_index += 1;
                                    }
                                }
                            } else {
                                // Cross track insert
                                if idx >= target_idx {
                                    visible_row_index += 1;
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let visible_row_index = visible_row_index as usize;

        let y = track_list_rect.min.y + (visible_row_index as f32 * (row_height + track_spacing))
            - editor_context.timeline.scroll_offset.y;

        let row_rect = egui::Rect::from_min_size(
            egui::pos2(track_list_rect.min.x, y),
            egui::vec2(track_list_rect.width(), row_height),
        );

        // Optimization: Skip rendering if out of view
        if !track_list_rect.intersects(row_rect) {
            continue;
        }

        match row {
            super::utils::flatten::DisplayRow::TrackHeader {
                track,
                depth,
                is_expanded,
                has_clips: _,
                has_sub_tracks: _,
                visible_row_index: _, // Ignored here as we use row.visible_row_index() method
            } => {
                let track_interaction_response = ui_content
                    .interact(
                        row_rect,
                        egui::Id::new(track.id).with("track_label_interact"),
                        egui::Sense::click(),
                    )
                    .on_hover_text(format!("Track ID: {}", track.id));

                track_interaction_response.context_menu(|ui| {
                    if let Some(comp_id) = editor_context.selection.composition_id {
                        // Add Sub-Track option
                        if ui
                            .button(format!("{} Add Sub-Track", icons::FOLDER_PLUS))
                            .clicked()
                        {
                            if let Err(e) =
                                project_service.add_sub_track(comp_id, track.id, "New Sub-Track")
                            {
                                error!("Failed to add sub-track: {:?}", e);
                            } else {
                                // Auto-expand the parent track to show the new child
                                editor_context.timeline.expanded_tracks.insert(track.id);
                                let current_state = project.read().unwrap().clone();
                                history_manager.push_project_state(current_state);
                            }
                            ui.close();
                        }

                        ui.separator();

                        if ui
                            .button(format!("{} Remove Track", icons::TRASH))
                            .clicked()
                        {
                            if let Err(e) = project_service.remove_track(comp_id, track.id) {
                                error!("Failed to remove track: {:?}", e);
                            } else {
                                // If the removed track was selected, deselect it
                                if editor_context.selection.last_selected_track_id == Some(track.id)
                                {
                                    editor_context.selection.last_selected_track_id = None;
                                    editor_context.selection.last_selected_entity_id = None;
                                    editor_context.selection.selected_entities.clear();
                                }
                                let current_state = project.read().unwrap().clone();
                                history_manager.push_project_state(current_state);
                            }
                            ui.close();
                        }
                    }
                });

                if track_interaction_response.clicked() {
                    editor_context.selection.last_selected_track_id = Some(track.id);
                }

                track_list_painter.rect_filled(
                    row_rect,
                    0.0,
                    if editor_context.selection.last_selected_track_id == Some(track.id) {
                        egui::Color32::from_rgb(50, 80, 120)
                    } else if visible_row_index % 2 == 0 {
                        egui::Color32::from_gray(50)
                    } else {
                        egui::Color32::from_gray(60)
                    },
                );

                // Indentation
                let indent = *depth as f32 * 10.0;
                let mut text_offset_x = 5.0 + indent;

                // Expand/Collapse Icon (Always show if it's a track, might want to check children count?
                // flatten.rs uses has_clips/has_sub_tracks but we can simpler just show for Tracks)
                // Actually `flatten.rs` determines `is_folder`... here we just check if it can expand.
                // Assuming all tracks *can* be folders effectively.

                let icon_rect = egui::Rect::from_min_size(
                    egui::pos2(row_rect.min.x + indent, row_rect.min.y),
                    egui::vec2(16.0, row_height),
                );

                let icon_response = ui_content.interact(
                    icon_rect,
                    egui::Id::new(track.id).with("expand_icon"),
                    egui::Sense::click(),
                );

                if icon_response.clicked() {
                    if *is_expanded {
                        editor_context.timeline.expanded_tracks.remove(&track.id);
                    } else {
                        editor_context.timeline.expanded_tracks.insert(track.id);
                    }
                }

                let icon = if *is_expanded {
                    icons::CARET_DOWN
                } else {
                    icons::CARET_RIGHT
                };

                track_list_painter.text(
                    icon_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    icon,
                    egui::FontId::monospace(12.0),
                    egui::Color32::WHITE,
                );
                text_offset_x += 16.0;

                track_list_painter.text(
                    row_rect.left_center() + egui::vec2(text_offset_x, 0.0),
                    egui::Align2::LEFT_CENTER,
                    format!("Track {}", track.name),
                    egui::FontId::monospace(10.0),
                    egui::Color32::GRAY,
                );
            }
            super::utils::flatten::DisplayRow::ClipRow {
                clip,
                parent_track: _,
                depth,
                visible_row_index: _,
                child_index: _,
            } => {
                // Render Clip Name
                track_list_painter.rect_filled(
                    row_rect,
                    0.0,
                    if visible_row_index % 2 == 0 {
                        egui::Color32::from_gray(45)
                    } else {
                        egui::Color32::from_gray(55)
                    },
                );

                let indent = *depth as f32 * 10.0;
                let text_offset_x = 5.0 + indent + 16.0; // Extra indent for clip (no folder icon)

                let clip_name = if let Some(asset_id) = clip.reference_id {
                    asset_names
                        .get(&asset_id)
                        .cloned()
                        .unwrap_or_else(|| "Unknown Asset".to_string())
                } else {
                    format!("{}", clip.kind)
                };

                track_list_painter.text(
                    row_rect.left_center() + egui::vec2(text_offset_x, 0.0),
                    egui::Align2::LEFT_CENTER,
                    clip_name,
                    egui::FontId::proportional(12.0),
                    egui::Color32::LIGHT_GRAY,
                );
            }
        }
    }

    track_list_response.context_menu(|ui_content| {
        if let Some(comp_id) = editor_context.selection.composition_id {
            if ui_content
                .add(egui::Button::new(egui::RichText::new(format!(
                    "{} Add Track",
                    icons::PLUS
                ))))
                .clicked()
            {
                project_service
                    .add_track(comp_id, "New Track")
                    .expect("Failed to add track");
                let current_state = project.read().unwrap().clone();
                history_manager.push_project_state(current_state);
                ui_content.close();
            }
        } else {
            ui_content.label("Select a Composition first");
        }
    });

    (num_rows, row_height, track_spacing)
}
