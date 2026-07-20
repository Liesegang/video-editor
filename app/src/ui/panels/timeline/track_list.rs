use egui::Ui;
use egui_phosphor::regular as icons;
use library::model::project::{PortOwner, Project};
use library::model::{Clip, NodeContent};
use library::EditorService as ProjectService;
use log::error;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::{
    action::HistoryManager,
    state::{context::EditorContext, context_types::SelectionTarget},
};

/// Deferred actions to execute after read lock is released
#[derive(Debug)]
enum DeferredTrackAction {
    Add {
        comp_id: Uuid,
    },
    Remove {
        comp_id: Uuid,
        track_id: Uuid,
    },
    Rename {
        track_id: Uuid,
        new_name: String,
    },
    Move {
        comp_id: Uuid,
        track_id: Uuid,
        destination_index: usize,
    },
}

/// Returns insertion slots in Composition order and their screen-space Y
/// coordinates. Expanded clip rows remain part of their Track's visual group,
/// so a slot is placed only before a top-level Track or after the final group.
fn track_insertion_markers(
    display_rows: &[super::utils::flatten::DisplayRow<'_>],
    list_top: f32,
    scroll_y: f32,
    row_height: f32,
    track_spacing: f32,
) -> Vec<(usize, f32)> {
    let stride = row_height + track_spacing;
    let mut markers: Vec<(usize, f32)> = display_rows
        .iter()
        .filter_map(|row| match row {
            super::utils::flatten::DisplayRow::TrackHeader {
                depth,
                visible_row_index,
                ..
            } if *depth == 0 => Some(*visible_row_index),
            _ => None,
        })
        .enumerate()
        .map(|(slot, row_index)| {
            (
                slot,
                list_top + row_index as f32 * stride - scroll_y - track_spacing * 0.5,
            )
        })
        .collect();

    if markers.is_empty() {
        return markers;
    }

    let end_row = display_rows
        .last()
        .map(|row| row.visible_row_index() + 1)
        .unwrap_or(0);
    markers.push((
        markers.len(),
        list_top + end_row as f32 * stride - scroll_y - track_spacing * 0.5,
    ));
    markers
}

fn nearest_track_insertion_slot(pointer_y: f32, markers: &[(usize, f32)]) -> Option<(usize, f32)> {
    markers
        .iter()
        .copied()
        .min_by(|(_, first_y), (_, second_y)| {
            (pointer_y - *first_y)
                .abs()
                .total_cmp(&(pointer_y - *second_y).abs())
        })
}

/// Converts a slot in the original order into the Track's final index after
/// removing the source Track. The two slots adjacent to the source are both a
/// no-op, which makes dropping back in place stable.
fn destination_index_for_slot(
    source_index: usize,
    insertion_slot: usize,
    track_count: usize,
) -> Option<usize> {
    if track_count == 0 || source_index >= track_count || insertion_slot > track_count {
        return None;
    }

    Some(if insertion_slot > source_index {
        insertion_slot - 1
    } else {
        insertion_slot
    })
}

fn expanded_clip_label(
    project: &Project,
    clip: &Clip,
    asset_names: &HashMap<Uuid, String>,
) -> String {
    project
        .container_graph_semantics(PortOwner::Clip(clip.id))
        .authored_source_node_id()
        .and_then(|node_id| project.get_node(node_id))
        .map(|node| match node.content() {
            NodeContent::Media(media) => asset_names
                .get(&media.asset_id)
                .cloned()
                .unwrap_or_else(|| node.name.clone()),
            _ => node.name.clone(),
        })
        .unwrap_or_else(|| clip.name.clone())
}

fn track_for_selection(project: &Project, target: SelectionTarget) -> Option<Uuid> {
    match target {
        SelectionTarget::Track(track_id) => project.get_track(track_id).map(|_| track_id),
        SelectionTarget::Clip(clip_id) => project
            .get_clip(clip_id)
            .and_then(|_| project.find_track_for_clip(clip_id)),
        SelectionTarget::Node(node_id) => project.get_node(node_id).and_then(|_| {
            project
                .find_node_container(node_id)
                .and_then(|container| match container {
                    library::model::NodeContainer::Track(track_id) => Some(track_id),
                    library::model::NodeContainer::Clip(clip_id) => {
                        project.find_track_for_clip(clip_id)
                    }
                    library::model::NodeContainer::Composition(_) => None,
                })
        }),
        SelectionTarget::Composition(_) => None,
    }
}

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
    let mut deferred_actions: Vec<DeferredTrackAction> = Vec::new();

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

    let mut track_ids: Vec<uuid::Uuid> = Vec::new();
    let mut asset_names: HashMap<uuid::Uuid, String> = HashMap::new();
    let selected_composition_id = editor_context.active_composition_id;

    let proj_read = project.read().ok();

    if let Some(comp_id) = selected_composition_id {
        if let Some(ref proj) = proj_read {
            if let Some(comp) = proj.compositions.iter().find(|c| c.id == comp_id) {
                track_ids = comp.track_ids.clone();
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
            &track_ids,
            &editor_context.timeline.expanded_tracks,
        )
    } else {
        Vec::new()
    };
    let selected_track_id = proj_read.as_ref().and_then(|project| {
        editor_context
            .selection
            .primary()
            .and_then(|target| track_for_selection(project, target))
    });
    let num_rows = display_rows.len();

    // A composition switch or an externally removed Track cancels the
    // runtime gesture without mutating Project state.
    if editor_context
        .interaction
        .timeline_track_reorder
        .as_ref()
        .is_some_and(|reorder| {
            Some(reorder.composition_id) != selected_composition_id
                || !track_ids.contains(&reorder.track_id)
        })
    {
        editor_context.interaction.timeline_track_reorder = None;
    }

    // Iterate over visible rows
    // Calculate Reorder State for Preview
    let mut reorder_state = None;
    if let (Some(dragged_id), Some(hovered_tid)) = (
        editor_context
            .selection
            .primary()
            .and_then(SelectionTarget::clip_id),
        editor_context.interaction.dragged_entity_hovered_track_id,
    ) {
        if let Some(mouse_pos) = ui_content.ctx().pointer_latest_pos() {
            if let Some(ref proj) = proj_read {
                if let Some((target_index, header_idx)) =
                    super::clip_area::clips::calculate_insert_index(
                        mouse_pos.y,
                        &display_rows,
                        proj,
                        hovered_tid,
                        super::clip_area::clips::ClipRowLayout {
                            content_min_y: track_list_rect.min.y,
                            scroll_y: editor_context.timeline.scroll_offset.y,
                            row_height,
                            row_spacing: track_spacing,
                        },
                    )
                {
                    let source_track_id = editor_context
                        .interaction
                        .dragged_entity_original_track_id
                        .unwrap_or(hovered_tid);
                    if let Some(dragged_original_index) =
                        proj.get_track(source_track_id).and_then(|track| {
                            track
                                .clip_ids
                                .iter()
                                .position(|clip_id| *clip_id == dragged_id)
                        })
                    {
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
            if let super::utils::flatten::DisplayRow::ClipRow {
                clip,
                parent_track,
                child_index,
                ..
            } = row
            {
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
                        } else if idx >= target_idx {
                            // Cross track insert
                            visible_row_index += 1;
                        }
                    }
                }
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
                let canonical_index = track_ids
                    .iter()
                    .position(|candidate| *candidate == track.id);
                crate::qa::register_component_with_metadata(
                    format!("timeline.track:{}", track.id),
                    "timeline_track",
                    row_rect,
                    true,
                    Some(serde_json::json!({
                        "track_id": track.id,
                        "canonical_index": canonical_index,
                        "expanded": is_expanded,
                    })),
                );
                let track_interaction_response = ui_content
                    .interact(
                        row_rect,
                        egui::Id::new(track.id).with("track_label_interact"),
                        if editor_context.interaction.renaming_track_id == Some(track.id) {
                            egui::Sense::click()
                        } else {
                            egui::Sense::click_and_drag()
                        },
                    )
                    .on_hover_text(format!("Track ID: {}", track.id));

                track_interaction_response.context_menu(|ui| {
                    if let Some(comp_id) = editor_context.active_composition_id {
                        // Rename Track option
                        let rename = ui.button(format!("{} Rename", icons::PENCIL_SIMPLE));
                        crate::qa::register_component(
                            format!("timeline.menu.rename.track:{}", track.id),
                            "timeline_menu_item",
                            rename.rect,
                        );
                        if rename.clicked() {
                            editor_context.interaction.renaming_track_id = Some(track.id);
                            editor_context.interaction.rename_buffer = track.name.clone();
                            ui.close();
                        }

                        ui.separator();

                        let remove = ui.button(format!("{} Remove Track", icons::TRASH));
                        crate::qa::register_component(
                            format!("timeline.menu.delete.track:{}", track.id),
                            "timeline_menu_item",
                            remove.rect,
                        );
                        if remove.clicked() {
                            deferred_actions.push(DeferredTrackAction::Remove {
                                comp_id,
                                track_id: track.id,
                            });
                            ui.close();
                        }
                    }
                });

                if track_interaction_response.clicked_by(egui::PointerButton::Primary) {
                    editor_context.select_target(SelectionTarget::Track(track.id));
                }

                if track_interaction_response.drag_started_by(egui::PointerButton::Primary) {
                    if let (Some(comp_id), Some(source_index)) = (
                        selected_composition_id,
                        track_ids
                            .iter()
                            .position(|candidate| *candidate == track.id),
                    ) {
                        editor_context.interaction.timeline_track_reorder =
                            Some(crate::state::context_types::TimelineTrackReorderState {
                                composition_id: comp_id,
                                track_id: track.id,
                                source_index,
                                hover_insertion_slot: None,
                            });
                    }
                }

                track_list_painter.rect_filled(
                    row_rect,
                    0.0,
                    if selected_track_id == Some(track.id) {
                        egui::Color32::from_rgb(50, 80, 120)
                    } else if visible_row_index.is_multiple_of(2) {
                        egui::Color32::from_gray(50)
                    } else {
                        egui::Color32::from_gray(60)
                    },
                );

                if editor_context
                    .interaction
                    .timeline_track_reorder
                    .as_ref()
                    .is_some_and(|reorder| reorder.track_id == track.id)
                {
                    track_list_painter.rect_stroke(
                        row_rect.shrink(1.0),
                        0.0,
                        egui::Stroke::new(2.0, egui::Color32::from_rgb(90, 190, 255)),
                        egui::StrokeKind::Inside,
                    );
                }

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
                crate::qa::register_component_with_metadata(
                    format!("timeline.track_expand:{}", track.id),
                    "timeline_track_expand",
                    icon_rect,
                    true,
                    Some(serde_json::json!({"expanded": is_expanded})),
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
                if editor_context.interaction.renaming_track_id == Some(track.id) {
                    // Draw inline TextEdit for renaming
                    let text_rect = egui::Rect::from_min_size(
                        row_rect.left_center() + egui::vec2(text_offset_x, -10.0),
                        egui::vec2(row_rect.width() - text_offset_x - 10.0, 20.0),
                    );
                    let text_edit =
                        egui::TextEdit::singleline(&mut editor_context.interaction.rename_buffer)
                            .font(egui::FontId::monospace(10.0))
                            .desired_width(text_rect.width());

                    let response = ui_content.put(text_rect, text_edit);

                    // Focus the text edit automatically
                    if !response.has_focus()
                        && editor_context.interaction.renaming_track_id == Some(track.id)
                    {
                        response.request_focus();
                    }

                    // Confirm on Enter or lose focus
                    let committed = response.lost_focus()
                        || (response.has_focus()
                            && ui_content.input(|i| i.key_pressed(egui::Key::Enter)));

                    if committed {
                        // Commit the rename
                        let new_name = editor_context.interaction.rename_buffer.clone();
                        // Only update if name changed and is not empty
                        if !new_name.is_empty() && new_name != track.name {
                            deferred_actions.push(DeferredTrackAction::Rename {
                                track_id: track.id,
                                new_name,
                            });
                        }

                        // Clear rename state
                        editor_context.interaction.renaming_track_id = None;
                        editor_context.interaction.rename_buffer.clear();
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
                    if visible_row_index.is_multiple_of(2) {
                        egui::Color32::from_gray(45)
                    } else {
                        egui::Color32::from_gray(55)
                    },
                );

                let indent = *depth as f32 * 10.0;
                let text_offset_x = 5.0 + indent + 16.0; // Extra indent for clip (no folder icon)

                let clip_name = proj_read.as_ref().map_or_else(
                    || clip.name.clone(),
                    |project| expanded_clip_label(project, clip, &asset_names),
                );

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

    let insertion_markers = track_insertion_markers(
        &display_rows,
        track_list_rect.min.y,
        editor_context.timeline.scroll_offset.y,
        row_height,
        track_spacing,
    );
    let pointer_position = ui_content.ctx().pointer_latest_pos();

    for (slot, marker_y) in &insertion_markers {
        let rect = egui::Rect::from_min_max(
            egui::pos2(track_list_rect.left(), *marker_y - 4.0),
            egui::pos2(track_list_rect.right(), *marker_y + 4.0),
        );
        crate::qa::register_component_with_metadata(
            format!("timeline.track_insertion_slot:{slot}"),
            "timeline_track_insertion_slot",
            rect,
            true,
            Some(serde_json::json!({
                "slot": slot,
                "composition_id": selected_composition_id,
            })),
        );
    }

    if let Some(reorder) = editor_context.interaction.timeline_track_reorder.as_mut() {
        reorder.hover_insertion_slot = pointer_position
            .filter(|pointer| track_list_rect.contains(*pointer))
            .and_then(|pointer| nearest_track_insertion_slot(pointer.y, &insertion_markers))
            .map(|(slot, _)| slot);

        ui_content.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        ui_content.ctx().request_repaint();
    }

    if let Some(reorder) = editor_context.interaction.timeline_track_reorder.as_ref() {
        if let Some(marker_y) = reorder.hover_insertion_slot.and_then(|slot| {
            insertion_markers
                .iter()
                .find_map(|(candidate, y)| (*candidate == slot).then_some(*y))
        }) {
            let marker_y =
                marker_y.clamp(track_list_rect.top() + 1.0, track_list_rect.bottom() - 1.0);
            track_list_painter.line_segment(
                [
                    egui::pos2(track_list_rect.left() + 4.0, marker_y),
                    egui::pos2(track_list_rect.right() - 4.0, marker_y),
                ],
                egui::Stroke::new(3.0, egui::Color32::from_rgb(90, 190, 255)),
            );
        }
    }

    let cancel_track_reorder = ui_content.input(|input| input.key_pressed(egui::Key::Escape));
    let primary_released =
        ui_content.input(|input| input.pointer.button_released(egui::PointerButton::Primary));
    let primary_down = ui_content.input(|input| input.pointer.primary_down());

    if cancel_track_reorder {
        editor_context.interaction.timeline_track_reorder = None;
    } else if primary_released {
        if let Some(reorder) = editor_context.interaction.timeline_track_reorder.take() {
            if Some(reorder.composition_id) == selected_composition_id {
                if let Some(destination_index) = reorder.hover_insertion_slot.and_then(|slot| {
                    destination_index_for_slot(reorder.source_index, slot, track_ids.len())
                }) {
                    if destination_index != reorder.source_index {
                        deferred_actions.push(DeferredTrackAction::Move {
                            comp_id: reorder.composition_id,
                            track_id: reorder.track_id,
                            destination_index,
                        });
                    }
                }
            }
        }
    } else if editor_context.interaction.timeline_track_reorder.is_some() && !primary_down {
        // Covers pointer cancellation/window focus loss where no release event
        // reaches egui. Project remains untouched.
        editor_context.interaction.timeline_track_reorder = None;
    }

    track_list_response.context_menu(|ui_content| {
        if let Some(comp_id) = editor_context.active_composition_id {
            if ui_content
                .add(egui::Button::new(egui::RichText::new(format!(
                    "{} Add Track",
                    icons::PLUS
                ))))
                .clicked()
            {
                deferred_actions.push(DeferredTrackAction::Add { comp_id });
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
            DeferredTrackAction::Add { comp_id } => {
                if let Err(e) = project_service.add_track(comp_id, "New Track") {
                    error!("Failed to add track: {:?}", e);
                } else {
                    needs_history_push = true;
                }
            }
            DeferredTrackAction::Remove { comp_id, track_id } => {
                if let Err(e) = project_service.remove_track(comp_id, track_id) {
                    error!("Failed to remove track: {:?}", e);
                } else {
                    needs_history_push = true;
                }
            }
            DeferredTrackAction::Rename { track_id, new_name } => {
                if let Err(e) = project_service.rename_track(track_id, &new_name) {
                    error!("Failed to rename track: {:?}", e);
                } else {
                    needs_history_push = true;
                }
            }
            DeferredTrackAction::Move {
                comp_id,
                track_id,
                destination_index,
            } => match project_service.move_track_within_composition(
                comp_id,
                track_id,
                destination_index,
            ) {
                Ok(changed) => needs_history_push |= changed,
                Err(e) => error!("Failed to reorder track: {:?}", e),
            },
        }
    }

    if needs_history_push {
        if let Ok(proj) = project.read() {
            editor_context.reconcile_selection(&proj);
            history_manager.push_project_state(proj.clone());
        }
    }

    (num_rows, row_height, track_spacing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::generator_node;
    use library::editor::project_service::GeneratorNodeRequest;
    use library::model::frame::color::Color;
    use library::model::project::{
        NodeContainer, PortAddress, IMAGE_INPUT_PORT, IMAGE_OUTPUT_PORT, MERGE_IMAGES_PORT,
    };
    use library::model::{Node, Track};
    use library::plugin::PluginManager;

    #[test]
    fn track_highlight_is_derived_from_typed_primary_owner() {
        let mut project = Project::new("typed track highlight");
        let (composition, clip_track) =
            library::model::Composition::new("composition", 320, 180, 30.0, 2.0);
        let composition_id = composition.id;
        let clip_track_id = clip_track.id;
        let node_track = Track::new("node track");
        let node_track_id = node_track.id;
        let shared_id = Uuid::new_v4();
        let mut clip = Clip::new("same UUID Clip", 0.0, 1.0);
        clip.id = shared_id;
        let mut node = Node::new_merge("same UUID Node");
        node.id = shared_id;

        project
            .add_track(clip_track)
            .expect("container structural Merge insertion must succeed");
        project
            .add_track(node_track)
            .expect("container structural Merge insertion must succeed");
        project.add_clip(clip);
        project.add_node(node);
        project
            .add_composition(composition)
            .expect("container structural Merge insertion must succeed");
        project
            .attach_track_to_composition(composition_id, node_track_id)
            .unwrap();
        project
            .attach_clip_to_track(clip_track_id, shared_id)
            .unwrap();
        project
            .attach_node_to_container(NodeContainer::Track(node_track_id), shared_id)
            .unwrap();

        assert_eq!(
            track_for_selection(&project, SelectionTarget::Clip(shared_id)),
            Some(clip_track_id)
        );
        assert_eq!(
            track_for_selection(&project, SelectionTarget::Node(shared_id)),
            Some(node_track_id)
        );
        assert_eq!(
            track_for_selection(&project, SelectionTarget::Track(node_track_id)),
            Some(node_track_id)
        );
        assert_eq!(
            track_for_selection(&project, SelectionTarget::Composition(composition_id)),
            None
        );
    }

    #[test]
    fn expanded_clip_label_uses_source_instead_of_terminal_effect_or_merge(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::new("timeline label");
        let clip = Clip::new("Clip fallback", 0.0, 5.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        let container = NodeContainer::Clip(clip_id);

        let source = generator_node(
            "Authored source",
            GeneratorNodeRequest::Solid {
                color: Color::black(),
            },
        );
        let source_id = source.id;
        project.add_node(source);
        project.attach_node_to_container(container, source_id)?;
        let effect = PluginManager::default().create_effect_operation_node("blur")?;
        let effect_id = effect.id;
        project.add_node(effect);
        project.attach_node_to_container(container, effect_id)?;
        let merge = Node::new_merge("Terminal Merge");
        let merge_id = merge.id;
        project.add_node(merge);
        project.attach_node_to_container(container, merge_id)?;
        project.connect_ports(
            PortAddress::new(PortOwner::Node(source_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(effect_id), IMAGE_INPUT_PORT),
        )?;
        project.connect_ports(
            PortAddress::new(PortOwner::Node(effect_id), IMAGE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(merge_id), MERGE_IMAGES_PORT),
        )?;

        let clip = project.get_clip(clip_id).cloned().ok_or(
            library::model::project::ProjectGraphError::ClipNotFound(clip_id),
        )?;
        project.set_output_node(container, Some(effect_id))?;
        assert_eq!(
            expanded_clip_label(&project, &clip, &HashMap::new()),
            "Authored source"
        );
        project.set_output_node(container, Some(merge_id))?;
        assert_eq!(
            expanded_clip_label(&project, &clip, &HashMap::new()),
            "Authored source"
        );
        Ok(())
    }

    #[test]
    fn expanded_clip_label_does_not_escape_through_a_foreign_output_binding(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut project = Project::new("malformed timeline label");
        let clip = Clip::new("Clip fallback", 0.0, 5.0);
        let clip_id = clip.id;
        project.add_clip(clip);
        let foreign_clip = Clip::new("foreign clip", 0.0, 5.0);
        let foreign_clip_id = foreign_clip.id;
        project.add_clip(foreign_clip);
        let foreign_source = generator_node(
            "Foreign source",
            GeneratorNodeRequest::Solid {
                color: Color::black(),
            },
        );
        let foreign_source_id = foreign_source.id;
        project.add_node(foreign_source);
        project
            .attach_node_to_container(NodeContainer::Clip(foreign_clip_id), foreign_source_id)?;
        project.set_output_node(
            NodeContainer::Clip(foreign_clip_id),
            Some(foreign_source_id),
        )?;

        project
            .get_clip_mut(clip_id)
            .ok_or(library::model::project::ProjectGraphError::ClipNotFound(
                clip_id,
            ))?
            .output_node_id = Some(foreign_source_id);
        let clip = project.get_clip(clip_id).cloned().ok_or(
            library::model::project::ProjectGraphError::ClipNotFound(clip_id),
        )?;
        assert_eq!(
            expanded_clip_label(&project, &clip, &HashMap::new()),
            "Clip fallback"
        );
        Ok(())
    }

    #[test]
    fn insertion_slots_cover_first_last_and_follow_vertical_scroll() {
        let tracks = [Track::new("A"), Track::new("B"), Track::new("C")];
        let rows: Vec<_> = tracks
            .iter()
            .enumerate()
            .map(|(visible_row_index, track)| {
                super::super::utils::flatten::DisplayRow::TrackHeader {
                    track,
                    depth: 0,
                    is_expanded: false,
                    visible_row_index,
                }
            })
            .collect();

        let markers = track_insertion_markers(&rows, 100.0, 32.0, 30.0, 2.0);

        assert_eq!(markers, vec![(0, 67.0), (1, 99.0), (2, 131.0), (3, 163.0)]);
        assert_eq!(
            nearest_track_insertion_slot(66.0, &markers),
            Some((0, 67.0))
        );
        assert_eq!(
            nearest_track_insertion_slot(164.0, &markers),
            Some((3, 163.0))
        );
    }

    #[test]
    fn insertion_slot_conversion_supports_up_down_and_stable_no_op_drops() {
        // Move C before A, then A after C in a four-Track list.
        assert_eq!(destination_index_for_slot(2, 0, 4), Some(0));
        assert_eq!(destination_index_for_slot(0, 3, 4), Some(2));

        // The slots immediately before and after B both retain B at index 1.
        assert_eq!(destination_index_for_slot(1, 1, 4), Some(1));
        assert_eq!(destination_index_for_slot(1, 2, 4), Some(1));

        assert_eq!(destination_index_for_slot(0, 0, 0), None);
        assert_eq!(destination_index_for_slot(4, 0, 4), None);
        assert_eq!(destination_index_for_slot(0, 5, 4), None);
    }
}
