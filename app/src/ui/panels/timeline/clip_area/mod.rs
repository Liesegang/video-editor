use egui::Ui;
use library::model::project::project::Project;
use library::EditorService as ProjectService;
use std::sync::{Arc, RwLock};

use crate::{action::HistoryManager, state::context::EditorContext};

use super::geometry::TimelineGeometry;
use crate::command::{CommandId, CommandRegistry};
use crate::ui::viewport::{ViewportConfig, ViewportController, ViewportState};

mod background;
mod clip_interaction;
pub mod clips;
pub mod context_menu;
pub mod drag_and_drop;
pub mod interactions;

struct TimelineViewportState<'a> {
    scroll_offset: &'a mut egui::Vec2,
    h_zoom: &'a mut f32,
    v_zoom: &'a mut f32,
    min_h_zoom: f32,
    max_h_zoom: f32,
    min_v_zoom: f32,
    max_v_zoom: f32,
    max_scroll_y: f32,
}

impl<'a> ViewportState for TimelineViewportState<'a> {
    fn get_pan(&self) -> egui::Vec2 {
        *self.scroll_offset
    }
    fn set_pan(&mut self, pan: egui::Vec2) {
        let mut new_offset = pan;
        new_offset.x = new_offset.x.max(0.0);
        new_offset.y = new_offset.y.clamp(0.0, self.max_scroll_y);
        *self.scroll_offset = new_offset;
    }
    fn get_zoom(&self) -> egui::Vec2 {
        egui::vec2(*self.h_zoom, *self.v_zoom)
    }
    fn set_zoom(&mut self, zoom: egui::Vec2) {
        *self.h_zoom = zoom.x.clamp(self.min_h_zoom, self.max_h_zoom);
        *self.v_zoom = zoom.y.clamp(self.min_v_zoom, self.max_v_zoom);
    }
}

pub fn show_clip_area(
    ui_content: &mut Ui,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut ProjectService,
    project: &Arc<RwLock<Project>>,
    geo: &TimelineGeometry,
    registry: &CommandRegistry,
) -> (egui::Rect, egui::Response) {
    let row_height = geo.row_height;
    let track_spacing = geo.track_spacing;
    let composition_fps = geo.composition_fps;
    let (content_rect_for_clip_area, response) =
        ui_content.allocate_at_least(ui_content.available_size(), egui::Sense::hover());

    let is_dragging_item = editor_context.interaction.timeline.dragged_item.is_some();
    let selected_composition_id = editor_context.selection.composition_id;

    // ===== PHASE 1: Extract owned data from project (scoped read lock) =====
    let (root_track_ids, num_visible_tracks, current_comp_duration) = {
        let proj_read = match project.read() {
            Ok(p) => p,
            Err(_) => return (content_rect_for_clip_area, response),
        };

        let mut root_ids: Vec<uuid::Uuid> = Vec::new();
        let mut comp_duration = 300.0;

        if let Some(comp_id) = selected_composition_id {
            if let Some(comp) = proj_read.compositions.iter().find(|c| c.id == comp_id) {
                root_ids.push(comp.root_track_id);
                comp_duration = comp.duration;
            }
        }

        // Flatten tracks to get correct visible count
        let display_rows = super::utils::flatten::flatten_tracks_to_rows(
            &proj_read,
            &root_ids,
            &editor_context.timeline.expanded_tracks,
        );
        let visible_count = display_rows.len();

        (root_ids, visible_count, comp_duration)
    }; // proj_read dropped here

    // ===== PHASE 2: Drawing and UI (no project lock held) =====
    let painter = ui_content.painter_at(content_rect_for_clip_area);

    background::draw_track_backgrounds(
        &painter,
        content_rect_for_clip_area,
        num_visible_tracks,
        geo,
        editor_context.timeline.scroll_offset,
        current_comp_duration,
    );

    // --- Viewport Controller for Zoom/Pan ---
    const MAX_PIXELS_PER_FRAME_DESIRED: f32 = 20.0;
    let max_h_zoom = (MAX_PIXELS_PER_FRAME_DESIRED * composition_fps as f32)
        / editor_context.timeline.pixels_per_second;
    let min_possible_zoom = content_rect_for_clip_area.width()
        / (current_comp_duration as f32 * editor_context.timeline.pixels_per_second);
    let min_h_zoom = min_possible_zoom.min(0.01);

    let hand_tool_key = registry
        .commands
        .iter()
        .find(|c| c.id == CommandId::HandTool)
        .and_then(|c| c.shortcut)
        .map(|(_, k)| k);

    let mut state = TimelineViewportState {
        scroll_offset: &mut editor_context.timeline.scroll_offset,
        h_zoom: &mut editor_context.timeline.h_zoom,
        v_zoom: &mut editor_context.timeline.v_zoom,
        min_h_zoom,
        max_h_zoom,
        min_v_zoom: 0.1,
        max_v_zoom: 10.0,
        max_scroll_y: (num_visible_tracks as f32 * (row_height + track_spacing)
            - content_rect_for_clip_area.height())
        .max(0.0),
    };

    let mut controller = ViewportController::new(
        ui_content,
        ui_content.make_persistent_id("unique_timeline_viewport_controller_id"),
        hand_tool_key,
    )
    .with_config(ViewportConfig {
        zoom_uniform: false,
        allow_zoom_x: true,
        allow_zoom_y: true,
        allow_pan_x: true,
        allow_pan_y: true,
        min_zoom: 0.0001,
        max_zoom: 10000.0,
        ..Default::default()
    });

    let (_changed, vp_response) = controller.interact_with_rect(
        content_rect_for_clip_area,
        &mut state,
        &mut editor_context.interaction.preview.handled_hand_tool_drag,
    );

    // Handle Box Selection State Update
    if !editor_context.interaction.preview.handled_hand_tool_drag {
        if vp_response.drag_started_by(egui::PointerButton::Primary) {
            if !ui_content.input(|i| i.modifiers.alt) {
                if let Some(pos) = vp_response.interact_pointer_pos() {
                    editor_context
                        .interaction
                        .timeline
                        .timeline_selection_drag_start = Some(pos);
                }
            }
        }
    }

    // ===== PHASE 3: Drag/drop and context menu (may need write lock) =====
    interactions::handle_drag_drop_and_context_menu(
        ui_content,
        &vp_response,
        content_rect_for_clip_area,
        editor_context,
        project,
        project_service,
        history_manager,
        geo,
        num_visible_tracks,
    );

    // ===== PHASE 4: Draw clips (separate read lock scope) =====
    let clicked_on_entity = clips::draw_clips(
        ui_content,
        content_rect_for_clip_area,
        editor_context,
        project_service,
        history_manager,
        project,
        &root_track_ids,
        geo,
    );

    // ===== PHASE 5: Box Selection Logic (separate read lock scope) =====
    if let Some(start_pos) = editor_context
        .interaction
        .timeline
        .timeline_selection_drag_start
    {
        if ui_content.input(|i| i.pointer.primary_down()) {
            if let Some(current_pos) = ui_content.input(|i| i.pointer.interact_pos()) {
                let selection_rect = egui::Rect::from_two_pos(start_pos, current_pos);

                let painter = ui_content.painter_at(content_rect_for_clip_area);
                painter.rect_stroke(
                    selection_rect,
                    0.0,
                    egui::Stroke::new(1.0, egui::Color32::from_rgb(100, 200, 255)),
                    egui::StrokeKind::Middle,
                );
                painter.rect_filled(
                    selection_rect,
                    0.0,
                    egui::Color32::from_rgba_premultiplied(100, 200, 255, 30),
                );
            }
        } else {
            // Released - commit box selection (needs separate read lock)
            if let Some(current_pos) = ui_content.input(|i| i.pointer.interact_pos()) {
                let selection_rect = egui::Rect::from_two_pos(start_pos, current_pos);

                let found_clips = {
                    if let Ok(proj_read) = project.read() {
                        clips::get_clips_in_box(
                            selection_rect,
                            editor_context,
                            &proj_read,
                            &root_track_ids,
                            geo,
                            content_rect_for_clip_area.min.to_vec2(),
                        )
                    } else {
                        Vec::new()
                    }
                };

                let action = crate::ui::selection::get_box_action(
                    &ui_content.input(|i| i.modifiers),
                    found_clips,
                );

                match action {
                    crate::ui::selection::BoxAction::Replace(items) => {
                        editor_context.selection.selected_entities.clear();
                        editor_context.selection.last_selected_entity_id = None;
                        editor_context.selection.last_selected_track_id = None;

                        let mut last_id = None;
                        let mut last_track = None;
                        for (id, tid) in items {
                            editor_context.selection.selected_entities.insert(id);
                            last_id = Some(id);
                            last_track = Some(tid);
                        }
                        if let Some(lid) = last_id {
                            editor_context.selection.last_selected_entity_id = Some(lid);
                            editor_context.selection.last_selected_track_id = last_track;
                        }
                    }
                    crate::ui::selection::BoxAction::Add(items) => {
                        let mut last_id = None;
                        let mut last_track = None;
                        for (id, tid) in items {
                            editor_context.selection.selected_entities.insert(id);
                            last_id = Some(id);
                            last_track = Some(tid);
                        }
                        if let Some(lid) = last_id {
                            editor_context.selection.last_selected_entity_id = Some(lid);
                            editor_context.selection.last_selected_track_id = last_track;
                        }
                    }
                    crate::ui::selection::BoxAction::Remove(items) => {
                        for (id, _tid) in items {
                            editor_context.selection.selected_entities.remove(&id);
                        }
                    }
                }
            }
            editor_context
                .interaction
                .timeline
                .timeline_selection_drag_start = None;
        }
    }

    // Final selection clearing logic
    if !editor_context.interaction.timeline.is_resizing_entity
        && vp_response.clicked()
        && !clicked_on_entity
        && !is_dragging_item
    {
        editor_context.selection.selected_entities.clear();
        editor_context.selection.last_selected_entity_id = None;
        editor_context.selection.last_selected_track_id = None;
    }

    (content_rect_for_clip_area, response)
}
