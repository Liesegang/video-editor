use egui::{epaint::StrokeKind, Ui};
use egui_phosphor::regular as icons;
use library::model::project::clip::{TrackClip, TrackClipKind};
use library::model::project::node::Node;
use library::model::project::project::Project;
use library::model::project::track::TrackData;
use library::EditorService as ProjectService;
use uuid::Uuid;

use crate::state::context::EditorContext;

use super::super::geometry::TimelineGeometry;
use super::super::utils::flatten::DisplayRow;
use super::clips::{calculate_clip_rect, calculate_insert_index, draw_waveform};

const EDGE_DRAG_WIDTH: f32 = 5.0;

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
            if ui.button(format!("{} Remove", icons::TRASH)).clicked() {
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
                    .min(new_out_frame.saturating_sub(1));
            } else if right.dragged() {
                new_out_frame = ((new_out_frame as i64 + dt_frames).max(new_in_frame as i64 + 1)
                    as u64)
                    .min(source_max_out_frame);
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
                            let new_in_frame = (c.in_frame as i64 + dt_frames).max(0) as u64;
                            let new_out_frame =
                                (c.out_frame as i64 + dt_frames).max(new_in_frame as i64) as u64;

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
