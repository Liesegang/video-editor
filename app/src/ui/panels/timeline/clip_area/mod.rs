use egui::Ui;
use library::core::model::project::Project;
use library::EditorService as ProjectService;
use std::sync::{Arc, RwLock};

use crate::{action::HistoryManager, state::context::EditorContext};

use crate::command::{CommandId, CommandRegistry};
use crate::ui::viewport::{ViewportConfig, ViewportController, ViewportState};

mod background;
mod clips;
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
        *self.v_zoom = zoom.y.clamp(self.min_v_zoom, self.max_v_zoom); // Basic vertical zoom limits
    }
}

#[allow(clippy::too_many_arguments)]
pub fn show_clip_area(
    ui_content: &mut Ui,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut ProjectService,
    project: &Arc<RwLock<Project>>,
    pixels_per_unit: f32,
    num_tracks: usize,
    row_height: f32,
    track_spacing: f32,
    composition_fps: f64,
    registry: &CommandRegistry,
) -> (egui::Rect, egui::Response) {
    // ...
    let (content_rect_for_clip_area, response) =
        ui_content.allocate_at_least(ui_content.available_size(), egui::Sense::hover()); // Changed to hover()

    let is_dragging_item = editor_context.interaction.dragged_item.is_some();

    // --- Data collection for entities ---
    let mut current_tracks: Vec<library::core::model::Track> = Vec::new();
    let selected_composition_id = editor_context.selection.composition_id;
    if let Some(comp_id) = selected_composition_id {
        if let Ok(proj_read) = project.read() {
            if let Some(comp) = proj_read.compositions.iter().find(|c| c.id == comp_id) {
                current_tracks = comp.tracks.clone();
            }
        }
    }
    // --- End Data collection for entities ---

    // --- Drawing of track backgrounds ---
    let painter = ui_content.painter_at(content_rect_for_clip_area);

    // Get Composition Duration
    let mut current_comp_duration = 300.0; // Default fallback
    if let Some(comp_id) = selected_composition_id {
        if let Ok(proj_read) = project.read() {
            if let Some(comp) = proj_read.compositions.iter().find(|c| c.id == comp_id) {
                current_comp_duration = comp.duration;
            }
        }
    }

    background::draw_track_backgrounds(
        &painter,
        content_rect_for_clip_area,
        num_tracks,
        row_height,
        track_spacing,
        editor_context.timeline.scroll_offset.y,
        editor_context.timeline.scroll_offset.x,
        pixels_per_unit,
        current_comp_duration,
    );

    // --- Main Interaction Block (for overall clip area, e.g., scroll, zoom, asset drop) ---
    // --- Viewport Controller for Zoom/Pan ---
    // Calculate Constraints
    const MAX_PIXELS_PER_FRAME_DESIRED: f32 = 20.0;
    let max_h_zoom = (MAX_PIXELS_PER_FRAME_DESIRED * composition_fps as f32)
        / editor_context.timeline.pixels_per_second;
    let min_possible_zoom = content_rect_for_clip_area.width()
        / (current_comp_duration as f32 * editor_context.timeline.pixels_per_second);
    let min_h_zoom = min_possible_zoom.min(0.01);

    // Hand Tool Key
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
        max_scroll_y: (num_tracks as f32 * (row_height + track_spacing)
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
        allow_pan_y: true, // Enable all
        min_zoom: 0.0001,
        max_zoom: 10000.0,
        ..Default::default()
    });

    let (_changed, vp_response) = controller.interact_with_rect(
        content_rect_for_clip_area,
        &mut state,
        &mut editor_context.interaction.handled_hand_tool_drag,
    );

    // Handle Box Selection State Update
    // If not panning (hand tool), and dragging Primary on background
    if !editor_context.interaction.handled_hand_tool_drag {
        // We check input directly because ViewportController might consume interactions if we aren't careful,
        // but here we used interact_with_rect which returns response.

        // Wait, controller.interact_with_rect calls ui.interact.
        // If we want to override or check drags on that same rect, we can use vp_response.

        if vp_response.drag_started_by(egui::PointerButton::Primary) {
            // Check modifiers?
            if !ui_content.input(|i| i.modifiers.alt) {
                // Assuming Alt is not for selection
                if let Some(pos) = vp_response.interact_pointer_pos() {
                    editor_context.interaction.timeline_selection_drag_start = Some(pos);
                }
            }
        }
    }

    // Call legacy interaction (drag drop / context menu)
    interactions::handle_drag_drop_and_context_menu(
        ui_content,
        &vp_response,
        content_rect_for_clip_area,
        editor_context,
        project,
        project_service,
        history_manager,
        pixels_per_unit,
        composition_fps,
        num_tracks,
        row_height,
        track_spacing,
    );

    // --- Loop for drawing and interacting with entities ---
    let clicked_on_entity = clips::draw_clips(
        ui_content,
        content_rect_for_clip_area,
        editor_context,
        project_service,
        history_manager,
        &current_tracks,
        project,
        pixels_per_unit,
        row_height,
        track_spacing,
        composition_fps,
    );

    // Box Selection Logic (Draw & Commit)
    if let Some(start_pos) = editor_context.interaction.timeline_selection_drag_start {
        if ui_content.input(|i| i.pointer.primary_down()) {
            if let Some(current_pos) = ui_content.input(|i| i.pointer.interact_pos()) {
                let selection_rect = egui::Rect::from_two_pos(start_pos, current_pos);

                // Draw selection box
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
            // Released
            if let Some(current_pos) = ui_content.input(|i| i.pointer.interact_pos()) {
                let selection_rect = egui::Rect::from_two_pos(start_pos, current_pos);
                // Commit selection
                let found_clips = clips::get_clips_in_box(
                    selection_rect,
                    editor_context,
                    &current_tracks,
                    pixels_per_unit,
                    row_height,
                    track_spacing,
                    composition_fps,
                    content_rect_for_clip_area.min.to_vec2(),
                );

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
            editor_context.interaction.timeline_selection_drag_start = None;
        }
    }

    // Final selection clearing logic
    if !editor_context.interaction.is_resizing_entity
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
