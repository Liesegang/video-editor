use crate::model::ui_types::TimelineClip;
use crate::state::context::EditorContext;
use crate::ui::panels::preview::gizmo;
use egui::{PointerButton, Pos2, Rect, Response, Ui};
use library::model::project::project::Project;
use library::service::project_service::ProjectService;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub struct PreviewInteractions<'a> {
    pub ui: &'a mut Ui,
    pub editor_context: &'a mut EditorContext,
    pub project: &'a Arc<RwLock<Project>>,
    pub project_service: &'a ProjectService,
    pub gui_clips: &'a [TimelineClip],
    pub to_screen: Box<dyn Fn(Pos2) -> Pos2 + 'a>, // Closure wrapper
    pub to_world: Box<dyn Fn(Pos2) -> Pos2 + 'a>,
}

impl<'a> PreviewInteractions<'a> {
    pub fn new(
        ui: &'a mut Ui,
        editor_context: &'a mut EditorContext,
        project: &'a Arc<RwLock<Project>>,
        project_service: &'a ProjectService,
        gui_clips: &'a [TimelineClip],
        to_screen: impl Fn(Pos2) -> Pos2 + 'a,
        to_world: impl Fn(Pos2) -> Pos2 + 'a,
    ) -> Self {
        Self {
            ui,
            editor_context,
            project,
            project_service,
            gui_clips,
            to_screen: Box::new(to_screen),
            to_world: Box::new(to_world),
        }
    }

    pub fn handle(&mut self, response: &Response, content_rect: Rect) {
        let pointer_pos = self.ui.input(|i| i.pointer.hover_pos());

        // 1. Gizmo Interaction
        let interacted_with_gizmo = gizmo::handle_gizmo_interaction(
            self.ui,
            self.editor_context,
            self.project,
            self.project_service,
            pointer_pos,
            &*self.to_world,
        );

        // 2. Hit Testing (Hover)
        let hovered_entity_id = self.check_hit_test(pointer_pos, content_rect);

        // Check panning input (Middle mouse OR Shift+LeftDrag is handled elsewhere? No, user wants Shift+Left to be MultiSelect usually)
        // Hand tool logic is in ViewportController. checking response.dragged_by(Middle) covers middle mouse.
        // What about Spacebar? Hand tool key is handled in ViewportController.
        let is_panning_input = response.dragged_by(PointerButton::Middle);

        // 3. Interactions
        if !is_panning_input && !interacted_with_gizmo {
            // Drag Start Detection
            if response.drag_started_by(PointerButton::Primary) {
                if let Some(hovered) = hovered_entity_id {
                    // Started drag on an entity
                    // Ensure it is selected (if not modifier click)
                    // If Shift/Ctrl is held, we might be adding it to selection?
                    // Usually dragging implies selection.
                    // If not selected, select it.
                    if !self.editor_context.is_selected(hovered) {
                        // Modifiers?
                        let modifiers = self.ui.input(|i| i.modifiers);
                        if modifiers.shift || modifiers.ctrl {
                            self.editor_context.add_selection(
                                hovered,
                                self.get_track_id(hovered).unwrap_or_default(),
                            );
                        } else {
                            self.editor_context.select_clip(
                                hovered,
                                self.get_track_id(hovered).unwrap_or_default(),
                            );
                        }
                    }

                    self.editor_context.interaction.is_moving_selected_entity = true;
                    self.init_drag_state(pointer_pos);
                } else {
                    // Started drag on background -> Box Selection
                    self.editor_context.interaction.is_moving_selected_entity = false;
                    self.editor_context.interaction.body_drag_state = None;
                    if let Some(pos) = pointer_pos {
                        self.editor_context.interaction.preview_selection_drag_start = Some(pos);
                    }
                }
            }

            // Drag Move (selected entities)
            if response.dragged_by(PointerButton::Primary)
                && self.editor_context.interaction.is_moving_selected_entity
            {
                self.handle_drag_move(pointer_pos);
            }

            // Click Selection (Mouse Released without Drag)
            if response.clicked() {
                self.handle_click_selection(hovered_entity_id);
            }

            // Box Selection (Active or Committing)
            self.handle_box_selection(response);
        }

        // Cleanup on release
        if self.ui.input(|i| i.pointer.any_released()) {
            self.editor_context.interaction.is_moving_selected_entity = false;
            self.editor_context.interaction.body_drag_state = None;
        }
    }

    fn is_clip_visible(&self, gc: &TimelineClip, current_frame: i64) -> bool {
        if gc.kind == library::model::project::TrackClipKind::Audio {
            return false;
        }

        let in_frame = gc.in_frame as i64;
        let out_frame = gc.out_frame as i64;

        current_frame >= in_frame && current_frame < out_frame
    }

    fn get_clip_screen_corners(&self, gc: &TimelineClip) -> [Pos2; 4] {
        let base_w = gc.width.unwrap_or(1920.0);
        let base_h = gc.height.unwrap_or(1080.0);
        let sx = gc.scale_x / 100.0;
        let sy = gc.scale_y / 100.0;
        let center = egui::pos2(gc.position[0], gc.position[1]);
        let angle_rad = gc.rotation.to_radians();
        let cos = angle_rad.cos();
        let sin = angle_rad.sin();

        let transform_point = |local_x: f32, local_y: f32| -> egui::Pos2 {
            let ox = local_x - gc.anchor_x;
            let oy = local_y - gc.anchor_y;
            let sx_ox = ox * sx;
            let sy_oy = oy * sy;
            let rx = sx_ox * cos - sy_oy * sin;
            let ry = sx_ox * sin + sy_oy * cos;
            (self.to_screen)(center + egui::vec2(rx, ry))
        };

        [
            transform_point(0.0, 0.0),
            transform_point(base_w, 0.0),
            transform_point(base_w, base_h),
            transform_point(0.0, base_h),
        ]
    }

    fn check_hit_test(&self, pointer_pos: Option<Pos2>, content_rect: Rect) -> Option<Uuid> {
        let pos = pointer_pos?;
        if !content_rect.contains(pos) {
            return None;
        }

        // Get current frame
        let current_frame = if let Ok(proj_read) = self.project.read() {
            if let Some(comp) = self.editor_context.get_current_composition(&proj_read) {
                (self.editor_context.timeline.current_time as f64 * comp.fps).round() as i64
            } else {
                0
            }
        } else {
            0
        };

        let mut sorted_clips: Vec<&TimelineClip> = self.gui_clips.iter().collect();
        // Z-sort
        if let Ok(proj_read) = self.project.read() {
            if let Some(comp) = self.editor_context.get_current_composition(&proj_read) {
                sorted_clips.sort_by_key(|gc| {
                    comp.tracks
                        .iter()
                        .position(|t| t.id == gc.track_id)
                        .unwrap_or(0)
                });
            }
        }

        // Iterate top-down
        for gc in sorted_clips.iter().rev() {
            if !self.is_clip_visible(gc, current_frame) {
                continue;
            }

            let corners = self.get_clip_screen_corners(gc);

            // Point in Convex Polygon Check
            let check_edge = |p1: Pos2, p2: Pos2, p: Pos2| -> f32 {
                (p2.x - p1.x) * (p.y - p1.y) - (p2.y - p1.y) * (p.x - p1.x)
            };

            let d1 = check_edge(corners[0], corners[1], pos);
            let d2 = check_edge(corners[1], corners[2], pos);
            let d3 = check_edge(corners[2], corners[3], pos);
            let d4 = check_edge(corners[3], corners[0], pos);

            let has_pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0 || d4 > 0.0;
            let has_neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0 || d4 < 0.0;

            if !(has_pos && has_neg) {
                return Some(gc.id);
            }
        }
        None
    }

    fn init_drag_state(&mut self, pointer_pos: Option<Pos2>) {
        if let Some(pointer_pos) = pointer_pos {
            let mut original_positions = std::collections::HashMap::new();
            for selected_id in &self.editor_context.selection.selected_entities {
                if let Some(gc) = self.gui_clips.iter().find(|c| c.id == *selected_id) {
                    original_positions.insert(*selected_id, gc.position);
                }
            }
            self.editor_context.interaction.body_drag_state =
                Some(crate::state::context_types::BodyDragState {
                    start_mouse_pos: pointer_pos,
                    original_positions,
                });
        }
    }

    fn handle_click_selection(&mut self, hovered_id: Option<Uuid>) {
        let modifiers = self.ui.input(|i| i.modifiers);

        let action = crate::ui::selection::get_click_action(&modifiers, hovered_id);

        match action {
            crate::ui::selection::ClickAction::Select(id) => {
                let track_id = self.get_track_id(id).unwrap_or_default();
                self.editor_context.select_clip(id, track_id);
            }
            crate::ui::selection::ClickAction::Toggle(id) => {
                let track_id = self.get_track_id(id).unwrap_or_default();
                self.editor_context.toggle_selection(id, track_id);
            }
            crate::ui::selection::ClickAction::Clear => {
                self.editor_context.selection.selected_entities.clear();
                self.editor_context.selection.last_selected_entity_id = None;
                self.editor_context.selection.last_selected_track_id = None;
            }
            crate::ui::selection::ClickAction::DoNothing => {}
        }
    }

    // ... (drag handle logic unchanged) ...
    fn handle_drag_move(&self, pointer_pos: Option<Pos2>) {
        let current_zoom = self.editor_context.view.zoom;
        if let Some(comp_id) = self.editor_context.selection.composition_id {
            if let Some(drag_state) = &self.editor_context.interaction.body_drag_state {
                if let Some(curr_mouse) = pointer_pos {
                    let screen_delta = curr_mouse - drag_state.start_mouse_pos;
                    let world_delta = screen_delta / current_zoom;

                    let current_time = self.editor_context.timeline.current_time as f64;

                    for (entity_id, orig_pos) in &drag_state.original_positions {
                        let new_x = orig_pos[0] as f64 + world_delta.x as f64;
                        let new_y = orig_pos[1] as f64 + world_delta.y as f64;

                        if let Some(tid) = self.get_track_id(*entity_id) {
                            let _ = self.project_service.update_property_or_keyframe(
                                comp_id,
                                tid,
                                *entity_id,
                                "position_x",
                                current_time,
                                library::model::project::property::PropertyValue::Number(
                                    ordered_float::OrderedFloat(new_x),
                                ),
                                None,
                            );
                            let _ = self.project_service.update_property_or_keyframe(
                                comp_id,
                                tid,
                                *entity_id,
                                "position_y",
                                current_time,
                                library::model::project::property::PropertyValue::Number(
                                    ordered_float::OrderedFloat(new_y),
                                ),
                                None,
                            );
                        }
                    }
                }
            }
        }
    }

    fn handle_box_selection(&mut self, _response: &Response) {
        if let Some(start_pos) = self.editor_context.interaction.preview_selection_drag_start {
            if self.ui.input(|i| i.pointer.primary_down()) {
                // Drawing Box
                if let Some(current_pos) = self.ui.input(|i| i.pointer.interact_pos()) {
                    let selection_rect = Rect::from_two_pos(start_pos, current_pos);
                    let painter = self.ui.painter();
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
                // Commit
                if let Some(current_pos) = self.ui.input(|i| i.pointer.interact_pos()) {
                    let selection_rect = Rect::from_two_pos(start_pos, current_pos);
                    let modifiers = self.ui.input(|i| i.modifiers);

                    let found_clips = self.get_clips_in_box(selection_rect);

                    match crate::ui::selection::get_box_action(&modifiers, found_clips) {
                        crate::ui::selection::BoxAction::Replace(ids) => {
                            self.editor_context.selection.selected_entities.clear();
                            self.editor_context.selection.last_selected_entity_id = None;
                            self.editor_context.selection.last_selected_track_id = None;

                            let mut last_id = None;
                            let mut last_track = None;
                            for id in ids {
                                self.editor_context.selection.selected_entities.insert(id);
                                last_id = Some(id);
                                last_track = self.get_track_id(id);
                            }
                            if let Some(lid) = last_id {
                                self.editor_context.selection.last_selected_entity_id = Some(lid);
                                self.editor_context.selection.last_selected_track_id = last_track;
                            }
                        }
                        crate::ui::selection::BoxAction::Add(ids) => {
                            let mut last_id = None;
                            let mut last_track = None;
                            for id in ids {
                                self.editor_context.selection.selected_entities.insert(id);
                                last_id = Some(id);
                                last_track = self.get_track_id(id);
                            }
                            if let Some(lid) = last_id {
                                self.editor_context.selection.last_selected_entity_id = Some(lid);
                                self.editor_context.selection.last_selected_track_id = last_track;
                            }
                        }
                    }
                }
                self.editor_context.interaction.preview_selection_drag_start = None;
            }
        }
    }

    fn get_clips_in_box(&self, selection_rect: Rect) -> Vec<Uuid> {
        let mut found = Vec::new();

        // Get current frame
        let current_frame = if let Ok(proj_read) = self.project.read() {
            if let Some(comp) = self.editor_context.get_current_composition(&proj_read) {
                (self.editor_context.timeline.current_time as f64 * comp.fps).round() as i64
            } else {
                0
            }
        } else {
            0
        };

        for gc in self.gui_clips {
            if !self.is_clip_visible(gc, current_frame) {
                continue;
            }

            let corners = self.get_clip_screen_corners(gc);

            let min_x = corners[0]
                .x
                .min(corners[1].x)
                .min(corners[2].x)
                .min(corners[3].x);
            let max_x = corners[0]
                .x
                .max(corners[1].x)
                .max(corners[2].x)
                .max(corners[3].x);
            let min_y = corners[0]
                .y
                .min(corners[1].y)
                .min(corners[2].y)
                .min(corners[3].y);
            let max_y = corners[0]
                .y
                .max(corners[1].y)
                .max(corners[2].y)
                .max(corners[3].y);

            let clip_screen_rect =
                Rect::from_min_max(egui::pos2(min_x, min_y), egui::pos2(max_x, max_y));

            if selection_rect.intersects(clip_screen_rect) {
                found.push(gc.id);
            }
        }
        found
    }

    fn get_track_id(&self, entity_id: Uuid) -> Option<Uuid> {
        self.gui_clips
            .iter()
            .find(|gc| gc.id == entity_id)
            .map(|gc| gc.track_id)
    }
}
