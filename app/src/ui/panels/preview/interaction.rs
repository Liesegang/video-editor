use crate::state::context::EditorContext;
use crate::ui::panels::preview::{action::PreviewAction, clip::PreviewClip, gizmo};
use egui::{PointerButton, Pos2, Rect, Response, Ui};
use library::model::project::project::Project;
use library::model::project::property::PropertyValue;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub(super) struct PreviewInteractions<'a> {
    pub(super) ui: &'a mut Ui,
    pub(super) editor_context: &'a mut EditorContext,
    pub(super) project: &'a Arc<RwLock<Project>>,
    pub(super) history_manager: &'a mut crate::action::HistoryManager,
    pub(super) gui_clips: &'a [PreviewClip<'a>],
    pub(super) to_screen: Box<dyn Fn(Pos2) -> Pos2 + 'a>, // Closure wrapper
    pub(super) to_world: Box<dyn Fn(Pos2) -> Pos2 + 'a>,
}

impl<'a> PreviewInteractions<'a> {
    pub(super) fn new(
        ui: &'a mut Ui,
        editor_context: &'a mut EditorContext,
        project: &'a Arc<RwLock<Project>>,
        history_manager: &'a mut crate::action::HistoryManager,
        gui_clips: &'a [PreviewClip<'a>],
        to_screen: impl Fn(Pos2) -> Pos2 + 'a,
        to_world: impl Fn(Pos2) -> Pos2 + 'a,
    ) -> Self {
        Self {
            ui,
            editor_context,
            project,
            history_manager,
            gui_clips,
            to_screen: Box::new(to_screen),
            to_world: Box::new(to_world),
        }
    }

    pub(super) fn handle(
        &mut self,
        response: &Response,
        content_rect: Rect,
        pending_actions: &mut Vec<PreviewAction>,
    ) {
        let pointer_pos = self.ui.input(|i| i.pointer.hover_pos());
        let active_tool = self.editor_context.view.active_tool.clone();

        // If Pan tool is active, ViewportController handles interaction.
        if active_tool == crate::state::context_types::PreviewTool::Pan {
            return;
        }

        // 1. Gizmo Interaction
        let mut interacted_with_gizmo = false;
        if active_tool == crate::state::context_types::PreviewTool::Select {
            interacted_with_gizmo = gizmo::handle_gizmo_interaction(
                self.ui,
                self.editor_context,
                self.project,
                self.history_manager,
                pointer_pos,
                &*self.to_world,
                pending_actions,
            );
        } else if active_tool == crate::state::context_types::PreviewTool::Shape {
            // 1. Ensure State is Loaded
            let mut ensure_loaded = false;
            if self
                .editor_context
                .interaction
                .preview
                .vector_editor_state
                .is_none()
            {
                if let Some(id) = self
                    .editor_context
                    .selection
                    .selected_entities
                    .iter()
                    .next()
                {
                    // Check if it is a shape and get path
                    // use gui_clips to get track_id
                    if let Some(gc) = self.gui_clips.iter().find(|c| c.id() == *id) {
                        if matches!(
                            gc.clip.kind,
                            library::model::project::clip::TrackClipKind::Shape
                        ) {
                            if let Some(path_str) = gc.clip.properties.get_string("path") {
                                let state = crate::ui::panels::preview::vector_editor::svg_parser::parse_svg_path(&path_str);
                                self.editor_context.interaction.preview.vector_editor_state =
                                    Some(state);
                                ensure_loaded = true;
                            }
                        }
                    }
                }
            } else {
                ensure_loaded = true;
            }

            // 2. Handle Interaction
            if ensure_loaded {
                // Get Transform for the edited entity
                // We need to know WHICH entity is being edited.
                // We rely on selection.
                if let Some(id) = self
                    .editor_context
                    .selection
                    .selected_entities
                    .iter()
                    .next()
                {
                    if let Some(gc) = self.gui_clips.iter().find(|c| c.id() == *id) {
                        // Build Transform
                        let transform = library::model::frame::transform::Transform {
                            position: library::model::frame::transform::Position {
                                x: gc.transform.position.x,
                                y: gc.transform.position.y,
                            },
                            scale: library::model::frame::transform::Scale {
                                x: gc.transform.scale.x,
                                y: gc.transform.scale.y,
                            },
                            rotation: gc.transform.rotation,
                            anchor: library::model::frame::transform::Position {
                                x: gc.transform.anchor.x,
                                y: gc.transform.anchor.y,
                            },
                            opacity: gc.transform.opacity,
                        };

                        let mut changed = false;
                        if let Some(state) =
                            &mut self.editor_context.interaction.preview.vector_editor_state
                        {
                            let mut interaction = crate::ui::panels::preview::vector_editor::interaction::VectorEditorInteraction {
                                  state,
                                  transform,
                                  to_screen: Box::new(|p| (self.to_screen)(p)),
                                  to_world: Box::new(|p| (self.to_world)(p)),
                               };
                            let (changed_state, captured) = interaction.handle(self.ui, response);
                            changed = changed_state;
                            if captured {
                                interacted_with_gizmo = true;
                            }
                        }

                        if changed {
                            // Save back
                            if let Some(state) =
                                &self.editor_context.interaction.preview.vector_editor_state
                            {
                                let new_path = crate::ui::panels::preview::vector_editor::svg_writer::to_svg_path(state);

                                // Update property
                                if let Some(comp_id) = self.editor_context.selection.composition_id
                                {
                                    let current_time =
                                        self.editor_context.timeline.current_time as f64;
                                    pending_actions.push(PreviewAction::UpdateProperty {
                                        comp_id,
                                        track_id: gc.track_id,
                                        entity_id: *id,
                                        prop_name: "path".to_string(),
                                        time: current_time,
                                        value: PropertyValue::String(new_path),
                                    });
                                }
                            }
                        }
                        if changed {
                            interacted_with_gizmo = true;
                        }
                    }
                }
            }
        }

        // 2. Hit Testing (Hover)
        let hovered_entity_id = if active_tool == crate::state::context_types::PreviewTool::Select
            || active_tool == crate::state::context_types::PreviewTool::Text
            || active_tool == crate::state::context_types::PreviewTool::Shape
        // Allow selection when in Shape tool
        {
            self.check_hit_test(pointer_pos, content_rect)
        } else {
            None
        };

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
                    let modifiers = self.ui.input(|i| i.modifiers);
                    let action = crate::ui::selection::SelectionAction::from_modifiers(&modifiers);
                    let track_id = self.get_track_id(hovered).unwrap_or_default();
                    let mut should_drag = true;

                    match action {
                        crate::ui::selection::SelectionAction::Remove => {
                            if self.editor_context.is_selected(hovered) {
                                self.editor_context.toggle_selection(hovered, track_id);
                            }
                            should_drag = false;
                        }
                        crate::ui::selection::SelectionAction::Add
                        | crate::ui::selection::SelectionAction::Toggle => {
                            if !self.editor_context.is_selected(hovered) {
                                self.editor_context.toggle_selection(hovered, track_id);
                            }
                        }
                        crate::ui::selection::SelectionAction::Replace => {
                            if !self.editor_context.is_selected(hovered) {
                                self.editor_context.select_clip(hovered, track_id);
                            }
                        }
                    }

                    if should_drag && self.editor_context.is_selected(hovered) {
                        self.editor_context
                            .interaction
                            .preview
                            .is_moving_selected_entity = true;
                        self.init_drag_state(pointer_pos);
                    }
                } else {
                    // Started drag on background -> Box Selection
                    self.editor_context
                        .interaction
                        .preview
                        .is_moving_selected_entity = false;
                    self.editor_context.interaction.preview.body_drag_state = None;
                    if let Some(pos) = pointer_pos {
                        self.editor_context
                            .interaction
                            .preview
                            .preview_selection_drag_start = Some(pos);
                    }
                }
            }

            // Drag Move (selected entities)
            if response.dragged_by(PointerButton::Primary)
                && self
                    .editor_context
                    .interaction
                    .preview
                    .is_moving_selected_entity
            {
                self.handle_drag_move(pointer_pos, pending_actions);
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
            if self
                .editor_context
                .interaction
                .preview
                .is_moving_selected_entity
            {
                if let Ok(proj) = self.project.read() {
                    self.history_manager.push_project_state(proj.clone());
                }
            }
            self.editor_context
                .interaction
                .preview
                .is_moving_selected_entity = false;
            self.editor_context.interaction.preview.body_drag_state = None;
        }
    }

    fn is_clip_visible(&self, gc: &PreviewClip, current_frame: i64) -> bool {
        if gc.clip.kind == library::model::project::clip::TrackClipKind::Audio {
            return false;
        }

        let in_frame = gc.clip.in_frame as i64;
        let out_frame = gc.clip.out_frame as i64;

        current_frame >= in_frame && current_frame < out_frame
    }

    fn get_clip_screen_corners(&self, gc: &PreviewClip) -> [Pos2; 4] {
        let base_w = gc.content_bounds.map(|b| b.2).unwrap_or(1920.0);
        let base_h = gc.content_bounds.map(|b| b.3).unwrap_or(1080.0);

        // content_point is the top-left offset of the content in local space, relative to (0,0)
        let (off_x, off_y) = if let Some(pt) = gc.content_bounds {
            (pt.0, pt.1)
        } else {
            (0.0, 0.0)
        };

        let sx = gc.transform.scale.x as f32 / 100.0;
        let sy = gc.transform.scale.y as f32 / 100.0;
        let center = egui::pos2(
            gc.transform.position.x as f32,
            gc.transform.position.y as f32,
        );
        let angle_rad = (gc.transform.rotation as f32).to_radians();
        let cos = angle_rad.cos();
        let sin = angle_rad.sin();

        let transform_point = |local_x: f32, local_y: f32| -> egui::Pos2 {
            // Apply Content Offset
            let lx = local_x + off_x;
            let ly = local_y + off_y;

            let ox = lx - gc.transform.anchor.x as f32;
            let oy = ly - gc.transform.anchor.y as f32;
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

        // TODO: Z-sort properly. Here we rely on iteration order, which is track order usually.
        // Track order is bottom-to-top rendering usually? Or top-to-bottom tracks?
        // Usually lower track index = lower layer (rendered first).
        // So rev() gives top-most layer.
        for gc in self.gui_clips.iter().rev() {
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
                return Some(gc.id());
            }
        }
        None
    }

    fn init_drag_state(&mut self, pointer_pos: Option<Pos2>) {
        if let Some(pointer_pos) = pointer_pos {
            let mut original_positions = std::collections::HashMap::new();
            for selected_id in &self.editor_context.selection.selected_entities {
                if let Some(gc) = self.gui_clips.iter().find(|c| c.id() == *selected_id) {
                    original_positions.insert(
                        *selected_id,
                        [
                            gc.transform.position.x as f32,
                            gc.transform.position.y as f32,
                        ],
                    );
                }
            }
            self.editor_context.interaction.preview.body_drag_state =
                Some(crate::state::context_types::BodyDragState {
                    start_mouse_pos: pointer_pos,
                    original_positions,
                });
        }
    }

    fn handle_click_selection(&mut self, hovered_id: Option<Uuid>) {
        if self.editor_context.view.active_tool == crate::state::context_types::PreviewTool::Text {
            if let Some(id) = hovered_id {
                let is_text = self.gui_clips.iter().any(|c| {
                    c.id() == id
                        && matches!(
                            c.clip.kind,
                            library::model::project::clip::TrackClipKind::Text
                        )
                });
                if is_text {
                    self.editor_context
                        .interaction
                        .preview
                        .editing_text_entity_id = Some(id);
                    if let Some(gc) = self.gui_clips.iter().find(|c| c.id() == id) {
                        if let Some(text) = gc.clip.properties.get_string("text") {
                            self.editor_context.interaction.preview.text_edit_buffer = text;
                        }
                    }
                } else {
                    self.editor_context
                        .interaction
                        .preview
                        .editing_text_entity_id = None;
                }
            } else {
                self.editor_context
                    .interaction
                    .preview
                    .editing_text_entity_id = None;
            }
        }

        let modifiers = self.ui.input(|i| i.modifiers);

        let action = crate::ui::selection::get_click_action(&modifiers, hovered_id);

        match action {
            crate::ui::selection::ClickAction::Select(id) => {
                let track_id = self.get_track_id(id).unwrap_or_default();
                self.editor_context.select_clip(id, track_id);
            }
            crate::ui::selection::ClickAction::Add(id) => {
                let track_id = self.get_track_id(id).unwrap_or_default();
                if !self.editor_context.is_selected(id) {
                    self.editor_context.toggle_selection(id, track_id);
                }
            }
            crate::ui::selection::ClickAction::Remove(id) => {
                let track_id = self.get_track_id(id).unwrap_or_default();
                if self.editor_context.is_selected(id) {
                    self.editor_context.toggle_selection(id, track_id);
                }
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

    fn handle_drag_move(
        &self,
        pointer_pos: Option<Pos2>,
        pending_actions: &mut Vec<PreviewAction>,
    ) {
        let current_zoom = self.editor_context.view.zoom;
        if let Some(comp_id) = self.editor_context.selection.composition_id {
            if let Some(drag_state) = &self.editor_context.interaction.preview.body_drag_state {
                if let Some(curr_mouse) = pointer_pos {
                    let screen_delta = curr_mouse - drag_state.start_mouse_pos;
                    let world_delta = screen_delta / current_zoom;

                    let current_time = self.editor_context.timeline.current_time as f64;

                    for (entity_id, orig_pos) in &drag_state.original_positions {
                        let new_x = orig_pos[0] as f64 + world_delta.x as f64;
                        let new_y = orig_pos[1] as f64 + world_delta.y as f64;

                        if let Some(tid) = self.get_track_id(*entity_id) {
                            pending_actions.push(PreviewAction::UpdateProperty {
                                comp_id,
                                track_id: tid,
                                entity_id: *entity_id,
                                prop_name: "position".to_string(),
                                time: current_time,
                                value: PropertyValue::Vec2(
                                    library::model::project::property::Vec2 {
                                        x: ordered_float::OrderedFloat(new_x),
                                        y: ordered_float::OrderedFloat(new_y),
                                    },
                                ),
                            });
                        }
                    }
                }
            }
        }
    }

    fn handle_box_selection(&mut self, _response: &Response) {
        if let Some(start_pos) = self
            .editor_context
            .interaction
            .preview
            .preview_selection_drag_start
        {
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
                        crate::ui::selection::BoxAction::Remove(ids) => {
                            for id in ids {
                                self.editor_context.selection.selected_entities.remove(&id);
                            }
                        }
                    }
                }
                self.editor_context
                    .interaction
                    .preview
                    .preview_selection_drag_start = None;
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
                found.push(gc.id());
            }
        }
        found
    }

    fn get_track_id(&self, entity_id: Uuid) -> Option<Uuid> {
        self.gui_clips
            .iter()
            .find(|gc| gc.id() == entity_id)
            .map(|gc| gc.track_id)
    }
    pub(super) fn draw_text_overlay(&mut self, pending_actions: &mut Vec<PreviewAction>) {
        if let Some(id) = self
            .editor_context
            .interaction
            .preview
            .editing_text_entity_id
        {
            if let Some(gc) = self.gui_clips.iter().find(|c| c.id() == id) {
                let corners = self.get_clip_screen_corners(gc);
                let min_x = corners.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
                let min_y = corners.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
                let max_x = corners
                    .iter()
                    .map(|p| p.x)
                    .fold(f32::NEG_INFINITY, f32::max);
                let max_y = corners
                    .iter()
                    .map(|p| p.y)
                    .fold(f32::NEG_INFINITY, f32::max);

                let rect = Rect::from_min_max(Pos2::new(min_x, min_y), Pos2::new(max_x, max_y));

                // Calculate Font Size
                let font_size = gc.clip.properties.get_f32("size").unwrap_or(100.0);

                let zoom = self.editor_context.view.zoom;
                // Assuming uniform scale or using scale_y for height
                let scale_factor = (gc.transform.scale.y as f32 / 100.0) * zoom;
                let effective_size = font_size * scale_factor;

                let mut text = self
                    .editor_context
                    .interaction
                    .preview
                    .text_edit_buffer
                    .clone();
                let widget_id = self.ui.make_persistent_id(id).with("text_edit");

                let response = self.ui.put(
                    rect,
                    egui::TextEdit::multiline(&mut text)
                        .id(widget_id)
                        .frame(false)
                        .text_color(egui::Color32::TRANSPARENT)
                        .font(egui::FontId::proportional(effective_size))
                        .desired_width(rect.width()),
                );

                if !response.has_focus() {
                    response.request_focus();
                }

                if response.changed() {
                    self.editor_context.interaction.preview.text_edit_buffer = text.clone();

                    if let Some(comp_id) = self.editor_context.selection.composition_id {
                        pending_actions.push(PreviewAction::UpdateProperty {
                            comp_id,
                            track_id: gc.track_id,
                            entity_id: id,
                            prop_name: "text".to_string(),
                            time: self.editor_context.timeline.current_time as f64,
                            value: PropertyValue::String(text),
                        });
                    }
                }
            }
        }
    }
}
