use egui::Ui;
use library::model::project::clip::TrackClip;
use library::model::project::node::Node;
use library::model::project::project::Project;
use library::model::project::track::TrackData;
use library::EditorService as ProjectService;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::{action::HistoryManager, state::context::EditorContext};

use super::super::geometry::TimelineGeometry;
use super::super::utils::flatten::{flatten_tracks_to_rows, DisplayRow};
use super::clip_interaction::{draw_single_clip, DeferredClipAction};

pub(super) fn calculate_clip_rect(
    in_frame: u64,
    out_frame: u64,
    track_index: usize,
    scroll_offset: egui::Vec2,
    geo: &TimelineGeometry,
    base_offset: egui::Vec2,
) -> egui::Rect {
    let timeline_duration = out_frame.saturating_sub(in_frame);
    let initial_x = base_offset.x
        + (in_frame as f32 / geo.composition_fps as f32) * geo.pixels_per_unit
        - scroll_offset.x;
    let initial_y =
        base_offset.y - scroll_offset.y + track_index as f32 * (geo.row_height + geo.track_spacing);

    let width = (timeline_duration as f32 / geo.composition_fps as f32) * geo.pixels_per_unit;
    let safe_width = width.max(1.0);

    egui::Rect::from_min_size(
        egui::pos2(initial_x, initial_y),
        egui::vec2(safe_width, geo.row_height),
    )
}

pub(super) fn draw_waveform(
    painter: &egui::Painter,
    clip_rect: egui::Rect,
    audio_data: &[f32],
    source_begin_frame: i64,
    composition_fps: f64,
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
        let source_time = (source_begin_frame as f64 / composition_fps) + _time_offset as f64;
        let start_sample_idx = if source_time >= 0.0 {
            (source_time * sample_rate) as usize * channels
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

// Helper to collect all clips from a track and its descendants using Project node lookup
pub(super) fn collect_descendant_clips<'a>(
    project: &'a Project,
    track: &'a TrackData,
    clips: &mut Vec<&'a TrackClip>,
) {
    for child_id in &track.child_ids {
        match project.get_node(*child_id) {
            Some(Node::Clip(clip)) => clips.push(clip),
            Some(Node::Track(sub_track)) => collect_descendant_clips(project, sub_track, clips),
            _ => {}
        }
    }
}

pub(in crate::ui::panels::timeline) fn calculate_insert_index(
    mouse_y: f32,
    content_rect_min_y: f32,
    scroll_offset_y: f32,
    geo: &TimelineGeometry,
    display_rows: &[DisplayRow],
    project: &Project,
    _root_track_ids: &[Uuid],
    hovered_track_id: Uuid,
) -> Option<(usize, usize)> {
    // Returns (target_index, header_row_index)

    // Find header row for hovered track
    if let Some((header_idx, _)) = display_rows.iter().enumerate().find(|(_, r)| {
        r.track_id() == hovered_track_id && matches!(r, DisplayRow::TrackHeader { .. })
    }) {
        let current_y_in_clip_area = mouse_y - content_rect_min_y + scroll_offset_y;

        let hovered_row_index =
            (current_y_in_clip_area / (geo.row_height + geo.track_spacing)).floor() as isize;
        let header_row_index = header_idx as isize;

        let raw_target_index = hovered_row_index - header_row_index - 1;

        // Clamp to valid range
        if let Some(track) = project.get_track(hovered_track_id) {
            // Count clips in this track
            let clip_count = track
                .child_ids
                .iter()
                .filter(|id| matches!(project.get_node(**id), Some(Node::Clip(_))))
                .count();

            // Invert index because display order is reversed (Top of UI = End of List)
            let max_index = clip_count as isize;
            let inverted_target = max_index - raw_target_index;
            let target_index = inverted_target.clamp(0, max_index) as usize;

            return Some((target_index, header_idx));
        }
    }
    None
}

pub(super) fn draw_clips(
    ui_content: &mut Ui,
    content_rect_for_clip_area: egui::Rect,
    editor_context: &mut EditorContext,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    project: &Arc<RwLock<Project>>,
    root_track_ids: &[Uuid],
    geo: &TimelineGeometry,
) -> bool {
    let row_height = geo.row_height;
    let track_spacing = geo.track_spacing;
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
            root_track_ids,
            &editor_context.timeline.expanded_tracks,
        );

        // Calculate Reorder State if dragging
        let mut reorder_state = None;
        if let (Some(dragged_id), Some(hovered_tid)) = (
            editor_context.selection.last_selected_entity_id,
            editor_context
                .interaction
                .timeline
                .dragged_entity_hovered_track_id,
        ) {
            if let Some(mouse_pos) = ui_content.ctx().pointer_latest_pos() {
                if let Some((target_index, header_idx)) = calculate_insert_index(
                    mouse_pos.y,
                    content_rect_for_clip_area.min.y,
                    editor_context.timeline.scroll_offset.y,
                    geo,
                    &display_rows,
                    &proj_read,
                    root_track_ids,
                    hovered_tid,
                ) {
                    // Find dragged clip original info
                    let mut dragged_original_index = 0;
                    if let Some(track) = proj_read.get_track(hovered_tid) {
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
                        let mut clips_to_draw: Vec<&TrackClip> = Vec::new();
                        collect_descendant_clips(&proj_read, track, &mut clips_to_draw);

                        // Check if clip is a direct child of this track
                        let direct_clip_ids: std::collections::HashSet<Uuid> = track
                            .child_ids
                            .iter()
                            .filter(|id| matches!(proj_read.get_node(**id), Some(Node::Clip(_))))
                            .copied()
                            .collect();

                        for clip in clips_to_draw {
                            let is_summary_clip = !direct_clip_ids.contains(&clip.id);

                            draw_single_clip(
                                ui_content,
                                content_rect_for_clip_area,
                                editor_context,
                                &mut deferred_actions,
                                project_service,
                                &proj_read,
                                root_track_ids,
                                clip,
                                track,
                                *visible_row_index,
                                geo,
                                is_summary_clip,
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
                        root_track_ids,
                        clip,
                        parent_track,
                        *visible_row_index,
                        geo,
                        false,
                        &mut clicked_on_entity,
                        &display_rows,
                        &reorder_state,
                    );
                }
            }
        }

        // Draw asset drag preview indicator
        if let Some(ref _dragged_item) = editor_context.interaction.timeline.dragged_item {
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
            DeferredClipAction::UpdateClipTime {
                clip_id,
                new_in_frame,
                new_out_frame,
            } => {
                if let Err(e) =
                    project_service.update_clip_time(clip_id, new_in_frame, new_out_frame)
                {
                    log::error!("Failed to update clip time: {:?}", e);
                }
            }
            DeferredClipAction::MoveClipToTrack {
                comp_id,
                original_track_id,
                clip_id,
                target_track_id,
                in_frame,
                target_index,
            } => {
                // Read the clip's CURRENT in_frame, which may have been updated
                // by a preceding UpdateClipTime action in the same frame.
                // Using the stale `in_frame` from the READ phase would overwrite
                // the horizontal drag applied by UpdateClipTime.
                let current_in_frame = project_service
                    .with_project(|p| p.get_clip(clip_id).map(|c| c.in_frame))
                    .unwrap_or(in_frame);

                if let Err(e) = project_service.move_clip_to_track_at_index(
                    comp_id,
                    original_track_id,
                    clip_id,
                    target_track_id,
                    current_in_frame,
                    target_index,
                ) {
                    log::error!("Failed to move entity: {:?}", e);
                    // Rollback selection state: clip is still on the original track
                    if editor_context.selection.last_selected_track_id == Some(target_track_id) {
                        editor_context.selection.last_selected_track_id = Some(original_track_id);
                    }
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

pub(super) fn get_clips_in_box(
    rect: egui::Rect,
    editor_context: &EditorContext,
    project: &Project,
    root_track_ids: &[Uuid],
    geo: &TimelineGeometry,
    rect_offset: egui::Vec2,
) -> Vec<(Uuid, Uuid)> {
    let mut found_clips = Vec::new();
    let display_rows = flatten_tracks_to_rows(
        project,
        root_track_ids,
        &editor_context.timeline.expanded_tracks,
    );

    for row in display_rows {
        let mut clips_to_check: Vec<(&TrackClip, &TrackData, usize)> = Vec::new();

        match row {
            DisplayRow::TrackHeader {
                track,
                visible_row_index,
                is_expanded,
                ..
            } => {
                if !is_expanded {
                    let mut clips = Vec::new();
                    collect_descendant_clips(project, track, &mut clips);
                    for clip in clips {
                        clips_to_check.push((clip, track, visible_row_index));
                    }
                }
            }
            DisplayRow::ClipRow {
                clip,
                parent_track,
                visible_row_index,
                ..
            } => {
                clips_to_check.push((clip, parent_track, visible_row_index));
            }
        }

        for (clip, track, row_idx) in clips_to_check {
            let clip_rect = calculate_clip_rect(
                clip.in_frame,
                clip.out_frame,
                row_idx,
                editor_context.timeline.scroll_offset,
                geo,
                rect_offset,
            );

            if rect.intersects(clip_rect) {
                found_clips.push((clip.id, track.id));
            }
        }
    }
    found_clips
}

#[cfg(test)]
mod tests {
    use super::super::super::geometry::TimelineGeometry;
    use super::*;

    fn default_geo() -> TimelineGeometry {
        TimelineGeometry {
            pixels_per_unit: 100.0, // 100 pixels per second
            row_height: 30.0,
            track_spacing: 2.0,
            composition_fps: 30.0,
        }
    }

    // ── Domain: calculate_clip_rect ──

    #[test]
    fn clip_rect_basic_position() {
        let geo = default_geo();
        let rect = calculate_clip_rect(
            0,                // in_frame
            30,               // out_frame (1 second at 30fps)
            0,                // track_index (first row)
            egui::Vec2::ZERO, // scroll_offset
            &geo,
            egui::Vec2::ZERO, // base_offset
        );
        // x = 0/30 * 100 - 0 = 0
        // y = 0 * (30+2) = 0
        // width = 30/30 * 100 = 100
        assert!((rect.min.x - 0.0).abs() < 0.01);
        assert!((rect.min.y - 0.0).abs() < 0.01);
        assert!((rect.width() - 100.0).abs() < 0.01);
        assert!((rect.height() - 30.0).abs() < 0.01);
    }

    #[test]
    fn clip_rect_with_offset_frames() {
        let geo = default_geo();
        let rect = calculate_clip_rect(
            30, // in_frame (1 sec)
            60, // out_frame (2 sec)
            0,
            egui::Vec2::ZERO,
            &geo,
            egui::Vec2::ZERO,
        );
        // x = 30/30 * 100 = 100
        // width = 30/30 * 100 = 100
        assert!((rect.min.x - 100.0).abs() < 0.01);
        assert!((rect.width() - 100.0).abs() < 0.01);
    }

    #[test]
    fn clip_rect_on_second_row() {
        let geo = default_geo();
        let rect = calculate_clip_rect(
            0,
            30,
            1, // second row
            egui::Vec2::ZERO,
            &geo,
            egui::Vec2::ZERO,
        );
        // y = 1 * (30 + 2) = 32
        assert!((rect.min.y - 32.0).abs() < 0.01);
    }

    #[test]
    fn clip_rect_with_scroll() {
        let geo = default_geo();
        let rect = calculate_clip_rect(
            0,
            30,
            0,
            egui::vec2(50.0, 10.0), // scrolled
            &geo,
            egui::Vec2::ZERO,
        );
        // x = 0 - 50 = -50
        // y = 0 - 10 = -10
        assert!((rect.min.x - (-50.0)).abs() < 0.01);
        assert!((rect.min.y - (-10.0)).abs() < 0.01);
    }

    #[test]
    fn clip_rect_with_base_offset() {
        let geo = default_geo();
        let rect = calculate_clip_rect(
            0,
            30,
            0,
            egui::Vec2::ZERO,
            &geo,
            egui::vec2(100.0, 50.0), // base offset
        );
        // x = 100 + 0 = 100
        // y = 50 + 0 = 50
        assert!((rect.min.x - 100.0).abs() < 0.01);
        assert!((rect.min.y - 50.0).abs() < 0.01);
    }

    #[test]
    fn clip_rect_zero_duration_has_min_width() {
        let geo = default_geo();
        let rect = calculate_clip_rect(
            10,
            10, // zero duration!
            0,
            egui::Vec2::ZERO,
            &geo,
            egui::Vec2::ZERO,
        );
        // width = max(0, 1.0) = 1.0 (minimum width)
        assert!((rect.width() - 1.0).abs() < 0.01);
    }

    // ── Domain: calculate_insert_index ──

    #[test]
    fn insert_index_with_empty_track() {
        use super::super::super::utils::flatten::DisplayRow;
        use library::model::project::node::Node;
        use library::model::project::track::TrackData;

        let mut project = Project::new("test");
        let track_id = Uuid::new_v4();
        let mut track = TrackData::new("Track");
        track.id = track_id;
        project.nodes.insert(track_id, Node::Track(track.clone()));

        let display_rows = vec![DisplayRow::TrackHeader {
            track: project.get_track(track_id).unwrap(),
            depth: 0,
            is_expanded: true,
            visible_row_index: 0,
        }];

        let geo = default_geo();
        // Mouse at y = header row area → row index 0
        let result = calculate_insert_index(
            5.0, // mouse_y (within first row)
            0.0, // content_rect_min_y
            0.0, // scroll_offset_y
            &geo,
            &display_rows,
            &project,
            &[track_id],
            track_id,
        );
        // Empty track: clip_count=0, raw_target_index = 0-0-1 = -1
        // inverted = 0 - (-1) = 1, clamped to [0, 0] = 0
        assert!(result.is_some());
        let (index, header_idx) = result.unwrap();
        assert_eq!(header_idx, 0);
        assert_eq!(index, 0);
    }
}
