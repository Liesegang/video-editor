use egui::Ui;
use egui_phosphor::regular as icons;
use library::model::project::project::Project;
use library::EditorService as ProjectService;
use log::error;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::{action::HistoryManager, state::context::EditorContext};

use super::geometry::TimelineGeometry;

/// Deferred actions to execute after read lock is released
#[derive(Debug)]
enum DeferredTrackAction {
    AddTrack {
        comp_id: Uuid,
    },
    AddSubTrack {
        comp_id: Uuid,
        parent_track_id: Uuid,
    },
    RemoveTrack {
        comp_id: Uuid,
        track_id: Uuid,
    },
    RenameTrack {
        track_id: Uuid,
        new_name: String,
    },
}

pub(super) fn show_track_list(
    ui_content: &mut Ui,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut ProjectService,
    project: &Arc<RwLock<Project>>,
    sidebar_width: f32,
) -> (usize, f32, f32) {
    let row_height = 30.0;
    let track_spacing = 2.0;
    let mut deferred_actions: Vec<DeferredTrackAction> = Vec::new();
    let mut tracks_to_expand: Vec<Uuid> = Vec::new();
    let mut tracks_to_deselect: Vec<Uuid> = Vec::new();

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

    let mut root_track_ids: Vec<uuid::Uuid> = Vec::new();
    let mut asset_names: HashMap<uuid::Uuid, String> = HashMap::new();
    let selected_composition_id = editor_context.selection.composition_id;

    let proj_read = project.read().ok();

    if let Some(comp_id) = selected_composition_id {
        if let Some(ref proj) = proj_read {
            if let Some(comp) = proj.compositions.iter().find(|c| c.id == comp_id) {
                root_track_ids.push(comp.root_track_id);
            }
            // Cache asset names for quick lookup
            for asset in &proj.assets {
                asset_names.insert(asset.id, asset.name.clone());
            }
        }
    }

    // Flatten tracks based on expanded state using the new row-based flattener
    let display_rows = if let Some(ref proj) = proj_read {
        super::utils::flatten::flatten_tracks_to_rows(
            proj,
            &root_track_ids,
            &editor_context.timeline.expanded_tracks,
        )
    } else {
        Vec::new()
    };
    let num_rows = display_rows.len();

    // Iterate over visible rows
    // Calculate Reorder State for Preview
    let mut reorder_state = None;
    if let (Some(dragged_id), Some(hovered_tid)) = (
        editor_context.selection.last_selected_entity_id,
        editor_context
            .interaction
            .timeline
            .dragged_entity_hovered_track_id,
    ) {
        if let Some(mouse_pos) = ui_content.ctx().pointer_latest_pos() {
            if let Some(ref proj) = proj_read {
                let insert_geo = TimelineGeometry {
                    pixels_per_unit: 0.0,
                    row_height,
                    track_spacing,
                    composition_fps: 0.0,
                };
                if let Some((target_index, header_idx)) =
                    super::clip_area::clips::calculate_insert_index(
                        mouse_pos.y,
                        track_list_rect.min.y,
                        editor_context.timeline.scroll_offset.y,
                        &insert_geo,
                        &display_rows,
                        proj,
                        &root_track_ids,
                        hovered_tid,
                    )
                {
                    let mut dragged_original_index = 0;
                    if let Some(track) = proj.get_track(hovered_tid) {
                        if let Some(pos) = track.child_ids.iter().position(|id| *id == dragged_id) {
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
                        if let Some(original_track_id) = editor_context
                            .interaction
                            .timeline
                            .dragged_entity_original_track_id
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
                visible_row_index: _, // Ignored here as we use row.visible_row_index() method
                ..
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
                            deferred_actions.push(DeferredTrackAction::AddSubTrack {
                                comp_id,
                                parent_track_id: track.id,
                            });
                            tracks_to_expand.push(track.id);
                            ui.close();
                        }

                        ui.separator();

                        // Rename Track option
                        if ui
                            .button(format!("{} Rename", icons::PENCIL_SIMPLE))
                            .clicked()
                        {
                            editor_context.interaction.timeline.renaming_track_id = Some(track.id);
                            editor_context.interaction.timeline.rename_buffer = track.name.clone();
                            ui.close();
                        }

                        ui.separator();

                        if ui
                            .button(format!("{} Remove Track", icons::TRASH))
                            .clicked()
                        {
                            deferred_actions.push(DeferredTrackAction::RemoveTrack {
                                comp_id,
                                track_id: track.id,
                            });
                            // Mark for deselection if this track was selected
                            if editor_context.selection.last_selected_track_id == Some(track.id) {
                                tracks_to_deselect.push(track.id);
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

                // Check if this track is being renamed
                if editor_context.interaction.timeline.renaming_track_id == Some(track.id) {
                    // Draw inline TextEdit for renaming
                    let text_rect = egui::Rect::from_min_size(
                        row_rect.left_center() + egui::vec2(text_offset_x, -10.0),
                        egui::vec2(row_rect.width() - text_offset_x - 10.0, 20.0),
                    );
                    let text_edit = egui::TextEdit::singleline(
                        &mut editor_context.interaction.timeline.rename_buffer,
                    )
                    .font(egui::FontId::monospace(10.0))
                    .desired_width(text_rect.width());

                    let response = ui_content.put(text_rect, text_edit);

                    // Focus the text edit automatically
                    if !response.has_focus()
                        && editor_context.interaction.timeline.renaming_track_id == Some(track.id)
                    {
                        response.request_focus();
                    }

                    // Confirm on Enter or lose focus
                    let committed = response.lost_focus()
                        || (response.has_focus()
                            && ui_content.input(|i| i.key_pressed(egui::Key::Enter)));

                    if committed {
                        // Commit the rename
                        let new_name = editor_context.interaction.timeline.rename_buffer.clone();
                        // Only update if name changed and is not empty
                        if !new_name.is_empty() && new_name != track.name {
                            deferred_actions.push(DeferredTrackAction::RenameTrack {
                                track_id: track.id,
                                new_name,
                            });
                        }

                        // Clear rename state
                        editor_context.interaction.timeline.renaming_track_id = None;
                        editor_context.interaction.timeline.rename_buffer.clear();
                    }
                } else {
                    // Normal track name display
                    track_list_painter.text(
                        row_rect.left_center() + egui::vec2(text_offset_x, 0.0),
                        egui::Align2::LEFT_CENTER,
                        format!("Track {}", track.name),
                        egui::FontId::monospace(10.0),
                        egui::Color32::GRAY,
                    );
                }
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
                deferred_actions.push(DeferredTrackAction::AddTrack { comp_id });
                ui_content.close();
            }
        } else {
            ui_content.label("Select a Composition first");
        }
    });

    // Drop read lock before executing deferred actions
    drop(proj_read);

    // Execute deferred actions (no read lock held)
    let mut needs_history_push = false;
    for action in deferred_actions {
        match action {
            DeferredTrackAction::AddTrack { comp_id } => {
                if let Err(e) = project_service.add_track(comp_id, "New Track") {
                    error!("Failed to add track: {:?}", e);
                } else {
                    needs_history_push = true;
                }
            }
            DeferredTrackAction::AddSubTrack {
                comp_id,
                parent_track_id,
            } => {
                if let Err(e) =
                    project_service.add_sub_track(comp_id, parent_track_id, "New Sub-Track")
                {
                    error!("Failed to add sub-track: {:?}", e);
                } else {
                    needs_history_push = true;
                }
            }
            DeferredTrackAction::RemoveTrack { comp_id, track_id } => {
                if let Err(e) = project_service.remove_track(comp_id, track_id) {
                    error!("Failed to remove track: {:?}", e);
                } else {
                    needs_history_push = true;
                }
            }
            DeferredTrackAction::RenameTrack { track_id, new_name } => {
                if let Err(e) = project_service.rename_track(track_id, &new_name) {
                    error!("Failed to rename track: {:?}", e);
                } else {
                    needs_history_push = true;
                }
            }
        }
    }

    // Apply deferred state changes
    for track_id in tracks_to_expand {
        editor_context.timeline.expanded_tracks.insert(track_id);
    }
    for track_id in tracks_to_deselect {
        if editor_context.selection.last_selected_track_id == Some(track_id) {
            editor_context.selection.last_selected_track_id = None;
            editor_context.selection.last_selected_entity_id = None;
            editor_context.selection.selected_entities.clear();
        }
    }

    if needs_history_push {
        if let Ok(proj) = project.read() {
            history_manager.push_project_state(proj.clone());
        }
    }

    (num_rows, row_height, track_spacing)
}
