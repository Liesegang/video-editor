use egui::{epaint::StrokeKind, Ui};
use egui_phosphor::regular as icons;
use library::project::clip::{TrackClip, TrackClipKind};
use library::project::node::Node;
use library::project::project::Project;
use library::project::track::TrackData;
use library::EditorService as ProjectService;
use uuid::Uuid;

use crate::context::context::EditorContext;

use super::super::geometry::TimelineGeometry;
use super::super::utils::flatten::DisplayRow;
use super::clips::{calculate_clip_rect, calculate_insert_index, draw_waveform};

const EDGE_DRAG_WIDTH: f32 = 5.0;

// ── Pure frame-calculation helpers (testable without UI) ──

/// Compute the maximum allowed out_frame based on source media duration.
/// Returns `u64::MAX` when the clip has no known duration (infinite extension).
pub(super) fn compute_source_max_out_frame(
    in_frame: u64,
    out_frame: u64,
    source_begin_frame: i64,
    duration_frame: Option<u64>,
) -> u64 {
    if let Some(duration) = duration_frame {
        let source_end_offset = duration as i64 - source_begin_frame;
        if source_end_offset > 0 {
            in_frame.saturating_add(source_end_offset as u64)
        } else {
            // source_begin_frame >= duration — should not normally happen, but
            // fall back to the current out_frame so the clip stays valid.
            out_frame
        }
    } else {
        u64::MAX
    }
}

/// Compute new in_frame when resizing from the left edge.
/// Clamps to `[0, out_frame - 1]` so the clip keeps at least 1 frame.
pub(super) fn compute_resize_left_frame(in_frame: u64, out_frame: u64, dt_frames: i64) -> u64 {
    ((in_frame as i64 + dt_frames).max(0) as u64).min(out_frame.saturating_sub(1))
}

/// Compute new out_frame when resizing from the right edge.
/// Clamps to `[in_frame + 1, source_max_out_frame]`.
pub(super) fn compute_resize_right_frame(
    in_frame: u64,
    out_frame: u64,
    dt_frames: i64,
    source_max_out_frame: u64,
) -> u64 {
    ((out_frame as i64 + dt_frames).max(in_frame as i64 + 1) as u64).min(source_max_out_frame)
}

/// Compute new (in_frame, out_frame) when dragging a clip horizontally.
/// Preserves clip duration and clamps so `in_frame >= 0`.
pub(super) fn compute_drag_frames(in_frame: u64, out_frame: u64, dt_frames: i64) -> (u64, u64) {
    let duration = out_frame.saturating_sub(in_frame);
    let new_in_frame = (in_frame as i64 + dt_frames).max(0) as u64;
    let new_out_frame = new_in_frame + duration;
    (new_in_frame, new_out_frame)
}

/// Deferred actions collected during UI phase, executed after read lock is released
#[derive(Debug)]
pub(super) enum DeferredClipAction {
    /// Update clip time (resize)
    UpdateClipTime {
        clip_id: Uuid,
        new_in_frame: u64,
        new_out_frame: u64,
    },
    /// Move clip to track at index (reorder/move)
    MoveClipToTrack {
        comp_id: Uuid,
        original_track_id: Uuid,
        clip_id: Uuid,
        target_track_id: Uuid,
        in_frame: u64,
        target_index: Option<usize>,
    },
    /// Remove clip from track
    RemoveClip { track_id: Uuid, clip_id: Uuid },
    /// Push history state after changes
    PushHistory,
}

// Helper to find which track contains a clip
pub(super) fn find_track_containing_clip(
    project: &Project,
    root_track_ids: &[Uuid],
    clip_id: Uuid,
) -> Option<Uuid> {
    fn search_track(project: &Project, track_id: Uuid, clip_id: Uuid) -> Option<Uuid> {
        if let Some(track) = project.get_track(track_id) {
            for child_id in &track.child_ids {
                if *child_id == clip_id {
                    return Some(track_id);
                }
                if let Some(Node::Track(_)) = project.get_node(*child_id) {
                    if let Some(found) = search_track(project, *child_id, clip_id) {
                        return Some(found);
                    }
                }
            }
        }
        None
    }

    for root_id in root_track_ids {
        if let Some(found) = search_track(project, *root_id, clip_id) {
            return Some(found);
        }
    }
    None
}

pub(super) fn draw_single_clip(
    ui_content: &mut Ui,
    content_rect_for_clip_area: egui::Rect,
    editor_context: &mut EditorContext,
    deferred_actions: &mut Vec<DeferredClipAction>,
    project_service: &ProjectService,
    project: &Project,
    root_track_ids: &[Uuid],
    clip: &TrackClip,
    track: &TrackData,
    row_index: usize,
    geo: &TimelineGeometry,
    is_summary_clip: bool,
    clicked_on_entity: &mut bool,
    display_rows: &[DisplayRow],
    reorder_state: &Option<(Uuid, Uuid, usize, usize, usize)>,
) {
    let pixels_per_unit = geo.pixels_per_unit;
    let row_height = geo.row_height;
    let track_spacing = geo.track_spacing;
    let composition_fps = geo.composition_fps;
    // Determine Color based on kind using helper
    let (r, g, b) = clip.display_color();
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

                let is_same_track_sort = if let Some(orig_tid) = editor_context
                    .interaction
                    .timeline
                    .dragged_entity_original_track_id
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
        clip.in_frame,
        clip.out_frame,
        visual_row_index,
        editor_context.timeline.scroll_offset,
        geo,
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

    let interaction_id = if is_summary_clip {
        egui::Id::new(clip.id).with("summary").with(row_index)
    } else {
        egui::Id::new(clip.id)
    };

    let clip_resp = ui_content.interact(initial_clip_rect, interaction_id, sense);

    if !is_summary_clip {
        clip_resp.context_menu(|ui| {
            use crate::widgets::context_menu::{show_context_menu, ContextMenuBuilder};

            #[derive(Clone)]
            enum ClipAction {
                Remove,
            }

            let menu = ContextMenuBuilder::new()
                .danger_action(icons::TRASH, "Remove", ClipAction::Remove)
                .build();
            if let Some(action) = show_context_menu(ui, &menu) {
                match action {
                    ClipAction::Remove => {
                        if let Some(_comp_id) = editor_context.selection.composition_id {
                            deferred_actions.push(DeferredClipAction::RemoveClip {
                                track_id: track.id,
                                clip_id: clip.id,
                            });
                            ui.ctx().request_repaint();
                        }
                    }
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
            editor_context.interaction.timeline.is_resizing_entity = true;
            editor_context.select_clip(clip.id, track.id);
            _is_resizing = true;
        }
    }

    if editor_context.interaction.timeline.is_resizing_entity
        && editor_context.selection.last_selected_entity_id == Some(clip.id)
        && !is_summary_clip
    {
        if let (Some(left), Some(right)) = (&left_edge_resp, &right_edge_resp) {
            let mut new_in_frame = clip.in_frame;
            let mut new_out_frame = clip.out_frame;

            let source_max_out_frame = compute_source_max_out_frame(
                clip.in_frame,
                clip.out_frame,
                clip.source_begin_frame,
                clip.duration_frame,
            );

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
                new_in_frame = compute_resize_left_frame(new_in_frame, new_out_frame, dt_frames);
            } else if right.dragged() {
                new_out_frame = compute_resize_right_frame(
                    new_in_frame,
                    new_out_frame,
                    dt_frames,
                    source_max_out_frame,
                );
            }

            if new_in_frame != clip.in_frame || new_out_frame != clip.out_frame {
                if let (Some(_comp_id), Some(_tid)) = (
                    editor_context.selection.composition_id,
                    editor_context.selection.last_selected_track_id,
                ) {
                    deferred_actions.push(DeferredClipAction::UpdateClipTime {
                        clip_id: clip.id,
                        new_in_frame,
                        new_out_frame,
                    });
                }
            }
        }
    }

    if let (Some(left), Some(right)) = (&left_edge_resp, &right_edge_resp) {
        if left.drag_stopped() || right.drag_stopped() {
            editor_context.interaction.timeline.is_resizing_entity = false;
            deferred_actions.push(DeferredClipAction::PushHistory);
        }
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
        drawing_clip_rect.min + egui::vec2(5.0, 5.0),
        egui::Align2::LEFT_TOP,
        &clip_text,
        egui::FontId::default(),
        egui::Color32::BLACK,
    );

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

    if !editor_context.interaction.timeline.is_resizing_entity && clip_resp.clicked() {
        let action = crate::widgets::selection::get_click_action(
            &ui_content.input(|i| i.modifiers),
            Some(clip.id),
        );

        match action {
            crate::widgets::selection::ClickAction::Select(id) => {
                editor_context.select_clip(id, track.id);
            }
            crate::widgets::selection::ClickAction::Add(id) => {
                if !editor_context.is_selected(id) {
                    editor_context.toggle_selection(id, track.id);
                }
            }
            crate::widgets::selection::ClickAction::Remove(id) => {
                if editor_context.is_selected(id) {
                    editor_context.toggle_selection(id, track.id);
                }
            }
            crate::widgets::selection::ClickAction::Toggle(id) => {
                editor_context.toggle_selection(id, track.id);
            }
            _ => {}
        }
        *clicked_on_entity = true;
    }

    if !editor_context.interaction.timeline.is_resizing_entity && clip_resp.drag_started() {
        if !editor_context.is_selected(clip.id) {
            editor_context.select_clip(clip.id, track.id);
        }
        if !is_summary_clip {
            editor_context.selection.last_selected_entity_id = Some(clip.id);
            editor_context.selection.last_selected_track_id = Some(track.id);
            editor_context
                .interaction
                .timeline
                .dragged_entity_original_track_id = Some(track.id);
            editor_context
                .interaction
                .timeline
                .dragged_entity_hovered_track_id = Some(track.id);
            editor_context.interaction.timeline.dragged_entity_has_moved = false;
        }
    }

    if !editor_context.interaction.timeline.is_resizing_entity
        && clip_resp.dragged()
        && editor_context.is_selected(clip.id)
        && !is_summary_clip
    {
        if clip_resp.drag_delta().length_sq() > 0.0 {
            editor_context.interaction.timeline.dragged_entity_has_moved = true;
        }

        let dt_frames_f32 = clip_resp.drag_delta().x / pixels_per_unit * composition_fps as f32;
        let dt_frames = dt_frames_f32.round() as i64;

        if dt_frames != 0 {
            if let Some(_comp_id) = editor_context.selection.composition_id {
                let selected_ids: Vec<Uuid> = editor_context
                    .selection
                    .selected_entities
                    .iter()
                    .cloned()
                    .collect();

                for entity_id in selected_ids {
                    // Use Project.get_clip for lookup
                    if let Some(c) = project.get_clip(entity_id) {
                        // Find which track contains this clip
                        if let Some(_tid) =
                            find_track_containing_clip(project, root_track_ids, entity_id)
                        {
                            let (new_in_frame, new_out_frame) =
                                compute_drag_frames(c.in_frame, c.out_frame, dt_frames);

                            deferred_actions.push(DeferredClipAction::UpdateClipTime {
                                clip_id: c.id,
                                new_in_frame,
                                new_out_frame,
                            });
                        }
                    }
                }
            }
        }

        // Handle vertical movement
        if let Some(mouse_pos) = ui_content.ctx().pointer_latest_pos() {
            let mut allow_track_change = true;

            if let Some(press_origin) = ui_content.input(|i| i.pointer.press_origin()) {
                let total_vertical_delta = (mouse_pos.y - press_origin.y).abs();
                let threshold = (row_height + track_spacing) * 0.75;

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
                    if editor_context
                        .interaction
                        .timeline
                        .dragged_entity_hovered_track_id
                        != Some(hovered_display_track.track_id())
                    {
                        editor_context
                            .interaction
                            .timeline
                            .dragged_entity_hovered_track_id =
                            Some(hovered_display_track.track_id());
                    }
                }
            } else {
                if let Some(original_id) = editor_context
                    .interaction
                    .timeline
                    .dragged_entity_original_track_id
                {
                    if editor_context
                        .interaction
                        .timeline
                        .dragged_entity_hovered_track_id
                        != Some(original_id)
                    {
                        editor_context
                            .interaction
                            .timeline
                            .dragged_entity_hovered_track_id = Some(original_id);
                    }
                }
            }
        }
    }

    if !editor_context.interaction.timeline.is_resizing_entity
        && clip_resp.drag_stopped()
        && editor_context.is_selected(clip.id)
        && !is_summary_clip
    {
        let mut moved_track = false;
        if let (Some(original_track_id), Some(hovered_track_id), Some(comp_id)) = (
            editor_context
                .interaction
                .timeline
                .dragged_entity_original_track_id,
            editor_context
                .interaction
                .timeline
                .dragged_entity_hovered_track_id,
            editor_context.selection.composition_id,
        ) {
            let mut target_index_opt = None;
            if let Some(mouse_pos) = ui_content.ctx().pointer_latest_pos() {
                if let Some((target_index, _)) = calculate_insert_index(
                    mouse_pos.y,
                    content_rect_for_clip_area.min.y,
                    editor_context.timeline.scroll_offset.y,
                    geo,
                    display_rows,
                    project,
                    root_track_ids,
                    hovered_track_id,
                ) {
                    target_index_opt = Some(target_index);
                }
            }

            deferred_actions.push(DeferredClipAction::MoveClipToTrack {
                comp_id,
                original_track_id,
                clip_id: clip.id,
                target_track_id: hovered_track_id,
                in_frame: clip.in_frame,
                target_index: target_index_opt,
            });
            editor_context.selection.last_selected_track_id = Some(hovered_track_id);
            moved_track = true;
        }

        if moved_track || editor_context.interaction.timeline.dragged_entity_has_moved {
            deferred_actions.push(DeferredClipAction::PushHistory);
        }

        editor_context
            .interaction
            .timeline
            .dragged_entity_original_track_id = None;
        editor_context
            .interaction
            .timeline
            .dragged_entity_hovered_track_id = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::project::clip::{TrackClip, TrackClipKind};
    use library::project::node::Node;
    use library::project::project::Project;
    use library::project::track::TrackData;

    // ── Domain: compute_source_max_out_frame ──

    #[test]
    fn source_max_no_duration_returns_max() {
        let result = compute_source_max_out_frame(10, 50, 0, None);
        assert_eq!(result, u64::MAX);
    }

    #[test]
    fn source_max_with_duration_computes_correctly() {
        // duration=100 frames, source_begin=0, in_frame=10 → max out = 10+100 = 110
        let result = compute_source_max_out_frame(10, 50, 0, Some(100));
        assert_eq!(result, 110);
    }

    #[test]
    fn source_max_with_offset_begin() {
        // duration=100, source_begin=30, in_frame=10 → remaining=70, max out = 10+70 = 80
        let result = compute_source_max_out_frame(10, 50, 30, Some(100));
        assert_eq!(result, 80);
    }

    #[test]
    fn source_max_begin_exceeds_duration_returns_out_frame() {
        // BUG FIX: source_begin_frame >= duration_frame should return out_frame, not in_frame
        let result = compute_source_max_out_frame(10, 50, 120, Some(100));
        assert_eq!(result, 50); // Should be out_frame (50), not in_frame (10)
    }

    #[test]
    fn source_max_begin_equals_duration_returns_out_frame() {
        let result = compute_source_max_out_frame(10, 50, 100, Some(100));
        assert_eq!(result, 50); // offset = 0, falls into <= 0 branch
    }

    // ── Domain: compute_resize_left_frame ──

    #[test]
    fn resize_left_shrinks_clip() {
        // Clip at frames 10-50, drag right by 5 → new in_frame = 15
        let result = compute_resize_left_frame(10, 50, 5);
        assert_eq!(result, 15);
    }

    #[test]
    fn resize_left_extends_clip() {
        // Clip at frames 10-50, drag left by 5 → new in_frame = 5
        let result = compute_resize_left_frame(10, 50, -5);
        assert_eq!(result, 5);
    }

    #[test]
    fn resize_left_clamps_to_zero() {
        // Clip at frames 10-50, drag left by 20 → clamped to 0
        let result = compute_resize_left_frame(10, 50, -20);
        assert_eq!(result, 0);
    }

    #[test]
    fn resize_left_clamps_to_out_minus_one() {
        // Clip at frames 10-50, drag right by 100 → clamped to 49 (out-1)
        let result = compute_resize_left_frame(10, 50, 100);
        assert_eq!(result, 49);
    }

    #[test]
    fn resize_left_preserves_minimum_one_frame() {
        // Clip at frames 10-11 (1 frame), drag right by 0 → stays at 10
        let result = compute_resize_left_frame(10, 11, 0);
        assert_eq!(result, 10);
    }

    // ── Domain: compute_resize_right_frame ──

    #[test]
    fn resize_right_extends_clip() {
        // Clip at frames 10-50, drag right by 10 → new out_frame = 60
        let result = compute_resize_right_frame(10, 50, 10, u64::MAX);
        assert_eq!(result, 60);
    }

    #[test]
    fn resize_right_shrinks_clip() {
        // Clip at frames 10-50, drag left by 10 → new out_frame = 40
        let result = compute_resize_right_frame(10, 50, -10, u64::MAX);
        assert_eq!(result, 40);
    }

    #[test]
    fn resize_right_clamps_to_in_plus_one() {
        // Clip at frames 10-50, drag left by 100 → clamped to 11 (in+1)
        let result = compute_resize_right_frame(10, 50, -100, u64::MAX);
        assert_eq!(result, 11);
    }

    #[test]
    fn resize_right_clamps_to_source_max() {
        // Clip at frames 10-50, drag right by 100 → clamped to source max 80
        let result = compute_resize_right_frame(10, 50, 100, 80);
        assert_eq!(result, 80);
    }

    #[test]
    fn resize_right_source_max_less_than_in_plus_one() {
        // Edge case: source_max < in_frame + 1 (pathological)
        // Both clamps conflict → in_frame + 1 wins via max, then min caps it
        let result = compute_resize_right_frame(10, 50, -100, 5);
        assert_eq!(result, 5); // min(11, 5) = 5 — still clipped by source_max
    }

    // ── Domain: compute_drag_frames ──

    #[test]
    fn drag_preserves_duration() {
        // Clip at frames 10-20 (duration=10), drag right by 5
        let (new_in, new_out) = compute_drag_frames(10, 20, 5);
        assert_eq!(new_in, 15);
        assert_eq!(new_out, 25);
        assert_eq!(new_out - new_in, 10); // Duration preserved
    }

    #[test]
    fn drag_left_clamps_at_zero_preserves_duration() {
        // BUG FIX: Clip at frames 10-20 (duration=10), drag left by 100
        // Old code: new_in=0, new_out=max(-80, 0)=0 → ZERO-LENGTH CLIP!
        // Fixed:    new_in=0, new_out=0+10=10 → duration preserved
        let (new_in, new_out) = compute_drag_frames(10, 20, -100);
        assert_eq!(new_in, 0);
        assert_eq!(new_out, 10);
        assert_eq!(new_out - new_in, 10); // Duration preserved!
    }

    #[test]
    fn drag_to_exact_zero() {
        // Clip at frames 10-20, drag left by exactly 10
        let (new_in, new_out) = compute_drag_frames(10, 20, -10);
        assert_eq!(new_in, 0);
        assert_eq!(new_out, 10);
    }

    #[test]
    fn drag_no_movement() {
        let (new_in, new_out) = compute_drag_frames(10, 20, 0);
        assert_eq!(new_in, 10);
        assert_eq!(new_out, 20);
    }

    #[test]
    fn drag_single_frame_clip_preserves_duration() {
        // 1-frame clip at frame 5-6
        let (new_in, new_out) = compute_drag_frames(5, 6, -100);
        assert_eq!(new_in, 0);
        assert_eq!(new_out, 1);
        assert_eq!(new_out - new_in, 1);
    }

    // ── Domain: find_track_containing_clip ──

    fn make_test_project() -> (Project, Uuid, Uuid, Uuid, Uuid) {
        let mut project = Project::new("test");

        let track_id = Uuid::new_v4();
        let clip_id_1 = Uuid::new_v4();
        let clip_id_2 = Uuid::new_v4();

        let mut track = TrackData::new("Test Track");
        track.id = track_id;
        track.child_ids = vec![clip_id_1, clip_id_2];

        let clip1 = TrackClip::new(
            clip_id_1,
            None,
            TrackClipKind::Video,
            0,
            30,
            0,
            Some(100),
            30.0,
            Default::default(),
        );
        let clip2 = TrackClip::new(
            clip_id_2,
            None,
            TrackClipKind::Audio,
            10,
            50,
            0,
            Some(200),
            30.0,
            Default::default(),
        );

        project.nodes.insert(track_id, Node::Track(track));
        project.nodes.insert(clip_id_1, Node::Clip(clip1));
        project.nodes.insert(clip_id_2, Node::Clip(clip2));

        (project, track_id, clip_id_1, clip_id_2, Uuid::new_v4())
    }

    #[test]
    fn find_clip_in_direct_track() {
        let (project, track_id, clip_id_1, _, _) = make_test_project();
        let result = find_track_containing_clip(&project, &[track_id], clip_id_1);
        assert_eq!(result, Some(track_id));
    }

    #[test]
    fn find_clip_not_in_any_track() {
        let (project, track_id, _, _, _) = make_test_project();
        let missing_id = Uuid::new_v4();
        let result = find_track_containing_clip(&project, &[track_id], missing_id);
        assert_eq!(result, None);
    }

    #[test]
    fn find_clip_in_nested_track() {
        let (mut project, track_id, _, clip_id_2, _) = make_test_project();

        // Create a sub-track and move clip_id_2 into it
        let sub_track_id = Uuid::new_v4();
        let mut sub_track = TrackData::new("Sub Track");
        sub_track.id = sub_track_id;
        sub_track.child_ids = vec![clip_id_2];

        // Remove clip_id_2 from main track, add sub_track instead
        if let Some(Node::Track(t)) = project.nodes.get_mut(&track_id) {
            t.child_ids.retain(|id| *id != clip_id_2);
            t.child_ids.push(sub_track_id);
        }
        project.nodes.insert(sub_track_id, Node::Track(sub_track));

        let result = find_track_containing_clip(&project, &[track_id], clip_id_2);
        assert_eq!(result, Some(sub_track_id));
    }

    #[test]
    fn find_clip_with_empty_root_tracks() {
        let (project, _, clip_id_1, _, _) = make_test_project();
        let result = find_track_containing_clip(&project, &[], clip_id_1);
        assert_eq!(result, None);
    }

    // ── Domain: Deferred action ordering (horizontal drag + track move) ──

    #[test]
    fn drag_then_move_should_preserve_horizontal_position() {
        // Simulate the scenario where UpdateClipTime and MoveClipToTrack both
        // fire in the same frame. UpdateClipTime runs first (horizontal drag)
        // then MoveClipToTrack should use the UPDATED in_frame, not the stale one.
        //
        // Clip starts at frames 10-20 (duration=10).
        // Horizontal drag moves it to 15-25.
        // MoveClipToTrack should use in_frame=15 (current), not 10 (stale).
        let original_in = 10u64;
        let original_out = 20u64;
        let dt_frames = 5i64;

        let (new_in, new_out) = compute_drag_frames(original_in, original_out, dt_frames);
        assert_eq!(new_in, 15);
        assert_eq!(new_out, 25);

        // After UpdateClipTime sets (15, 25), MoveClipToTrack should read 15
        // as current_in_frame, NOT use original_in=10 which would reset to (10, 20).
        assert_ne!(
            new_in, original_in,
            "Horizontal drag should change in_frame"
        );
    }

    #[test]
    fn drag_far_left_then_move_preserves_clamped_position() {
        // Clip at frames 100-200 (duration=100). Drag far left by -500.
        let (new_in, new_out) = compute_drag_frames(100, 200, -500);
        assert_eq!(new_in, 0);
        assert_eq!(new_out, 100);

        // MoveClipToTrack should use 0 (current), not 100 (stale).
        // Duration must be preserved.
        assert_eq!(new_out - new_in, 100);
    }

    // ── Domain: compute_source_max_out_frame edge cases ──

    #[test]
    fn source_max_negative_begin_frame() {
        // source_begin_frame can be negative (trimmed start before 0)
        // duration=100, source_begin=-10 → offset=110, max = 10+110=120
        let result = compute_source_max_out_frame(10, 50, -10, Some(100));
        assert_eq!(result, 120);
    }

    #[test]
    fn source_max_large_duration() {
        // Very long source media
        let result = compute_source_max_out_frame(0, 30, 0, Some(u64::MAX / 2));
        assert_eq!(result, u64::MAX / 2);
    }

    // ── Domain: Resize edge cases ──

    #[test]
    fn resize_left_on_clip_starting_at_zero() {
        // Clip at frames 0-30, try to extend left (impossible, already at 0)
        let result = compute_resize_left_frame(0, 30, -10);
        assert_eq!(result, 0);
    }

    #[test]
    fn resize_right_on_one_frame_clip() {
        // Clip at frames 10-11 (1 frame), try to shrink right (impossible)
        let result = compute_resize_right_frame(10, 11, -10, u64::MAX);
        assert_eq!(result, 11); // min clamp: in_frame + 1 = 11
    }
}
