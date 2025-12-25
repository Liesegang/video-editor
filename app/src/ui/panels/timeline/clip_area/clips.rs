use egui::{epaint::StrokeKind, Ui};
use egui_phosphor::regular as icons;
use library::model::project::project::Project;
use library::model::project::{Track, TrackClip, TrackClipKind, TrackItem};
use library::EditorService as ProjectService;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::{action::HistoryManager, state::context::EditorContext};

use super::super::utils::flatten::{flatten_tracks_to_rows, DisplayRow};

const EDGE_DRAG_WIDTH: f32 = 5.0;

#[allow(clippy::too_many_arguments)]
fn calculate_clip_rect(
    in_frame: u64,
    out_frame: u64,
    track_index: usize,
    scroll_offset: egui::Vec2,
    pixels_per_unit: f32,
    row_height: f32,
    track_spacing: f32,
    composition_fps: f64,
    base_offset: egui::Vec2, // e.g. content_rect.min or similar
) -> egui::Rect {
    let timeline_duration = out_frame.saturating_sub(in_frame);
    let initial_x = base_offset.x + (in_frame as f32 / composition_fps as f32) * pixels_per_unit
        - scroll_offset.x;
    let initial_y =
        base_offset.y - scroll_offset.y + track_index as f32 * (row_height + track_spacing);

    let width = (timeline_duration as f32 / composition_fps as f32) * pixels_per_unit;
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
    source_begin_frame: i64, // Changed to i64
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
            audio_data.len() + 1 // Invalid index
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

// Helper to collect all clips from a track and its descendants
fn collect_descendant_clips<'a>(track: &'a Track, clips: &mut Vec<&'a TrackClip>) {
    clips.extend(track.clips());
    for child_item in &track.children {
        if let TrackItem::SubTrack(child) = child_item {
            collect_descendant_clips(child, clips);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn calculate_insert_index(
    mouse_y: f32,
    content_rect_min_y: f32,
    scroll_offset_y: f32,
    row_height: f32,
    track_spacing: f32,
    display_rows: &[DisplayRow],
    current_tracks: &[library::model::project::Track],
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

        // Target index relative to track (0-indexed clips)
        // Row structure: Header (0), Clip 0 (1), Clip 1 (2)...
        // So clip_index = row_diff - 1
        let raw_target_index = hovered_row_index - header_row_index - 1;

        // Clamp to valid range
        if let Some(track) = current_tracks.iter().find(|t| t.id == hovered_track_id) {
            let max_index = track.clips().count();
            let target_index = raw_target_index.clamp(0, max_index as isize) as usize;
            return Some((target_index, header_idx));
        }
    }
    None
}

#[allow(clippy::too_many_arguments)]
pub fn draw_clips(
    ui_content: &mut Ui,
    content_rect_for_clip_area: egui::Rect,
    editor_context: &mut EditorContext,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    current_tracks: &[library::model::project::Track],

    _project: &Arc<RwLock<Project>>,
    pixels_per_unit: f32,
    row_height: f32,
    track_spacing: f32,
    composition_fps: f64,
) -> bool {
    let mut clicked_on_entity = false;

    // Flatten tracks for display using new DisplayRow system - MOVED TO TOP
    let display_rows =
        flatten_tracks_to_rows(current_tracks, &editor_context.timeline.expanded_tracks);

    // Calcluate Reorder State if dragging
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
                current_tracks,
                hovered_tid,
            ) {
                // Find dragged clip original info
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
                    collect_descendant_clips(track, &mut clips_to_draw);

                    for clip in clips_to_draw {
                        let is_summary_clip = !track.clips().any(|c| c.id == clip.id);

                        draw_single_clip(
                            ui_content,
                            content_rect_for_clip_area,
                            editor_context,
                            project_service,
                            history_manager,
                            current_tracks,
                            clip,
                            track,
                            *visible_row_index,
                            pixels_per_unit,
                            row_height,
                            track_spacing,
                            composition_fps,
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
                    project_service,
                    history_manager,
                    current_tracks,
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

    clicked_on_entity
}

#[allow(clippy::too_many_arguments)]
fn draw_single_clip(
    ui_content: &mut Ui,
    content_rect_for_clip_area: egui::Rect,
    editor_context: &mut EditorContext,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    current_tracks: &[Track],
    clip: &TrackClip,
    track: &Track,
    row_index: usize,
    pixels_per_unit: f32,
    row_height: f32,
    track_spacing: f32,
    composition_fps: f64,
    is_summary_clip: bool,
    clicked_on_entity: &mut bool,
    display_rows: &[DisplayRow],
    reorder_state: &Option<(Uuid, Uuid, usize, usize, usize)>, // dragged_id, track_id, orig_idx, target_idx, header_row_idx
) {
    // Determine Color based on kind using helper
    let (r, g, b) = clip.display_color();
    let clip_color = egui::Color32::from_rgb(r, g, b);

    // Apply Live Reordering Visual Shift
    let mut visual_row_index = row_index;

    // Check if we are in a reordering state
    if let Some((dragged_id, r_track_id, src_idx, dst_idx, header_row_idx)) = reorder_state {
        // CASE 1: This is the dragged clip (snap to target)
        if clip.id == *dragged_id {
            // Snap to target row (works for both same track and cross-track)
            visual_row_index = header_row_idx + 1 + dst_idx;
        }
        // CASE 2: Other clips in the TARGET track (shift to make gap)
        else if track.id == *r_track_id {
            // Get original child index from DisplayRow if available
            let mut original_child_index = None;
            if let Some(DisplayRow::ClipRow { child_index, .. }) = display_rows.get(row_index) {
                original_child_index = Some(*child_index);
            }

            if let Some(idx) = original_child_index {
                // Shift logic
                let mut new_child_index = idx;
                let src = *src_idx;
                let dst = *dst_idx;

                // Only shift if we are in the same track as original?
                // If dragging from A to B:
                // In B (target track): src is effectively "undefined" or "infinity" (new item coming in).
                // Actually, reorder_state.src_idx is only valid if dragged_id was originally in r_track_id.

                // Checking if dragged item WAS in this track
                // We need to know if we are doing Intra-Track (Sort) or Inter-Track (Insert).
                // reorder_state has `dragged_original_index`. This index is strictly valid for the track where it CAME from.

                // If r_track_id == dragged_entity_original_track_id: then Sort.
                // Else: Insert.

                // We can check if `current_tracks` (model) has the clip in this track to know if it's local.
                // Optimization: Check editor_context.

                let is_same_track_sort = if let Some(orig_tid) =
                    editor_context.interaction.dragged_entity_original_track_id
                {
                    orig_tid == *r_track_id
                } else {
                    false
                };

                if is_same_track_sort {
                    // Intra-Track Sort Logic (existing logic)
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
                    // Inter-Track Insert Logic
                    // Dragging FROM another track TO here.
                    // Simply open a gap at dst.
                    // Everything >= dst shifts down.
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
        clip.in_frame,
        clip.out_frame,
        visual_row_index, // Use the adjusted visual row index
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

    // --- Interaction for clips ---

    let sense = if is_summary_clip {
        egui::Sense::click()
    } else {
        egui::Sense::click_and_drag()
    };

    // Note: Use a combined ID for summary clips to avoid conflicts if same clip shown multiple times
    let interaction_id = if is_summary_clip {
        egui::Id::new(clip.id).with("summary").with(row_index)
    } else {
        egui::Id::new(clip.id)
    };

    let clip_resp = ui_content.interact(initial_clip_rect, interaction_id, sense);

    if !is_summary_clip {
        clip_resp.context_menu(|ui| {
            if ui.button(format!("{} Remove", icons::TRASH)).clicked() {
                if let Some(comp_id) = editor_context.selection.composition_id {
                    if let Err(e) =
                        project_service.remove_clip_from_track(comp_id, track.id, clip.id)
                    {
                        log::error!("Failed to remove entity: {:?}", e);
                    } else {
                        editor_context.selection.selected_entities.remove(&clip.id);
                        if editor_context.selection.last_selected_entity_id == Some(clip.id) {
                            editor_context.selection.last_selected_entity_id = None;
                            editor_context.selection.last_selected_track_id = None;
                        }
                        let current_state = project_service.get_project().read().unwrap().clone();
                        history_manager.push_project_state(current_state);
                        ui.ctx().request_repaint();
                        ui.close();
                    }
                }
            }
        });
    }

    // Edges (Resize) - Disable for summary clips for now
    let mut left_edge_resp = None;
    let mut right_edge_resp = None;

    if !is_summary_clip {
        // Create edge responses
        let left_edge_rect = egui::Rect::from_min_size(
            egui::pos2(initial_clip_rect.min.x, initial_clip_rect.min.y),
            egui::vec2(EDGE_DRAG_WIDTH, initial_clip_rect.height()),
        );
        left_edge_resp = Some(ui_content.interact(
            left_edge_rect,
            egui::Id::new(clip.id).with("left_edge"),
            egui::Sense::drag(),
        ));

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
            let mut new_in_frame = clip.in_frame;
            let mut new_out_frame = clip.out_frame;

            // Source constraints
            let source_max_out_frame = if let Some(duration) = clip.duration_frame {
                let source_end_offset = duration as i64 - clip.source_begin_frame;
                if source_end_offset > 0 {
                    clip.in_frame.saturating_add(source_end_offset as u64)
                } else {
                    clip.in_frame
                }
            } else {
                u64::MAX
            };

            let delta_x = if left.dragged() {
                left.drag_delta().x
            } else if right.dragged() {
                right.drag_delta().x
            } else {
                0.0
            };

            let dt_frames_f32 = delta_x / pixels_per_unit * composition_fps as f32;
            let dt_frames = dt_frames_f32.round() as i64;

            if left.dragged() {
                new_in_frame = ((new_in_frame as i64 + dt_frames).max(0) as u64)
                    .min(new_out_frame.saturating_sub(1)); // Minimum 1 frame duration
            } else if right.dragged() {
                new_out_frame = ((new_out_frame as i64 + dt_frames).max(new_in_frame as i64 + 1)
                    as u64) // Minimum 1 frame duration
                    .min(source_max_out_frame); // Cannot go beyond source duration
            }

            // Update if there's an actual change
            if new_in_frame != clip.in_frame || new_out_frame != clip.out_frame {
                if let (Some(comp_id), Some(tid)) = (
                    editor_context.selection.composition_id,
                    editor_context.selection.last_selected_track_id,
                ) {
                    project_service
                        .update_clip_time(comp_id, tid, clip.id, new_in_frame, new_out_frame)
                        .ok();
                }
            }
        }
    }

    if let (Some(left), Some(right)) = (&left_edge_resp, &right_edge_resp) {
        if left.drag_stopped() || right.drag_stopped() {
            editor_context.interaction.is_resizing_entity = false;
            let current_state = project_service.get_project().read().unwrap().clone();
            history_manager.push_project_state(current_state);
        }
    }

    // Calculate display position (potentially adjusted for drag preview)
    let mut display_x = initial_clip_rect.min.x;
    let display_y = initial_clip_rect.min.y;

    // Adjust position for dragged entity preview
    if editor_context.is_selected(clip.id) && clip_resp.dragged() && !is_summary_clip {
        display_x += clip_resp.drag_delta().x;

        // Remove legacy display_y calculation logic as it's handled by reorder_state now.
        // We only adjust X here for time-shift visualization.
    }

    let drawing_clip_rect = egui::Rect::from_min_size(
        egui::pos2(display_x, display_y),
        egui::vec2(safe_width, row_height),
    );

    // --- Drawing for clips (always) ---
    let is_sel_entity = editor_context.is_selected(clip.id);
    let mut transparent_color =
        egui::Color32::from_rgba_premultiplied(clip_color.r(), clip_color.g(), clip_color.b(), 150);

    if is_summary_clip {
        // Dim summary clips and maybe make them more transparent
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
    if (clip.kind == TrackClipKind::Audio || clip.kind == TrackClipKind::Video) && safe_width > 10.0
    {
        if let Some(asset_id) = clip.reference_id {
            let cache = project_service.get_cache_manager();
            if let Some(audio_data) = cache.get_audio(asset_id) {
                let sample_rate = project_service
                    .get_audio_service()
                    .get_audio_engine()
                    .get_sample_rate() as f64;
                let channels = project_service
                    .get_audio_service()
                    .get_audio_engine()
                    .get_channels() as usize;
                draw_waveform(
                    &painter,
                    drawing_clip_rect,
                    &audio_data,
                    clip.source_begin_frame,
                    composition_fps,
                    pixels_per_unit,
                    sample_rate,
                    channels,
                );
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

    // Text clipping
    let mut clip_text = clip.kind.to_string();
    if is_summary_clip {
        clip_text = format!("(Ref) {}", clip_text);
    }

    painter.text(
        drawing_clip_rect.min + egui::vec2(5.0, 5.0), // Top left align
        egui::Align2::LEFT_TOP,
        &clip_text,
        egui::FontId::default(),
        egui::Color32::BLACK,
    );
    // --- End Drawing for clips ---

    // Cursor feedback
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

    if !editor_context.interaction.is_resizing_entity && clip_resp.drag_started() {
        if !editor_context.is_selected(clip.id) {
            editor_context.select_clip(clip.id, track.id);
        }
        if !is_summary_clip {
            editor_context.selection.last_selected_entity_id = Some(clip.id);
            editor_context.selection.last_selected_track_id = Some(track.id);
            editor_context.interaction.dragged_entity_original_track_id = Some(track.id);
            editor_context.interaction.dragged_entity_hovered_track_id = Some(track.id);
            editor_context.interaction.dragged_entity_has_moved = false;
        }
    }
    if !editor_context.interaction.is_resizing_entity
        && clip_resp.dragged()
        && editor_context.is_selected(clip.id)
        && !is_summary_clip
    {
        if clip_resp.drag_delta().length_sq() > 0.0 {
            editor_context.interaction.dragged_entity_has_moved = true;
        }

        let dt_frames_f32 = clip_resp.drag_delta().x / pixels_per_unit * composition_fps as f32;
        let dt_frames = dt_frames_f32.round() as i64;

        if dt_frames != 0 {
            if let Some(comp_id) = editor_context.selection.composition_id {
                let selected_ids: Vec<Uuid> = editor_context
                    .selection
                    .selected_entities
                    .iter()
                    .cloned()
                    .collect();

                for entity_id in selected_ids {
                    // Optimized recursive helper
                    fn find_clip_recursive_single(
                        track: &Track,
                        clip_id: Uuid,
                    ) -> Option<(TrackClip, Uuid)> {
                        if let Some(c) = track.clips().find(|c| c.id == clip_id) {
                            return Some((c.clone(), track.id));
                        }
                        for item in &track.children {
                            if let TrackItem::SubTrack(sub) = item {
                                if let Some(res) = find_clip_recursive_single(sub, clip_id) {
                                    return Some(res);
                                }
                            }
                        }
                        None
                    }

                    fn find_clip_recursive(
                        tracks: &[Track],
                        clip_id: Uuid,
                    ) -> Option<(TrackClip, Uuid)> {
                        for t in tracks {
                            if let Some(res) = find_clip_recursive_single(t, clip_id) {
                                return Some(res);
                            }
                        }
                        None
                    }

                    if let Some((c, tid)) = find_clip_recursive(current_tracks, entity_id) {
                        let new_in_frame = (c.in_frame as i64 + dt_frames).max(0) as u64;
                        let new_out_frame =
                            (c.out_frame as i64 + dt_frames).max(new_in_frame as i64) as u64;

                        project_service
                            .update_clip_time(comp_id, tid, c.id, new_in_frame, new_out_frame)
                            .ok();
                    }
                }
            }
        }

        // Handle vertical movement (track change detection) with stickiness
        if let Some(mouse_pos) = ui_content.ctx().pointer_latest_pos() {
            let mut allow_track_change = true;

            // Calculate total vertical drag distance to implement stickiness
            // We only allow changing tracks if the user implies vertical movement
            if let Some(press_origin) = ui_content.input(|i| i.pointer.press_origin()) {
                let total_vertical_delta = (mouse_pos.y - press_origin.y).abs();
                let threshold = (row_height + track_spacing) * 0.75; // Require significant vertical movement

                if total_vertical_delta < threshold {
                    allow_track_change = false;
                }
            }

            if allow_track_change {
                let current_y_in_clip_area = mouse_pos.y - content_rect_for_clip_area.min.y
                    + editor_context.timeline.scroll_offset.y;

                let hovered_track_index =
                    (current_y_in_clip_area / (row_height + track_spacing)).floor() as usize;

                if let Some(hovered_display_track) = display_rows.get(hovered_track_index) {
                    if editor_context.interaction.dragged_entity_hovered_track_id
                        != Some(hovered_display_track.track_id())
                    {
                        editor_context.interaction.dragged_entity_hovered_track_id =
                            Some(hovered_display_track.track_id());
                    }
                }
            } else {
                // Stick to original track
                if let Some(original_id) =
                    editor_context.interaction.dragged_entity_original_track_id
                {
                    if editor_context.interaction.dragged_entity_hovered_track_id
                        != Some(original_id)
                    {
                        editor_context.interaction.dragged_entity_hovered_track_id =
                            Some(original_id);
                    }
                }
            }
        }
    }
    if !editor_context.interaction.is_resizing_entity
        && clip_resp.drag_stopped()
        && editor_context.is_selected(clip.id)
        && !is_summary_clip
    {
        let mut moved_track = false;
        if let (Some(original_track_id), Some(hovered_track_id), Some(comp_id)) = (
            editor_context.interaction.dragged_entity_original_track_id,
            editor_context.interaction.dragged_entity_hovered_track_id,
            editor_context.selection.composition_id,
        ) {
            // Allow moving within same track (Reordering)
            // Calculate target index
            let mut target_index_opt = None;
            if let Some(mouse_pos) = ui_content.ctx().pointer_latest_pos() {
                if let Some((target_index, _)) = calculate_insert_index(
                    mouse_pos.y,
                    content_rect_for_clip_area.min.y,
                    editor_context.timeline.scroll_offset.y,
                    row_height,
                    track_spacing,
                    display_rows,
                    current_tracks,
                    hovered_track_id,
                ) {
                    target_index_opt = Some(target_index);
                }
            }

            // Move entity
            if let Err(e) = project_service.move_clip_to_track_at_index(
                comp_id,
                original_track_id,
                clip.id,
                hovered_track_id,
                clip.in_frame,
                target_index_opt,
            ) {
                log::error!("Failed to move entity: {:?}", e);
            } else {
                editor_context.selection.last_selected_track_id = Some(hovered_track_id);
                moved_track = true;
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

#[allow(clippy::too_many_arguments)]
pub fn get_clips_in_box(
    rect: egui::Rect,
    editor_context: &EditorContext, // Access scroll_offset
    current_tracks: &[library::model::project::Track],
    pixels_per_unit: f32,
    row_height: f32,
    track_spacing: f32,
    composition_fps: f64,
    rect_offset: egui::Vec2,
) -> Vec<(Uuid, Uuid)> {
    // Returns (EntityId, TrackId)
    let mut found_clips = Vec::new();
    let display_rows =
        flatten_tracks_to_rows(current_tracks, &editor_context.timeline.expanded_tracks);

    for row in display_rows {
        // Collect clips to check based on row type
        let mut clips_to_check: Vec<(&TrackClip, &Track, usize)> = Vec::new(); // clip, track, visible_row_index

        match row {
            DisplayRow::TrackHeader {
                track,
                visible_row_index,
                is_expanded,
                ..
            } => {
                // If collapsed, check clips on this row
                if !is_expanded {
                    let mut clips = Vec::new();
                    collect_descendant_clips(track, &mut clips);
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
                pixels_per_unit,
                row_height,
                track_spacing,
                composition_fps,
                rect_offset,
            );

            if rect.intersects(clip_rect) {
                found_clips.push((clip.id, track.id));
            }
        }
    }
    found_clips
}
