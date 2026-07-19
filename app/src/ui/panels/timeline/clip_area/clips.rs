use egui::{epaint::StrokeKind, Ui};
use egui_phosphor::regular as icons;
use library::audio::cache::{AudioChunkKey, AudioDecodeFormat, AudioSourceKey};
use library::model::asset::AssetKind;
use library::model::project::Project;
use library::model::{Clip, Node, NodeContent, Track};
use library::EditorService as ProjectService;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::{action::HistoryManager, state::context::EditorContext};

use super::super::utils::flatten::{flatten_tracks_to_rows, DisplayRow};

const EDGE_DRAG_WIDTH: f32 = 5.0;

/// Deferred actions collected during UI phase, executed after read lock is released
#[derive(Debug)]
enum DeferredClipAction {
    /// Update clip timing (resize/move start)
    UpdateClipTiming {
        clip_id: Uuid,
        new_start_time: f64,
        new_duration: f64,
        new_trim_in: f64,
    },
    MoveClip {
        composition_id: Uuid,
        source_track_id: Uuid,
        clip_id: Uuid,
        target_track_id: Uuid,
        new_start_time: f64,
        target_index: Option<usize>,
    },

    /// Remove clip from track
    RemoveClip { track_id: Uuid, clip_id: Uuid },
    /// Push history state after changes
    PushHistory,
}

#[allow(clippy::too_many_arguments)]
fn calculate_clip_rect(
    start_time: f64,
    duration: f64,
    track_index: usize,
    scroll_offset: egui::Vec2,
    pixels_per_unit: f32,
    row_height: f32,
    track_spacing: f32,
    _composition_fps: f64,
    base_offset: egui::Vec2,
) -> egui::Rect {
    let initial_x = base_offset.x + (start_time as f32) * pixels_per_unit - scroll_offset.x;
    let initial_y =
        base_offset.y - scroll_offset.y + track_index as f32 * (row_height + track_spacing);

    let width = (duration as f32) * pixels_per_unit;
    let safe_width = width.max(1.0);

    egui::Rect::from_min_size(
        egui::pos2(initial_x, initial_y),
        egui::vec2(safe_width, row_height),
    )
}

fn draw_waveform(
    painter: &egui::Painter,
    clip_rect: egui::Rect,
    audio_data: &[f32],
    audio_start_time: f64,
    _layer_start_time: f64,
    trim_in: f64,
    _composition_fps: f64,
    pixels_per_unit: f32,
    sample_rate: f64,
    channels: usize,
) {
    let rect_w = clip_rect.width();
    let rect_h = clip_rect.height();
    let center_y = clip_rect.center().y;
    let max_amp_height = rect_h * 0.4;

    let samples_per_pixel = (sample_rate / pixels_per_unit as f64) * channels as f64;
    let step_width = if samples_per_pixel > 1000.0 { 2.0 } else { 1.0 };
    let mut x = 0.0;

    while x < rect_w {
        let _time_offset = x as f32 / pixels_per_unit;
        // Source time = Time since start of clip (in source media)
        // Clip shows [trim_in, trim_in + duration] of source
        let source_time = trim_in + _time_offset as f64;

        // Map to sample index
        let start_sample_idx = if source_time >= audio_start_time {
            ((source_time - audio_start_time) * sample_rate) as usize * channels
        } else {
            audio_data.len() + 1
        };
        let end_sample_idx = start_sample_idx + samples_per_pixel as usize;

        if start_sample_idx < audio_data.len() {
            let end = end_sample_idx.min(audio_data.len());
            let mut max_amp = 0.0f32;
            let stride = if end - start_sample_idx > 100 { 10 } else { 1 };

            for i in (start_sample_idx..end).step_by(stride) {
                let abs_val = audio_data[i].abs();
                if abs_val > max_amp {
                    max_amp = abs_val;
                }
            }

            if max_amp > 0.0 {
                let height = (max_amp * max_amp_height as f32).max(1.0);
                let x_pos = clip_rect.min.x + x;
                painter.line_segment(
                    [
                        egui::pos2(x_pos, center_y - height),
                        egui::pos2(x_pos, center_y + height),
                    ],
                    egui::Stroke::new(1.0, egui::Color32::from_rgba_premultiplied(0, 0, 0, 100)),
                );
            }
        }
        x += step_width;
    }
}

fn collect_track_clips<'a>(project: &'a Project, track: &'a Track, clips: &mut Vec<&'a Clip>) {
    for clip_id in &track.clip_ids {
        if let Some(clip) = project.get_clip(*clip_id) {
            clips.push(clip);
        }
    }
}

fn primary_node<'a>(clip: &Clip, project: &'a Project) -> Option<&'a Node> {
    clip.output_node_id
        .and_then(|node_id| project.get_node(node_id))
        .or_else(|| {
            clip.node_ids
                .iter()
                .find_map(|node_id| project.get_node(*node_id))
        })
}

fn get_clip_color(clip: &Clip, project: &Project) -> (u8, u8, u8) {
    match primary_node(clip, project).map(|node| &node.content) {
        Some(NodeContent::Media(m)) => {
            if let Some(asset) = project.assets.iter().find(|a| a.id == m.asset_id) {
                match asset.kind {
                    AssetKind::Audio => (100, 200, 100),
                    AssetKind::Video => (100, 100, 200),
                    AssetKind::Image => (150, 100, 200),
                    _ => (150, 150, 150),
                }
            } else {
                (200, 50, 50) // Missing asset
            }
        }
        Some(NodeContent::Generator(generator)) => match generator {
            library::model::GeneratorContent::Shape => (200, 200, 100),
            library::model::GeneratorContent::Text => (200, 150, 100),
            library::model::GeneratorContent::SkSL => (100, 200, 200),
            library::model::GeneratorContent::Solid => (150, 150, 150),
        },
        Some(NodeContent::PluginOperation(_)) => (180, 110, 210),
        Some(NodeContent::Reference(_)) | Some(NodeContent::Merge) | None => (150, 150, 150),
    }
}

#[allow(clippy::too_many_arguments)]
pub fn calculate_insert_index(
    mouse_y: f32,
    content_rect_min_y: f32,
    scroll_offset_y: f32,
    row_height: f32,
    track_spacing: f32,
    display_rows: &[DisplayRow],
    project: &Project,
    _track_ids: &[Uuid],
    hovered_track_id: Uuid,
) -> Option<(usize, usize)> {
    // Returns (target_index, header_row_index)

    // Find header row for hovered track
    if let Some((header_idx, _)) = display_rows.iter().enumerate().find(|(_, r)| {
        r.track_id() == hovered_track_id && matches!(r, DisplayRow::TrackHeader { .. })
    }) {
        let current_y_in_clip_area = mouse_y - content_rect_min_y + scroll_offset_y;

        let hovered_row_index =
            (current_y_in_clip_area / (row_height + track_spacing)).floor() as isize;
        let header_row_index = header_idx as isize;

        let raw_target_index = hovered_row_index - header_row_index - 1;

        // Clamp to valid range
        if let Some(track) = project.get_track(hovered_track_id) {
            // Count clips in this track
            let clip_count = track.clip_ids.len();

            // Invert index because display order is reversed (Top of UI = End of List)
            let max_index = clip_count as isize;

            let inverted_target = max_index - raw_target_index;
            let target_index = inverted_target.clamp(0, max_index) as usize;

            return Some((target_index, header_idx));
        }
    }
    None
}

/// Logical insertion slots for an expanded Track.  Slot 0 is before the
/// first canonical clip and `clip_count` is after the last.  The Timeline is
/// visually reversed (later clips are higher), so slot numbers descend as Y
/// increases.
fn clip_insertion_markers(
    display_rows: &[DisplayRow],
    track_id: Uuid,
    content_rect_min_y: f32,
    scroll_offset_y: f32,
    row_height: f32,
    track_spacing: f32,
    project: &Project,
) -> Vec<(usize, f32)> {
    let Some(header_row) = display_rows.iter().position(|row| {
        row.track_id() == track_id && matches!(row, DisplayRow::TrackHeader { .. })
    }) else {
        return Vec::new();
    };
    let Some(track) = project.get_track(track_id) else {
        return Vec::new();
    };
    let row_step = row_height + track_spacing;
    let clip_count = track.clip_ids.len();
    (0..=clip_count)
        .map(|slot| {
            let boundary_row = header_row + 1 + (clip_count - slot);
            (
                slot,
                content_rect_min_y + boundary_row as f32 * row_step - scroll_offset_y,
            )
        })
        .collect()
}

fn nearest_clip_insertion_slot(pointer_y: f32, markers: &[(usize, f32)]) -> Option<usize> {
    markers
        .iter()
        .min_by(|(_, lhs_y), (_, rhs_y)| {
            (pointer_y - *lhs_y)
                .abs()
                .total_cmp(&(pointer_y - *rhs_y).abs())
        })
        .map(|(slot, _)| *slot)
}

/// Convert an insertion slot in the original list into the index expected
/// after the source Clip is detached.  The two slots directly adjacent to a
/// Clip are intentional no-ops, which is what keeps a horizontal timing drag
/// from silently changing layer order.
fn destination_index_for_clip_slot(
    same_track: bool,
    source_index: usize,
    insertion_slot: usize,
    target_clip_count: usize,
) -> Option<usize> {
    if !same_track {
        return Some(insertion_slot.min(target_clip_count));
    }
    if target_clip_count == 0 || source_index >= target_clip_count {
        return None;
    }
    let destination = if insertion_slot > source_index {
        insertion_slot - 1
    } else {
        insertion_slot
    }
    .min(target_clip_count - 1);
    (destination != source_index).then_some(destination)
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ClipTiming {
    start_time: f64,
    duration: f64,
    trim_in: f64,
}

fn timing_after_left_edge_drag(clip: &Clip, delta_time: f64) -> Option<ClipTiming> {
    let start_time = clip.start_time.into_inner() + delta_time;
    let duration = clip.duration.into_inner() - delta_time;
    // local_time(t) = (t - start) * time_stretch + trim_in. At the new
    // boundary, preserving content therefore advances trim by delta*stretch.
    let trim_in = clip.trim_in.into_inner() + delta_time * clip.time_stretch.into_inner();
    (start_time >= 0.0 && duration > 0.0 && trim_in >= 0.0).then_some(ClipTiming {
        start_time,
        duration,
        trim_in,
    })
}

fn timing_after_body_drag(clip: &Clip, delta_time: f64) -> Option<ClipTiming> {
    let start_time = (clip.start_time.into_inner() + delta_time).max(0.0);
    (start_time != clip.start_time.into_inner()).then_some(ClipTiming {
        start_time,
        duration: clip.duration.into_inner(),
        trim_in: clip.trim_in.into_inner(),
    })
}

#[allow(clippy::too_many_arguments)]
pub fn draw_clips(
    ui_content: &mut Ui,
    content_rect_for_clip_area: egui::Rect,
    editor_context: &mut EditorContext,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    project: &Arc<RwLock<Project>>,
    track_ids: &[Uuid],
    pixels_per_unit: f32,
    row_height: f32,
    track_spacing: f32,
    composition_fps: f64,
) -> bool {
    let mut clicked_on_entity = false;
    let mut deferred_actions: Vec<DeferredClipAction> = Vec::new();

    // ===== PHASE 1: Read lock scope - UI rendering and action collection =====
    {
        let proj_read = match project.read() {
            Ok(p) => p,
            Err(_) => return false,
        };

        // Flatten tracks for display using new DisplayRow system
        let display_rows = flatten_tracks_to_rows(
            &proj_read,
            track_ids,
            &editor_context.timeline.expanded_tracks,
        );

        for track_id in track_ids {
            if !editor_context.timeline.expanded_tracks.contains(track_id) {
                continue;
            }
            for (slot, y) in clip_insertion_markers(
                &display_rows,
                *track_id,
                content_rect_for_clip_area.min.y,
                editor_context.timeline.scroll_offset.y,
                row_height,
                track_spacing,
                &proj_read,
            ) {
                let rect = egui::Rect::from_min_max(
                    egui::pos2(content_rect_for_clip_area.min.x, y - 4.0),
                    egui::pos2(content_rect_for_clip_area.max.x, y + 4.0),
                );
                crate::qa::register_component_with_metadata(
                    format!("timeline.clip_insertion_slot.{track_id}:{slot}"),
                    "timeline_clip_insertion_slot",
                    rect,
                    true,
                    Some(serde_json::json!({
                        "track_id": track_id,
                        "slot": slot,
                    })),
                );
            }
        }

        // Calculate Reorder State if dragging
        let mut reorder_state = None;
        if let (Some(dragged_id), Some(hovered_tid)) = (
            editor_context.selection.last_selected_entity_id,
            editor_context.interaction.dragged_entity_hovered_track_id,
        ) {
            if let Some(mouse_pos) = ui_content.ctx().pointer_latest_pos() {
                if let Some((target_index, header_idx)) = calculate_insert_index(
                    mouse_pos.y,
                    content_rect_for_clip_area.min.y,
                    editor_context.timeline.scroll_offset.y,
                    row_height,
                    track_spacing,
                    &display_rows,
                    &proj_read,
                    track_ids,
                    hovered_tid,
                ) {
                    // Find dragged clip original info
                    let source_track_id = editor_context
                        .interaction
                        .dragged_entity_original_track_id
                        .unwrap_or(hovered_tid);
                    if let Some(dragged_original_index) =
                        proj_read.get_track(source_track_id).and_then(|track| {
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

        for row in &display_rows {
            match row {
                DisplayRow::TrackHeader {
                    track,
                    visible_row_index,
                    is_expanded,
                    ..
                } => {
                    // If collapsed, draw all clips on this row
                    if !is_expanded {
                        let mut clips_to_draw: Vec<&Clip> = Vec::new();
                        collect_track_clips(&proj_read, track, &mut clips_to_draw);

                        for clip in clips_to_draw {
                            draw_single_clip(
                                ui_content,
                                content_rect_for_clip_area,
                                editor_context,
                                &mut deferred_actions,
                                project_service,
                                &proj_read,
                                track_ids,
                                clip,
                                track,
                                *visible_row_index,
                                pixels_per_unit,
                                row_height,
                                track_spacing,
                                composition_fps,
                                false,
                                &mut clicked_on_entity,
                                &display_rows,
                                &reorder_state,
                            );
                        }
                    }
                }
                DisplayRow::ClipRow {
                    clip,
                    parent_track,
                    visible_row_index,
                    ..
                } => {
                    // Draw single clip on its own row
                    draw_single_clip(
                        ui_content,
                        content_rect_for_clip_area,
                        editor_context,
                        &mut deferred_actions,
                        project_service,
                        &proj_read,
                        track_ids,
                        clip,
                        parent_track,
                        *visible_row_index,
                        pixels_per_unit,
                        row_height,
                        track_spacing,
                        composition_fps,
                        false,
                        &mut clicked_on_entity,
                        &display_rows,
                        &reorder_state,
                    );
                }
            }
        }

        // Draw asset drag preview indicator
        if let Some(ref _dragged_item) = editor_context.interaction.dragged_item {
            if let Some(mouse_pos) = ui_content.ctx().pointer_latest_pos() {
                if content_rect_for_clip_area.contains(mouse_pos) {
                    // Calculate insert position
                    let relative_y = mouse_pos.y - content_rect_for_clip_area.min.y
                        + editor_context.timeline.scroll_offset.y;
                    let row_with_spacing = row_height + track_spacing;
                    let row_index = (relative_y / row_with_spacing).floor() as usize;

                    // Determine if we're in the top or bottom half of a row
                    let y_in_row = relative_y % row_with_spacing;
                    let insert_at_top = y_in_row < row_height / 2.0;

                    // Calculate the Y position for the indicator line
                    let indicator_row = if insert_at_top {
                        row_index
                    } else {
                        row_index + 1
                    };
                    let indicator_y = content_rect_for_clip_area.min.y
                        + (indicator_row as f32 * row_with_spacing)
                        - editor_context.timeline.scroll_offset.y;

                    // Draw a horizontal line indicator
                    let painter = ui_content.painter();
                    let line_start = egui::pos2(content_rect_for_clip_area.min.x, indicator_y);
                    let line_end = egui::pos2(content_rect_for_clip_area.max.x, indicator_y);
                    painter.line_segment(
                        [line_start, line_end],
                        egui::Stroke::new(3.0, egui::Color32::from_rgb(100, 200, 255)),
                    );

                    // Draw small triangles at the edges
                    let triangle_size = 8.0;
                    painter.add(egui::Shape::convex_polygon(
                        vec![
                            egui::pos2(line_start.x, indicator_y - triangle_size),
                            egui::pos2(line_start.x + triangle_size, indicator_y),
                            egui::pos2(line_start.x, indicator_y + triangle_size),
                        ],
                        egui::Color32::from_rgb(100, 200, 255),
                        egui::Stroke::NONE,
                    ));
                    painter.add(egui::Shape::convex_polygon(
                        vec![
                            egui::pos2(line_end.x, indicator_y - triangle_size),
                            egui::pos2(line_end.x - triangle_size, indicator_y),
                            egui::pos2(line_end.x, indicator_y + triangle_size),
                        ],
                        egui::Color32::from_rgb(100, 200, 255),
                        egui::Stroke::NONE,
                    ));
                }
            }
        }
    } // proj_read dropped here

    // ===== PHASE 2: Execute deferred actions (no lock held) =====
    let mut needs_history_push = false;
    let mut removed_clip_ids: Vec<Uuid> = Vec::new();
    for action in deferred_actions {
        match action {
            DeferredClipAction::UpdateClipTiming {
                clip_id,
                new_start_time,
                new_duration,
                new_trim_in,
            } => {
                project_service
                    .update_clip_timing(clip_id, new_start_time, new_duration, new_trim_in)
                    .ok();
            }
            DeferredClipAction::MoveClip {
                composition_id,
                source_track_id,
                clip_id,
                target_track_id,
                new_start_time,
                target_index,
            } => {
                match project_service.move_clip_to_track_at_index(
                    composition_id,
                    source_track_id,
                    clip_id,
                    target_track_id,
                    new_start_time,
                    target_index,
                ) {
                    Ok(()) => needs_history_push = true,
                    Err(error) => log::error!("Failed to move timeline clip: {error}"),
                }
            }

            DeferredClipAction::RemoveClip { track_id, clip_id } => {
                if let Err(e) = project_service.remove_clip_from_track(track_id, clip_id) {
                    log::error!("Failed to remove clip: {:?}", e);
                } else {
                    removed_clip_ids.push(clip_id);
                    needs_history_push = true;
                }
            }
            DeferredClipAction::PushHistory => {
                needs_history_push = true;
            }
        }
    }

    // Update selection for removed clips
    for clip_id in &removed_clip_ids {
        editor_context.selection.selected_entities.remove(clip_id);
        if editor_context.selection.last_selected_entity_id == Some(*clip_id) {
            editor_context.selection.last_selected_entity_id = None;
            editor_context.selection.last_selected_track_id = None;
        }
    }

    if needs_history_push {
        if let Ok(proj) = project.read() {
            history_manager.push_project_state(proj.clone());
        }
    }

    clicked_on_entity
}

#[allow(clippy::too_many_arguments)]
fn draw_single_clip(
    ui_content: &mut Ui,
    content_rect_for_clip_area: egui::Rect,
    editor_context: &mut EditorContext,
    deferred_actions: &mut Vec<DeferredClipAction>,
    project_service: &ProjectService,
    project: &Project,
    _track_ids: &[Uuid],
    clip: &Clip,
    track: &Track,
    row_index: usize,
    pixels_per_unit: f32,
    row_height: f32,
    track_spacing: f32,
    composition_fps: f64,
    is_summary_clip: bool,
    clicked_on_entity: &mut bool,
    display_rows: &[DisplayRow],
    reorder_state: &Option<(Uuid, Uuid, usize, usize, usize)>,
) {
    // Determine Color based on kind helper
    let (r, g, b) = get_clip_color(clip, project);
    let clip_color = egui::Color32::from_rgb(r, g, b);

    // Apply Live Reordering Visual Shift
    let mut visual_row_index = row_index;

    // Check if we are in a reordering state
    if let Some((dragged_id, r_track_id, src_idx, dst_idx, header_row_idx)) = reorder_state {
        if clip.id == *dragged_id {
            visual_row_index = header_row_idx + 1 + dst_idx;
        } else if track.id == *r_track_id {
            // Get original child index from DisplayRow if available
            let mut original_child_index = None;
            if let Some(DisplayRow::ClipRow { child_index, .. }) = display_rows.get(row_index) {
                original_child_index = Some(*child_index);
            }

            if let Some(idx) = original_child_index {
                let mut new_child_index = idx;
                let src = *src_idx;
                let dst = *dst_idx;

                let is_same_track_sort = if let Some(orig_tid) =
                    editor_context.interaction.dragged_entity_original_track_id
                {
                    orig_tid == *r_track_id
                } else {
                    false
                };

                if is_same_track_sort {
                    if src < dst {
                        if idx > src && idx <= dst {
                            new_child_index = idx - 1;
                        }
                    } else if src > dst {
                        if idx >= dst && idx < src {
                            new_child_index = idx + 1;
                        }
                    }
                } else {
                    if idx >= dst {
                        new_child_index = idx + 1;
                    }
                }

                if new_child_index != idx {
                    visual_row_index = header_row_idx + 1 + new_child_index;
                }
            }
        }
    }

    let initial_clip_rect = calculate_clip_rect(
        *clip.start_time,
        *clip.duration,
        visual_row_index,
        editor_context.timeline.scroll_offset,
        pixels_per_unit,
        row_height,
        track_spacing,
        composition_fps,
        content_rect_for_clip_area.min.to_vec2(),
    );
    let safe_width = initial_clip_rect.width();

    // Visibility Culling
    if !content_rect_for_clip_area.intersects(initial_clip_rect) {
        return;
    }

    if !is_summary_clip {
        let canonical_index = track
            .clip_ids
            .iter()
            .position(|candidate| *candidate == clip.id);
        crate::qa::register_component_with_metadata(
            format!("timeline.clip:{}", clip.id),
            "timeline_clip",
            initial_clip_rect,
            true,
            Some(serde_json::json!({
                "clip_id": clip.id,
                "track_id": track.id,
                "canonical_index": canonical_index,
                "start_time": clip.start_time.into_inner(),
                "duration": clip.duration.into_inner(),
            })),
        );
    }

    // --- Interaction for clips ---
    let sense = if is_summary_clip {
        egui::Sense::click()
    } else {
        egui::Sense::click_and_drag()
    };

    let interaction_id = if is_summary_clip {
        egui::Id::new(clip.id).with("summary").with(row_index)
    } else {
        egui::Id::new(clip.id)
    };

    let clip_resp = ui_content.interact(initial_clip_rect, interaction_id, sense);

    if !is_summary_clip {
        clip_resp.context_menu(|ui| {
            let response = ui.button(format!("{} Remove", icons::TRASH));
            crate::qa::register_component(
                format!("timeline.menu.delete.clip:{}", clip.id),
                "timeline_menu_item",
                response.rect,
            );
            if response.clicked() {
                if let Some(_comp_id) = editor_context.selection.composition_id {
                    deferred_actions.push(DeferredClipAction::RemoveClip {
                        track_id: track.id,
                        clip_id: clip.id,
                    });
                    ui.ctx().request_repaint();
                    ui.close();
                }
            }
        });
    }

    // Edges (Resize)
    let mut left_edge_resp = None;
    let mut right_edge_resp = None;

    if !is_summary_clip {
        let left_edge_rect = egui::Rect::from_min_size(
            egui::pos2(initial_clip_rect.min.x, initial_clip_rect.min.y),
            egui::vec2(EDGE_DRAG_WIDTH, initial_clip_rect.height()),
        );
        left_edge_resp = Some(ui_content.interact(
            left_edge_rect,
            egui::Id::new(clip.id).with("left_edge"),
            egui::Sense::drag(),
        ));
        crate::qa::register_component_with_metadata(
            format!("timeline.clip_edge.left:{}", clip.id),
            "timeline_clip_edge",
            left_edge_rect,
            true,
            Some(serde_json::json!({"side": "left", "clip_id": clip.id})),
        );

        let right_edge_rect = egui::Rect::from_min_size(
            egui::pos2(
                initial_clip_rect.max.x - EDGE_DRAG_WIDTH,
                initial_clip_rect.min.y,
            ),
            egui::vec2(EDGE_DRAG_WIDTH, initial_clip_rect.height()),
        );
        right_edge_resp = Some(ui_content.interact(
            right_edge_rect,
            egui::Id::new(clip.id).with("right_edge"),
            egui::Sense::drag(),
        ));
        crate::qa::register_component_with_metadata(
            format!("timeline.clip_edge.right:{}", clip.id),
            "timeline_clip_edge",
            right_edge_rect,
            true,
            Some(serde_json::json!({"side": "right", "clip_id": clip.id})),
        );
    }

    // Handle edge dragging (resize)
    let mut _is_resizing = false;
    if let (Some(left), Some(right)) = (&left_edge_resp, &right_edge_resp) {
        if left.drag_started() || right.drag_started() {
            editor_context.interaction.is_resizing_entity = true;
            editor_context.select_clip(clip.id, track.id);
            _is_resizing = true;
        }
    }

    if editor_context.interaction.is_resizing_entity
        && editor_context.selection.last_selected_entity_id == Some(clip.id)
        && !is_summary_clip
    {
        if let (Some(left), Some(right)) = (&left_edge_resp, &right_edge_resp) {
            let mut new_start_time = clip.start_time.into_inner();
            let mut new_duration = clip.duration.into_inner();
            let mut new_trim_in = clip.trim_in.into_inner();

            let delta_x = if left.dragged() {
                left.drag_delta().x
            } else if right.dragged() {
                right.drag_delta().x
            } else {
                0.0
            };

            // Convert to time
            let delta_time = delta_x / pixels_per_unit;

            if left.dragged() {
                if let Some(timing) = timing_after_left_edge_drag(clip, delta_time as f64) {
                    new_start_time = timing.start_time;
                    new_duration = timing.duration;
                    new_trim_in = timing.trim_in;
                }
            } else if right.dragged() {
                // Moving End: Adjust duration only.
                let proposed_duration = new_duration + delta_time as f64;
                if proposed_duration > 0.0 {
                    new_duration = proposed_duration;
                }
            }

            if new_start_time != clip.start_time.into_inner()
                || new_duration != clip.duration.into_inner()
                || new_trim_in != clip.trim_in.into_inner()
            {
                if let (Some(_comp_id), Some(_tid)) = (
                    editor_context.selection.composition_id,
                    editor_context.selection.last_selected_track_id,
                ) {
                    deferred_actions.push(DeferredClipAction::UpdateClipTiming {
                        clip_id: clip.id,
                        new_start_time,
                        new_duration,
                        new_trim_in,
                    });
                }
            }
        }
    }

    if let (Some(left), Some(right)) = (&left_edge_resp, &right_edge_resp) {
        if left.drag_stopped() || right.drag_stopped() {
            editor_context.interaction.is_resizing_entity = false;
            deferred_actions.push(DeferredClipAction::PushHistory);
        }
    }

    let edge_is_dragging = left_edge_resp
        .as_ref()
        .is_some_and(|response| response.dragged())
        || right_edge_resp
            .as_ref()
            .is_some_and(|response| response.dragged());

    if clip_resp.drag_started() && !edge_is_dragging && !is_summary_clip {
        if !editor_context.is_selected(clip.id) {
            editor_context.select_clip(clip.id, track.id);
        }
        editor_context.interaction.is_moving_selected_entity = true;
        editor_context.interaction.dragged_entity_original_track_id = Some(track.id);
        editor_context.interaction.dragged_entity_hovered_track_id = Some(track.id);
        editor_context.interaction.dragged_entity_has_moved = false;
    }

    if editor_context.interaction.is_moving_selected_entity
        && editor_context.selection.last_selected_entity_id == Some(clip.id)
        && clip_resp.dragged()
        && !edge_is_dragging
    {
        if let Some(pointer) = clip_resp.interact_pointer_pos() {
            let row = ((pointer.y - content_rect_for_clip_area.min.y
                + editor_context.timeline.scroll_offset.y)
                / (row_height + track_spacing))
                .floor()
                .max(0.0) as usize;
            if let Some(target_row) = display_rows.get(row) {
                editor_context.interaction.dragged_entity_hovered_track_id =
                    Some(target_row.track_id());
            }
        }
        let delta_time = clip_resp.drag_delta().x as f64 / pixels_per_unit as f64;
        if let Some(timing) = timing_after_body_drag(clip, delta_time) {
            deferred_actions.push(DeferredClipAction::UpdateClipTiming {
                clip_id: clip.id,
                new_start_time: timing.start_time,
                new_duration: timing.duration,
                new_trim_in: timing.trim_in,
            });
            editor_context.interaction.dragged_entity_has_moved = true;
        }
    }

    if clip_resp.drag_stopped()
        && editor_context.interaction.is_moving_selected_entity
        && editor_context.selection.last_selected_entity_id == Some(clip.id)
        && !edge_is_dragging
        && !is_summary_clip
    {
        let source_track_id = editor_context
            .interaction
            .dragged_entity_original_track_id
            .unwrap_or(track.id);
        let target_track_id = editor_context
            .interaction
            .dragged_entity_hovered_track_id
            .unwrap_or(source_track_id);
        // Horizontal timing changes were applied incrementally to the same
        // authoritative Project while dragging. `drag_delta()` is per-frame
        // and is zero on egui's release frame, so commit the current value.
        let new_start_time = clip.start_time.into_inner();
        let target_index = clip_resp.interact_pointer_pos().and_then(|pointer| {
            let markers = clip_insertion_markers(
                display_rows,
                target_track_id,
                content_rect_for_clip_area.min.y,
                editor_context.timeline.scroll_offset.y,
                row_height,
                track_spacing,
                project,
            );
            let insertion_slot = nearest_clip_insertion_slot(pointer.y, &markers)?;
            let source_index = project
                .get_track(source_track_id)?
                .clip_ids
                .iter()
                .position(|candidate| *candidate == clip.id)?;
            let target_count = project.get_track(target_track_id)?.clip_ids.len();
            destination_index_for_clip_slot(
                source_track_id == target_track_id,
                source_index,
                insertion_slot,
                target_count,
            )
        });

        if let Some(composition_id) = editor_context.selection.composition_id {
            deferred_actions.push(DeferredClipAction::MoveClip {
                composition_id,
                source_track_id,
                clip_id: clip.id,
                target_track_id,
                new_start_time,
                target_index,
            });
        }
        editor_context.interaction.is_moving_selected_entity = false;
        editor_context.interaction.dragged_entity_original_track_id = None;
        editor_context.interaction.dragged_entity_hovered_track_id = None;
        editor_context.interaction.dragged_entity_has_moved = false;
    }

    // Calculate display position
    let mut display_x = initial_clip_rect.min.x;
    let display_y = initial_clip_rect.min.y;

    if editor_context.is_selected(clip.id) && clip_resp.dragged() && !is_summary_clip {
        display_x += clip_resp.drag_delta().x;
    }

    let drawing_clip_rect = egui::Rect::from_min_size(
        egui::pos2(display_x, display_y),
        egui::vec2(safe_width, row_height),
    );

    // --- Drawing ---
    let is_sel_entity = editor_context.is_selected(clip.id);
    let mut transparent_color =
        egui::Color32::from_rgba_premultiplied(clip_color.r(), clip_color.g(), clip_color.b(), 150);

    if is_summary_clip {
        transparent_color = egui::Color32::from_rgba_premultiplied(
            clip_color.r(),
            clip_color.g(),
            clip_color.b(),
            100,
        );
    }

    let painter = ui_content.painter_at(content_rect_for_clip_area);
    painter.rect_filled(drawing_clip_rect, 4.0, transparent_color);

    // Draw Audio Waveform
    if let Some(NodeContent::Media(m)) = primary_node(clip, project).map(|node| &node.content) {
        if let Some(asset) = project
            .assets
            .iter()
            .find(|asset| asset.id == m.asset_id && asset.kind == AssetKind::Audio)
        {
            if safe_width > 10.0 {
                let cache = project_service.get_cache_manager();
                let engine = project_service.get_audio_service().get_audio_engine();
                let sample_rate = engine.get_sample_rate();
                let channels = engine.get_channels();
                let stream_index = m.audio_stream_index.or(asset.stream_index);
                let format = AudioDecodeFormat::new(sample_rate, channels);
                let source = format.and_then(|format| {
                    AudioSourceKey::read(&asset.path, stream_index, format).ok()
                });
                let source_frame = (*clip.trim_in * f64::from(sample_rate)).max(0.0) as u64;
                let key = source.map(|source| AudioChunkKey::containing(source, source_frame));
                if let Some(audio_data) = key.as_ref().and_then(|key| cache.get_audio_chunk(key)) {
                    let audio_start_time =
                        audio_data.key().start_frame() as f64 / f64::from(sample_rate);

                    draw_waveform(
                        &painter,
                        drawing_clip_rect,
                        audio_data.samples(),
                        audio_start_time,
                        *clip.start_time,
                        *clip.trim_in,
                        composition_fps,
                        pixels_per_unit,
                        f64::from(sample_rate),
                        usize::from(channels),
                    );
                }
            }
        }
    }

    if is_sel_entity {
        painter.rect_stroke(
            drawing_clip_rect,
            4.0,
            egui::Stroke::new(2.0, egui::Color32::WHITE),
            StrokeKind::Middle,
        );
    }

    let mut clip_text = primary_node(clip, project)
        .map(|node| node.name.clone())
        .unwrap_or_else(|| clip.name.clone());

    if is_summary_clip {
        clip_text = format!("(Ref) {}", clip_text);
    }

    painter.text(
        drawing_clip_rect.min + egui::vec2(5.0, 5.0),
        egui::Align2::LEFT_TOP,
        &clip_text,
        egui::FontId::default(),
        egui::Color32::BLACK,
    );

    if !is_summary_clip {
        if let (Some(left), Some(right)) = (&left_edge_resp, &right_edge_resp) {
            if left.hovered() || right.hovered() {
                ui_content
                    .ctx()
                    .set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
        }
    }

    if !editor_context.interaction.is_resizing_entity && clip_resp.clicked() {
        let action = crate::ui::selection::get_click_action(
            &ui_content.input(|i| i.modifiers),
            Some(clip.id),
        );

        match action {
            crate::ui::selection::ClickAction::Select(id) => {
                editor_context.select_clip(id, track.id);
            }
            crate::ui::selection::ClickAction::Add(id) => {
                if !editor_context.is_selected(id) {
                    editor_context.toggle_selection(id, track.id);
                }
            }
            crate::ui::selection::ClickAction::Remove(id) => {
                if editor_context.is_selected(id) {
                    editor_context.toggle_selection(id, track.id);
                }
            }
            crate::ui::selection::ClickAction::Toggle(id) => {
                editor_context.toggle_selection(id, track.id);
            }
            _ => {}
        }
        *clicked_on_entity = true;
    }
}

pub fn get_clips_in_box(
    selection_rect: egui::Rect,
    editor_context: &EditorContext,
    project: &Project,
    track_ids: &[uuid::Uuid],
    _pixels_per_unit: f32,
    row_height: f32,
    track_spacing: f32,
    _composition_fps: f64,
    clip_area_top_left: egui::Vec2,
) -> Vec<(uuid::Uuid, uuid::Uuid)> {
    let mut found = Vec::new();
    let display_rows = super::super::utils::flatten::flatten_tracks_to_rows(
        project,
        track_ids,
        &editor_context.timeline.expanded_tracks,
    );

    let scroll_y = editor_context.timeline.scroll_offset.y;
    let scroll_x = editor_context.timeline.scroll_offset.x;
    let pixels_per_second = editor_context.timeline.pixels_per_second;

    for (row_idx, row) in display_rows.iter().enumerate() {
        let track_y = row_idx as f32 * (row_height + track_spacing) - scroll_y;
        if track_y + row_height < selection_rect.min.y || track_y > selection_rect.max.y {
            continue;
        }

        if let Some(track) = project.get_track(row.track_id()) {
            for clip_id in &track.clip_ids {
                if let Some(clip) = project.get_clip(*clip_id) {
                    // Calculate clip rect
                    let start_x =
                        (clip.start_time.into_inner() as f32 * pixels_per_second) - scroll_x;
                    let duration = clip.duration.into_inner();
                    let width = duration as f32 * pixels_per_second;

                    let clip_rect = egui::Rect::from_min_size(
                        egui::Pos2::new(
                            clip_area_top_left.x + start_x,
                            clip_area_top_left.y + track_y,
                        ),
                        egui::Vec2::new(width, row_height),
                    );

                    if selection_rect.intersects(clip_rect) {
                        found.push((clip.id, track.id));
                    }
                }
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn expanded_track_project() -> (Project, Uuid, Vec<Uuid>) {
        let mut project = Project::new("timeline reorder");
        let mut track = Track::new("Track");
        let track_id = track.id;
        let clips = [
            Clip::new("A", 0.0, 1.0),
            Clip::new("B", 1.0, 1.0),
            Clip::new("C", 2.0, 1.0),
        ];
        let clip_ids = clips.iter().map(|clip| clip.id).collect::<Vec<_>>();
        track.clip_ids = clip_ids.clone();
        for clip in clips {
            project.add_clip(clip);
        }
        project.add_track(track);
        (project, track_id, clip_ids)
    }

    #[test]
    fn expanded_track_exposes_every_canonical_insertion_slot_in_reverse_screen_order() {
        let (project, track_id, _) = expanded_track_project();
        let rows = flatten_tracks_to_rows(&project, &[track_id], &HashSet::from([track_id]));
        let markers = clip_insertion_markers(&rows, track_id, 100.0, 30.0, 30.0, 2.0, &project);

        assert_eq!(
            markers,
            vec![(0, 198.0), (1, 166.0), (2, 134.0), (3, 102.0)]
        );
        assert_eq!(nearest_clip_insertion_slot(103.0, &markers), Some(3));
        assert_eq!(nearest_clip_insertion_slot(197.0, &markers), Some(0));
    }

    #[test]
    fn horizontal_same_track_drop_is_a_noop_but_vertical_slot_reorders() {
        // A is index 0. Its adjacent slots (0 and 1) retain its order while
        // the slot after C detaches A and inserts it at destination index 2.
        assert_eq!(destination_index_for_clip_slot(true, 0, 0, 3), None);
        assert_eq!(destination_index_for_clip_slot(true, 0, 1, 3), None);
        assert_eq!(destination_index_for_clip_slot(true, 0, 3, 3), Some(2));
        assert_eq!(destination_index_for_clip_slot(true, 2, 0, 3), Some(0));

        // Cross-Track slots are already destination indices because the
        // source is detached from a different list.
        assert_eq!(destination_index_for_clip_slot(false, 0, 2, 2), Some(2));
    }

    #[test]
    fn converted_same_track_slot_produces_the_expected_authoritative_order() {
        let (mut project, track_id, clip_ids) = expanded_track_project();
        let destination = destination_index_for_clip_slot(true, 0, 3, 3).unwrap();
        project
            .attach_clip_to_track_at(track_id, clip_ids[0], Some(destination))
            .unwrap();

        assert_eq!(
            project.get_track(track_id).unwrap().clip_ids,
            vec![clip_ids[1], clip_ids[2], clip_ids[0]]
        );
    }

    #[test]
    fn left_edge_trim_keeps_the_content_frame_at_the_new_boundary() {
        let mut clip = Clip::new("trim", 2.0, 6.0);
        clip.trim_in = ordered_float::OrderedFloat(1.5);
        clip.time_stretch = ordered_float::OrderedFloat(1.75);
        let delta = 0.8;
        let expected_local_time_at_new_boundary = clip.local_time(2.0 + delta);

        let timing = timing_after_left_edge_drag(&clip, delta).unwrap();
        assert!((timing.start_time - 2.8).abs() < 1e-9);
        assert!((timing.duration - 5.2).abs() < 1e-9);
        assert!((timing.trim_in - expected_local_time_at_new_boundary).abs() < 1e-9);

        clip.start_time = ordered_float::OrderedFloat(timing.start_time);
        clip.duration = ordered_float::OrderedFloat(timing.duration);
        clip.trim_in = ordered_float::OrderedFloat(timing.trim_in);
        assert!(
            (clip.local_time(timing.start_time) - expected_local_time_at_new_boundary).abs() < 1e-9
        );
    }

    #[test]
    fn left_edge_trim_rejects_negative_source_or_empty_duration() {
        let mut clip = Clip::new("trim", 2.0, 1.0);
        clip.trim_in = ordered_float::OrderedFloat(0.25);
        clip.time_stretch = ordered_float::OrderedFloat(1.0);
        assert!(timing_after_left_edge_drag(&clip, 1.0).is_none());
        assert!(timing_after_left_edge_drag(&clip, -0.5).is_none());
    }

    #[test]
    fn body_drag_applies_frame_delta_without_changing_source_timing() {
        let mut clip = Clip::new("move", 2.0, 6.0);
        clip.trim_in = ordered_float::OrderedFloat(1.5);
        let timing = timing_after_body_drag(&clip, 0.75).unwrap();

        assert_eq!(timing.start_time, 2.75);
        assert_eq!(timing.duration, 6.0);
        assert_eq!(timing.trim_in, 1.5);
        assert!(timing_after_body_drag(&clip, 0.0).is_none());

        clip.start_time = ordered_float::OrderedFloat(0.25);
        let clamped = timing_after_body_drag(&clip, -1.0).unwrap();
        assert_eq!(clamped.start_time, 0.0);
    }
}
