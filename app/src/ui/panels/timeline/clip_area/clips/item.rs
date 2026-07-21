use egui::{epaint::StrokeKind, Ui};
use egui_phosphor::regular as icons;
use library::model::project::Project;
use library::model::{Clip, Track};
use library::EditorService as ProjectService;

use crate::{
    state::{context::EditorContext, context_types::SelectionTarget},
    ui::layer_order::reverse_index,
};

use super::{
    begin_resize_gesture, clip_graph_nodes, clip_insertion_markers,
    destination_index_for_clip_slot, finish_resize_gesture, get_clip_color,
    mark_resize_timing_changed, nearest_clip_insertion_slot, semantic_source_kind,
    semantic_source_label, timeline_drag_delta, timing_after_body_drag,
    timing_after_left_edge_drag, ClipAreaGeometry, ClipReorderProjection, DeferredClipAction,
    DisplayRow, EDGE_DRAG_WIDTH,
};

pub(super) struct SingleClipDrawContext<'a> {
    pub(super) ui_content: &'a mut Ui,
    pub(super) editor_context: &'a mut EditorContext,
    pub(super) deferred_actions: &'a mut Vec<DeferredClipAction>,
    pub(super) project_service: &'a ProjectService,
    pub(super) project: &'a Project,
    pub(super) geometry: ClipAreaGeometry,
    pub(super) display_rows: &'a [DisplayRow<'a>],
    pub(super) reorder_projection: Option<&'a ClipReorderProjection>,
}

pub(super) fn draw_single_clip(
    context: &mut SingleClipDrawContext<'_>,
    clip: &Clip,
    track: &Track,
    row_index: usize,
    is_summary_clip: bool,
) -> bool {
    let SingleClipDrawContext {
        ui_content,
        editor_context,
        deferred_actions,
        project_service,
        project,
        geometry,
        display_rows,
        reorder_projection,
    } = context;
    let graph_nodes = clip_graph_nodes(clip, project);
    // Result and semantic source are separate: explicit Style/Effect/Merge
    // results retain the color, label, and audio identity of their reachable
    // direct source.
    let (r, g, b) = get_clip_color(graph_nodes.semantic_source, project);
    let clip_color = egui::Color32::from_rgb(r, g, b);

    let visual_row_index = reorder_projection
        .and_then(|projection| projection.row_for_clip(clip.id))
        .unwrap_or(row_index);

    let initial_clip_rect = geometry.clip_rect(*clip.start_time, *clip.duration, visual_row_index);
    let safe_width = initial_clip_rect.width();

    // Visibility Culling
    if !geometry.content_rect.intersects(initial_clip_rect) {
        return false;
    }

    if !is_summary_clip {
        let canonical_index = track
            .clip_ids
            .iter()
            .position(|candidate| *candidate == clip.id);
        let visual_index =
            canonical_index.and_then(|index| reverse_index(index, track.clip_ids.len()));
        crate::qa::register_component_with_metadata(
            format!("timeline.clip:{}", clip.id),
            "timeline_clip",
            initial_clip_rect,
            true,
            Some(serde_json::json!({
                "clip_id": clip.id,
                "track_id": track.id,
                "canonical_index": canonical_index,
                "visual_index": visual_index,
                "display_row_index": visual_row_index,
                "canonical_order_semantics": "back_to_front",
                "visual_order_semantics": "front_to_back",
                "start_time": clip.start_time.into_inner(),
                "duration": clip.duration.into_inner(),
                "pixels_per_second": geometry.pixels_per_unit,
                "output_node_id": graph_nodes.output.map(|node| node.id),
                "semantic_source_node_id": graph_nodes.semantic_source.map(|node| node.id),
                "semantic_source_kind": graph_nodes.semantic_source.map(semantic_source_kind),
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
            if response.clicked() && editor_context.active_composition_id.is_some() {
                deferred_actions.push(DeferredClipAction::RemoveClip {
                    track_id: track.id,
                    clip_id: clip.id,
                });
                ui.ctx().request_repaint();
                ui.close();
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
    if let (Some(left), Some(right)) = (&left_edge_resp, &right_edge_resp) {
        if left.drag_started() || right.drag_started() {
            begin_resize_gesture(editor_context);
            editor_context.select_target(SelectionTarget::Clip(clip.id));
        }
    }

    if editor_context.interaction.is_resizing_entity
        && editor_context.selection.primary() == Some(SelectionTarget::Clip(clip.id))
        && !is_summary_clip
    {
        if let (Some(left), Some(right)) = (&left_edge_resp, &right_edge_resp) {
            let mut new_start_time = clip.start_time.into_inner();
            let mut new_duration = clip.duration.into_inner();
            let mut new_trim_in = clip.trim_in.into_inner();

            let delta_x = if left.dragged() {
                timeline_drag_delta(left).x
            } else if right.dragged() {
                timeline_drag_delta(right).x
            } else {
                0.0
            };

            // Convert to time
            let delta_time = delta_x / geometry.pixels_per_unit;

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

            if (new_start_time != clip.start_time.into_inner()
                || new_duration != clip.duration.into_inner()
                || new_trim_in != clip.trim_in.into_inner())
                && editor_context.active_composition_id.is_some()
            {
                deferred_actions.push(DeferredClipAction::UpdateClipTiming {
                    clip_id: clip.id,
                    new_start_time,
                    new_duration,
                    new_trim_in,
                });
                mark_resize_timing_changed(editor_context);
            }
        }
    }

    if let (Some(left), Some(right)) = (&left_edge_resp, &right_edge_resp) {
        let should_commit_resize =
            (left.drag_stopped() || right.drag_stopped()) && finish_resize_gesture(editor_context);
        if should_commit_resize {
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
        if !editor_context.is_selected(SelectionTarget::Clip(clip.id)) {
            editor_context.select_target(SelectionTarget::Clip(clip.id));
        }
        editor_context.interaction.is_moving_selected_entity = true;
        editor_context.interaction.dragged_entity_original_track_id = Some(track.id);
        editor_context.interaction.dragged_entity_hovered_track_id = Some(track.id);
        editor_context.interaction.dragged_entity_has_moved = false;
    }

    if editor_context.interaction.is_moving_selected_entity
        && editor_context.selection.primary() == Some(SelectionTarget::Clip(clip.id))
        && clip_resp.dragged()
        && !edge_is_dragging
    {
        if let Some(pointer) = clip_resp.interact_pointer_pos() {
            let row = ((pointer.y - geometry.content_rect.min.y
                + editor_context.timeline.scroll_offset.y)
                / (geometry.row_height + geometry.row_spacing))
                .floor()
                .max(0.0) as usize;
            if let Some(target_row) = display_rows.get(row) {
                editor_context.interaction.dragged_entity_hovered_track_id =
                    Some(target_row.track_id());
            }
        }
        let delta_time =
            f64::from(timeline_drag_delta(&clip_resp).x) / f64::from(geometry.pixels_per_unit);
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
        && editor_context.selection.primary() == Some(SelectionTarget::Clip(clip.id))
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
                project,
                geometry.row_layout(),
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

        if let Some(composition_id) = editor_context.active_composition_id {
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

    if editor_context.is_selected(SelectionTarget::Clip(clip.id))
        && clip_resp.dragged()
        && !is_summary_clip
    {
        display_x += clip_resp.drag_delta().x;
    }

    let drawing_clip_rect = egui::Rect::from_min_size(
        egui::pos2(display_x, display_y),
        egui::vec2(safe_width, geometry.row_height),
    );

    // --- Drawing ---
    let is_sel_entity = editor_context.is_selected(SelectionTarget::Clip(clip.id));
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

    let painter = ui_content.painter_at(geometry.content_rect);
    painter.rect_filled(drawing_clip_rect, 4.0, transparent_color);

    super::super::waveform::draw_clip_waveform(super::super::waveform::WaveformDrawContext {
        ctx: ui_content.ctx(),
        painter: &painter,
        clip_rect: drawing_clip_rect,
        viewport_rect: geometry.content_rect,
        pixels_per_second: geometry.pixels_per_unit,
        clip,
        project,
        project_service,
    });

    if is_sel_entity {
        painter.rect_stroke(
            drawing_clip_rect,
            4.0,
            egui::Stroke::new(2.0, egui::Color32::WHITE),
            StrokeKind::Middle,
        );
    }

    let mut clip_text = graph_nodes
        .semantic_source
        .map(semantic_source_label)
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

    let edge_hovered = left_edge_resp
        .as_ref()
        .zip(right_edge_resp.as_ref())
        .is_some_and(|(left, right)| left.hovered() || right.hovered());
    if !is_summary_clip && edge_hovered {
        ui_content
            .ctx()
            .set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }

    if !editor_context.interaction.is_resizing_entity && clip_resp.clicked() {
        let action = crate::ui::selection::get_click_action(
            &ui_content.input(|i| i.modifiers),
            Some(clip.id),
        );

        match action {
            crate::ui::selection::ClickAction::Select(id) => {
                editor_context.select_target(SelectionTarget::Clip(id));
            }
            crate::ui::selection::ClickAction::Add(id)
                if !editor_context.is_selected(SelectionTarget::Clip(id)) =>
            {
                editor_context.toggle_selection(SelectionTarget::Clip(id));
            }
            crate::ui::selection::ClickAction::Remove(id)
                if editor_context.is_selected(SelectionTarget::Clip(id)) =>
            {
                editor_context.toggle_selection(SelectionTarget::Clip(id));
            }
            crate::ui::selection::ClickAction::Toggle(id) => {
                editor_context.toggle_selection(SelectionTarget::Clip(id));
            }
            _ => {}
        }
        return true;
    }

    false
}
