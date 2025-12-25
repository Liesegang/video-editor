use egui::{epaint::StrokeKind, Ui};
use egui_phosphor::regular as icons;
use library::model::project::project::Project;
use library::model::project::{Track, TrackClip, TrackClipKind};
use library::EditorService as ProjectService;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::{action::HistoryManager, state::context::EditorContext};

use super::super::utils::flatten::flatten_tracks;

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
    clips.extend(&track.clips);
    for child in &track.children {
        collect_descendant_clips(child, clips);
    }
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

    // Flatten tracks for display
    let display_tracks = flatten_tracks(current_tracks, &editor_context.timeline.expanded_tracks);

    // Map to find track from flattened index if needed later, or just iterate display_tracks
    for display_track in &display_tracks {
        let i = display_track.visible_row_index;
        let track = display_track.track;

        let mut clips_to_draw: Vec<&TrackClip> = Vec::new();

        if display_track.is_folder && !display_track.is_expanded {
            // Collapsed folder: Show summary of all descendant clips
            collect_descendant_clips(track, &mut clips_to_draw);
        } else {
            // Normal track or expanded folder (only show own clips, children handled in their own rows)
            for clip in &track.clips {
                clips_to_draw.push(clip);
            }
        }

        for clip in clips_to_draw {
            // Determine Color based on kind using helper
            let (r, g, b) = clip.display_color();
            let clip_color = egui::Color32::from_rgb(r, g, b);

            let initial_clip_rect = calculate_clip_rect(
                clip.in_frame,
                clip.out_frame,
                i, // Use the visual row index
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
                continue;
            }

            // Check if this is a summary clip (from a collapsed folder)
            // A clip is a summary clip if it doesn't belong directly to the current display_track's track
            let is_summary_clip = !track.clips.iter().any(|c| c.id == clip.id);

            // --- Interaction for clips ---

            // If it's a summary clip, we might want to disable some interactions or make it read-only/selection-only
            // For now, let's allow selection but maybe visualize differently.

            let sense = if is_summary_clip {
                egui::Sense::click() // Maybe just click to select? Or disable drag?
            } else {
                egui::Sense::click_and_drag()
            };

            // Define clip_resp using the initial_clip_rect for hit detection
            // Note: Use a combined ID for summary clips to avoid conflicts if same clip shown multiple times (unlikely here but good practice)
            let interaction_id = if is_summary_clip {
                egui::Id::new(clip.id).with("summary").with(i)
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
                                if editor_context.selection.last_selected_entity_id == Some(clip.id)
                                {
                                    editor_context.selection.last_selected_entity_id = None;
                                    editor_context.selection.last_selected_track_id = None;
                                }
                                let current_state =
                                    project_service.get_project().read().unwrap().clone();
                                history_manager.push_project_state(current_state);
                                ui.ctx().request_repaint();
                                ui.close();
                            }
                        }
                    }
                });
            }

            // Edges (Resize) - Disable for summary clips for now to keep it simple
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
                        // clip.source_begin_frame is i64. duration is u64.
                        // We want: max_out = in + (duration - source_begin).
                        // But source_begin can be negative.
                        // If source_begin is -100, duration 1000. available = 1100?
                        // "Duration" usually means total length of file (0..duration).
                        // If source_begin is -100, we play "void" for 100 frames then source 0.
                        // So effective end of source is at source_time = duration.
                        // Timeline end = In + (duration - source_begin).
                        // Careful with signs.
                        let source_end_offset = duration as i64 - clip.source_begin_frame;
                        if source_end_offset > 0 {
                            clip.in_frame.saturating_add(source_end_offset as u64)
                        } else {
                            clip.in_frame // Should not happen if duration > source_begin
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
                            // .max(clip.source_begin_frame) // Constraint removed for negative source start
                            .min(new_out_frame.saturating_sub(1)); // Minimum 1 frame duration
                    } else if right.dragged() {
                        new_out_frame = ((new_out_frame as i64 + dt_frames)
                            .max(new_in_frame as i64 + 1)
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
                                .update_clip_time(
                                    comp_id,
                                    tid,
                                    clip.id,
                                    new_in_frame,
                                    new_out_frame,
                                )
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
            let mut display_y = initial_clip_rect.min.y;

            // Adjust position for dragged entity preview
            if editor_context.is_selected(clip.id) && clip_resp.dragged() && !is_summary_clip {
                display_x += clip_resp.drag_delta().x;

                if let Some(hovered_track_id) =
                    editor_context.interaction.dragged_entity_hovered_track_id
                {
                    // Find visible index of hovered track in flattened list
                    if let Some(hovered_display_track) = display_tracks
                        .iter()
                        .find(|t| t.track.id == hovered_track_id)
                    {
                        display_y = content_rect_for_clip_area.min.y
                            + editor_context.timeline.scroll_offset.y
                            + hovered_display_track.visible_row_index as f32
                                * (row_height + track_spacing);
                    }
                }
            }

            let drawing_clip_rect = egui::Rect::from_min_size(
                egui::pos2(display_x, display_y),
                egui::vec2(safe_width, row_height),
            );

            // --- Drawing for clips (always) ---
            let is_sel_entity = editor_context.is_selected(clip.id);
            let mut transparent_color = egui::Color32::from_rgba_premultiplied(
                clip_color.r(),
                clip_color.g(),
                clip_color.b(),
                150,
            );

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
            if (clip.kind == TrackClipKind::Audio || clip.kind == TrackClipKind::Video)
                && safe_width > 10.0
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
                            .get_channels() as usize; // Stereo is standard
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
                &clip_text, // Use Display impl
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

                // If summary, maybe always just select?
                // For now, treat same as normal.

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
                clicked_on_entity = true;
            }

            if !editor_context.interaction.is_resizing_entity && clip_resp.drag_started() {
                if !editor_context.is_selected(clip.id) {
                    editor_context.select_clip(clip.id, track.id);
                }
                if !is_summary_clip {
                    // Update primary selection for drag logic usually, but keep multi-selection
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

                let dt_frames_f32 =
                    clip_resp.drag_delta().x / pixels_per_unit * composition_fps as f32;
                let dt_frames = dt_frames_f32.round() as i64;

                if dt_frames != 0 {
                    if let Some(comp_id) = editor_context.selection.composition_id {
                        // Iterate all selected entities to apply delta
                        let selected_ids: Vec<Uuid> = editor_context
                            .selection
                            .selected_entities
                            .iter()
                            .cloned()
                            .collect();

                        for entity_id in selected_ids {
                            // Find the clip data in current_tracks to get current time
                            let mut clip_data = None;
                            let mut track_id_found = None;

                            // Efficiency: we iterate tracks every time.
                            // Since number of tracks/clips is usually small, this is okay.
                            // Map lookup would be faster but requires building map every frame?
                            // REFACTOR: Use flatten traversal or recursive search to find clip owner

                            fn find_clip_recursive(
                                tracks: &[Track],
                                clip_id: Uuid,
                            ) -> Option<(TrackClip, Uuid)> {
                                for t in tracks {
                                    if let Some(c) = t.clips.iter().find(|c| c.id == clip_id) {
                                        return Some((c.clone(), t.id));
                                    }
                                    if let Some(res) = find_clip_recursive(&t.children, clip_id) {
                                        return Some(res);
                                    }
                                }
                                None
                            }

                            if let Some((c, tid)) = find_clip_recursive(current_tracks, entity_id) {
                                clip_data = Some(c);
                                track_id_found = Some(tid);
                            }

                            if let (Some(c), Some(tid)) = (clip_data, track_id_found) {
                                let new_in_frame = (c.in_frame as i64 + dt_frames).max(0) as u64;
                                let new_out_frame = (c.out_frame as i64 + dt_frames)
                                    .max(new_in_frame as i64)
                                    as u64;

                                project_service
                                    .update_clip_time(
                                        comp_id,
                                        tid,
                                        c.id,
                                        new_in_frame,
                                        new_out_frame,
                                    )
                                    .ok();

                                // Removed update_clip_source_frames to ensure keyframes (and content) follow the clip (Relative Keyframing).
                                /*
                                let new_source_begin_frame =
                                    (c.source_begin_frame as i64 + dt_frames).max(0) as u64;
                                project_service
                                    .update_clip_source_frames(
                                        comp_id,
                                        tid,
                                        c.id,
                                        new_source_begin_frame,
                                    )
                                    .ok();
                                */
                            }
                        }
                    }
                }

                // Handle vertical movement (track change detection)
                if let Some(mouse_pos) = ui_content.ctx().pointer_latest_pos() {
                    let current_y_in_clip_area = mouse_pos.y - content_rect_for_clip_area.min.y
                        + editor_context.timeline.scroll_offset.y;

                    let hovered_track_index =
                        (current_y_in_clip_area / (row_height + track_spacing)).floor() as usize;

                    if let Some(hovered_display_track) = display_tracks.get(hovered_track_index) {
                        if editor_context.interaction.dragged_entity_hovered_track_id
                            != Some(hovered_display_track.track.id)
                        {
                            editor_context.interaction.dragged_entity_hovered_track_id =
                                Some(hovered_display_track.track.id);
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
                    if original_track_id != hovered_track_id {
                        // Move entity to new track
                        if let Err(e) = project_service.move_clip_to_track(
                            comp_id,
                            original_track_id,
                            clip.id,
                            hovered_track_id,
                            clip.in_frame,
                        ) {
                            log::error!("Failed to move entity to new track: {:?}", e);
                        } else {
                            editor_context.selection.last_selected_track_id =
                                Some(hovered_track_id);
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
    let display_tracks = flatten_tracks(current_tracks, &editor_context.timeline.expanded_tracks);

    for display_track in display_tracks {
        let i = display_track.visible_row_index;
        let track = display_track.track;

        let mut clips_to_check: Vec<&TrackClip> = Vec::new();
        // Only check visible clips (own clips) for selection box usually?
        // Or if it's summary, do we select child clips?
        // For simplicity, selection box only selects directly visible clips on their own tracks.
        // If a folder is collapsed, its summary clips are technically visible, but selecting them might be confusing if they are actually deep in hierarchy.
        // Let's assume we only select clips if the track is actually showing them as *its* content, or if expanded.
        // If collapsed, we probably shouldn't select inner clips via box selection on the summary track unless we want to support that.
        // Let's stick to: only select if track is expanded or it's a leaf track.
        // Actually, if track is collapsed, we probably don't want to select its children via box select on the folder line.

        if !display_track.is_folder || display_track.is_expanded {
            for clip in &track.clips {
                clips_to_check.push(clip);
            }
        }

        for clip in clips_to_check {
            let clip_rect = calculate_clip_rect(
                clip.in_frame,
                clip.out_frame,
                i,
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
