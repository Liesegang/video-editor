use crate::state::context::EditorContext;
use crate::ui::panels::preview::{
    action::PreviewAction,
    clip::{PreviewClip, visual_for_selection},
    gizmo,
};
use egui::{PointerButton, Pos2, Rect, Response, Ui};
use library::model::property::{PropertyValue, Vec2};
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Clone)]
struct PreviewHit {
    node_id: Uuid,
    instance_path: Vec<Uuid>,
}

pub struct PreviewInteractions<'a> {
    pub ui: &'a mut Ui,
    pub editor_context: &'a mut EditorContext,
    pub gui_clips: &'a [PreviewClip],
    pub to_screen: Box<dyn Fn(Pos2) -> Pos2 + 'a>, // Closure wrapper
    pub to_world: Box<dyn Fn(Pos2) -> Pos2 + 'a>,
}

impl<'a> PreviewInteractions<'a> {
    pub fn new(
        ui: &'a mut Ui,
        editor_context: &'a mut EditorContext,
        gui_clips: &'a [PreviewClip],
        to_screen: impl Fn(Pos2) -> Pos2 + 'a,
        to_world: impl Fn(Pos2) -> Pos2 + 'a,
    ) -> Self {
        Self {
            ui,
            editor_context,
            gui_clips,
            to_screen: Box::new(to_screen),
            to_world: Box::new(to_world),
        }
    }

    pub fn handle(
        &mut self,
        response: &Response,
        content_rect: Rect,
        pan_gesture_owned: bool,
        pending_actions: &mut Vec<PreviewAction>,
    ) {
        if pan_gesture_owned {
            // The viewport owns this entire primary press/drag/release. Clear
            // transient content gestures without generating updates or a
            // history commit, so the release cannot click through.
            self.editor_context.interaction.is_moving_selected_entity = false;
            self.editor_context.interaction.body_drag_state = None;
            self.editor_context.interaction.preview_selection_drag_start = None;
            self.editor_context.interaction.gizmo_state = None;
            if let Some(state) = &mut self.editor_context.interaction.vector_editor_state {
                state.selected_handle = None;
            }
            return;
        }

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
                self.gui_clips,
                pointer_pos,
                &*self.to_world,
                pending_actions,
            );
        } else if active_tool == crate::state::context_types::PreviewTool::Shape {
            if let Some(id) = self
                .editor_context
                .selection
                .selected_entities
                .iter()
                .next()
                .copied()
            {
                if let Some(gc) = visual_for_selection(
                    self.gui_clips,
                    id,
                    self.editor_context
                        .interaction
                        .preview_selected_instance_path
                        .as_deref(),
                ) {
                    if matches!(
                        &gc.node.content,
                        library::model::NodeContent::Generator(
                            library::model::GeneratorContent::Shape
                        )
                    ) {
                        if let Some(path_str) = gc.node.properties.get_string("path") {
                            // The path is always projected from Project. Only point
                            // selection/handle state survives between frames.
                            let parsed_path = crate::ui::panels::preview::vector_editor::svg_parser::parse_svg_path(&path_str);
                            if let Err(error) = &parsed_path {
                                log::warn!("Cannot edit invalid shape path: {error}");
                            }
                            if let Ok(mut path) = parsed_path {
                                let state = self
                                    .editor_context
                                    .interaction
                                    .vector_editor_state
                                    .get_or_insert_with(Default::default);
                                let mut interaction = crate::ui::panels::preview::vector_editor::interaction::VectorEditorInteraction {
                                  state,
                                  path: &mut path,
                                  transform: gc.world_transform,
                                  to_screen: Box::new(|p| (self.to_screen)(p)),
                                  to_world: Box::new(|p| (self.to_world)(p)),
                               };
                                let (changed, captured, commit_requested) =
                                    interaction.handle(self.ui, response);
                                drop(interaction);
                                if captured {
                                    interacted_with_gizmo = true;
                                }

                                if changed {
                                    let new_path = crate::ui::panels::preview::vector_editor::svg_writer::to_svg_path(&path);

                                    // Update property
                                    pending_actions.push(PreviewAction::UpdateProperty {
                                        node_id: id,
                                        prop_name: "path".to_string(),
                                        time: self.editor_context.timeline.current_time as f64,
                                        value: PropertyValue::String(new_path),
                                    });
                                    interacted_with_gizmo = true;
                                }
                                if commit_requested {
                                    pending_actions.push(PreviewAction::CommitHistory);
                                }
                            }
                        }
                    }
                }
            }
        }

        // 2. Hit Testing (Hover)
        let hovered_hit = if active_tool == crate::state::context_types::PreviewTool::Select
            || active_tool == crate::state::context_types::PreviewTool::Text
            || active_tool == crate::state::context_types::PreviewTool::Shape
        {
            // Allow selection when in Shape tool
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
                if let Some(hit) = hovered_hit.as_ref() {
                    let hovered = hit.node_id;
                    // Started drag on an entity
                    // Ensure it is selected (if not modifier click)
                    // If Shift/Ctrl is held, we might be adding it to selection?
                    // Usually dragging implies selection.
                    // If not selected, select it.
                    let modifiers = self.ui.input(|i| i.modifiers);
                    let action = crate::ui::selection::SelectionAction::from_modifiers(&modifiers);
                    let track_id = self.get_track_id(hovered, Some(&hit.instance_path));
                    let mut should_drag = true;

                    match action {
                        crate::ui::selection::SelectionAction::Remove => {
                            if self.editor_context.is_selected(hovered) {
                                self.editor_context
                                    .toggle_entity_selection(hovered, track_id);
                            }
                            should_drag = false;
                        }
                        crate::ui::selection::SelectionAction::Add
                        | crate::ui::selection::SelectionAction::Toggle => {
                            if !self.editor_context.is_selected(hovered) {
                                self.editor_context
                                    .toggle_entity_selection(hovered, track_id);
                            }
                        }
                        crate::ui::selection::SelectionAction::Replace => {
                            if !self.editor_context.is_selected(hovered) {
                                self.editor_context.select_entity(hovered, track_id);
                            }
                        }
                    }

                    if should_drag && self.editor_context.is_selected(hovered) {
                        self.editor_context.selection.last_selected_entity_id = Some(hovered);
                        self.editor_context.selection.last_selected_track_id = track_id;
                        self.editor_context
                            .interaction
                            .preview_selected_instance_path = Some(hit.instance_path.clone());
                        self.editor_context.interaction.is_moving_selected_entity = true;
                        self.init_drag_state(pointer_pos);
                    }
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
                self.handle_drag_move(pointer_pos, pending_actions);
            }

            // Click Selection (Mouse Released without Drag)
            if response.clicked() {
                self.handle_click_selection(hovered_hit.as_ref());
            }

            // Box Selection (Active or Committing)
            self.handle_box_selection(response);
        }

        // Cleanup only a body drag owned by Preview. Timeline and Preview are
        // rendered in the same frame and currently share the legacy moving
        // flag; consuming an unrelated release here would cancel Timeline's
        // drag before Timeline can commit its Project edit.
        if self.editor_context.interaction.body_drag_state.is_some()
            && self.ui.input(|i| i.pointer.any_released())
        {
            if self.editor_context.interaction.is_moving_selected_entity {
                pending_actions.push(PreviewAction::CommitHistory);
            }
            self.editor_context.interaction.is_moving_selected_entity = false;
            self.editor_context.interaction.body_drag_state = None;
        }
    }

    fn get_clip_screen_corners(&self, gc: &PreviewClip) -> Option<[Pos2; 4]> {
        let (x, y, width, height) = gc.content_bounds?;
        let transform_point = |local_x: f32, local_y: f32| {
            let (world_x, world_y) = gc
                .world_transform
                .map_point(f64::from(local_x), f64::from(local_y));
            (self.to_screen)(egui::pos2(world_x as f32, world_y as f32))
        };

        Some([
            transform_point(x, y),
            transform_point(x + width, y),
            transform_point(x + width, y + height),
            transform_point(x, y + height),
        ])
    }

    fn check_hit_test(&self, pointer_pos: Option<Pos2>, content_rect: Rect) -> Option<PreviewHit> {
        let pos = pointer_pos?;
        if !content_rect.contains(pos) {
            return None;
        }

        // FrameItem order is renderer order, so reverse traversal is the
        // actual top-most visual first (including ordered Merge inputs).
        for gc in self.gui_clips.iter().rev() {
            let Some(corners) = self.get_clip_screen_corners(gc) else {
                continue;
            };

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
                return Some(PreviewHit {
                    node_id: gc.id(),
                    instance_path: gc.instance_path.clone(),
                });
            }
        }
        None
    }

    fn init_drag_state(&mut self, pointer_pos: Option<Pos2>) {
        if let Some(pointer_pos) = pointer_pos {
            let mut original_positions = std::collections::HashMap::new();
            for selected_id in &self.editor_context.selection.selected_entities {
                let instance_path = if Some(*selected_id)
                    == self.editor_context.selection.last_selected_entity_id
                {
                    self.editor_context
                        .interaction
                        .preview_selected_instance_path
                        .as_deref()
                } else {
                    None
                };
                if let Some(gc) = visual_for_selection(self.gui_clips, *selected_id, instance_path)
                {
                    original_positions.insert(
                        *selected_id,
                        [
                            gc.source_transform.position.x as f32,
                            gc.source_transform.position.y as f32,
                        ],
                    );
                }
            }
            self.editor_context.interaction.body_drag_state =
                Some(crate::state::context_types::BodyDragState {
                    start_mouse_pos: pointer_pos,
                    original_positions,
                });
        }
    }

    fn handle_click_selection(&mut self, hovered_hit: Option<&PreviewHit>) {
        let hovered_id = hovered_hit.map(|hit| hit.node_id);
        if self.editor_context.view.active_tool == crate::state::context_types::PreviewTool::Text {
            if let Some(hit) = hovered_hit {
                let id = hit.node_id;
                let visual =
                    visual_for_selection(self.gui_clips, id, Some(hit.instance_path.as_slice()));
                let is_text = visual.is_some_and(|visual| {
                    matches!(
                        &visual.node.content,
                        library::model::NodeContent::Generator(
                            library::model::GeneratorContent::Text
                        )
                    )
                });
                if is_text {
                    self.editor_context.interaction.editing_text_entity_id = Some(id);
                    if let Some(gc) = visual {
                        if let Some(text) = gc.node.properties.get_string("text") {
                            self.editor_context.interaction.text_edit_buffer = text;
                        }
                    }
                } else {
                    self.editor_context.interaction.editing_text_entity_id = None;
                }
            } else {
                self.editor_context.interaction.editing_text_entity_id = None;
            }
        }

        let modifiers = self.ui.input(|i| i.modifiers);

        let action = crate::ui::selection::get_click_action(&modifiers, hovered_id);

        match action {
            crate::ui::selection::ClickAction::Select(id) => {
                let instance_path = hovered_hit
                    .filter(|hit| hit.node_id == id)
                    .map(|hit| hit.instance_path.clone());
                let track_id = self.get_track_id(id, instance_path.as_deref());
                self.editor_context.select_entity(id, track_id);
                self.editor_context
                    .interaction
                    .preview_selected_instance_path = instance_path;
            }
            crate::ui::selection::ClickAction::Add(id) => {
                let instance_path = hovered_hit
                    .filter(|hit| hit.node_id == id)
                    .map(|hit| hit.instance_path.clone());
                let track_id = self.get_track_id(id, instance_path.as_deref());
                if !self.editor_context.is_selected(id) {
                    self.editor_context.toggle_entity_selection(id, track_id);
                    self.editor_context
                        .interaction
                        .preview_selected_instance_path = instance_path;
                }
            }
            crate::ui::selection::ClickAction::Remove(id) => {
                let instance_path = hovered_hit
                    .filter(|hit| hit.node_id == id)
                    .map(|hit| hit.instance_path.clone());
                let track_id = self.get_track_id(id, instance_path.as_deref());
                if self.editor_context.is_selected(id) {
                    self.editor_context.toggle_entity_selection(id, track_id);
                }
                self.editor_context
                    .interaction
                    .preview_selected_instance_path = None;
            }
            crate::ui::selection::ClickAction::Toggle(id) => {
                let instance_path = hovered_hit
                    .filter(|hit| hit.node_id == id)
                    .map(|hit| hit.instance_path.clone());
                let track_id = self.get_track_id(id, instance_path.as_deref());
                self.editor_context.toggle_entity_selection(id, track_id);
                self.editor_context
                    .interaction
                    .preview_selected_instance_path = self
                    .editor_context
                    .is_selected(id)
                    .then_some(instance_path)
                    .flatten();
            }
            crate::ui::selection::ClickAction::Clear => {
                self.editor_context.selection.selected_entities.clear();
                self.editor_context.selection.last_selected_entity_id = None;
                self.editor_context.selection.last_selected_track_id = None;
                self.editor_context
                    .interaction
                    .preview_selected_instance_path = None;
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
        if let Some(drag_state) = &self.editor_context.interaction.body_drag_state {
            if let Some(curr_mouse) = pointer_pos {
                let screen_delta = curr_mouse - drag_state.start_mouse_pos;
                let world_delta = screen_delta / current_zoom;

                let current_time = self.editor_context.timeline.current_time as f64;

                for (entity_id, orig_pos) in &drag_state.original_positions {
                    let local_delta = self
                        .selected_visual(*entity_id)
                        .and_then(|visual| inverse_map_vector(visual.parent_transform, world_delta))
                        .unwrap_or(world_delta);
                    let new_x = orig_pos[0] as f64 + local_delta.x as f64;
                    let new_y = orig_pos[1] as f64 + local_delta.y as f64;

                    pending_actions.push(PreviewAction::UpdateProperty {
                        node_id: *entity_id,
                        prop_name: "position".to_string(),
                        time: current_time,
                        value: PropertyValue::Vec2(Vec2 {
                            x: ordered_float::OrderedFloat(new_x),
                            y: ordered_float::OrderedFloat(new_y),
                        }),
                    });
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
                            self.editor_context
                                .interaction
                                .preview_selected_instance_path = None;

                            let mut last_id = None;
                            let mut last_track = None;
                            for id in ids {
                                self.editor_context.selection.selected_entities.insert(id);
                                last_id = Some(id);
                                last_track = self.get_track_id(id, None);
                            }
                            if let Some(lid) = last_id {
                                self.editor_context.selection.last_selected_entity_id = Some(lid);
                                self.editor_context.selection.last_selected_track_id = last_track;
                                self.editor_context
                                    .interaction
                                    .preview_selected_instance_path = None;
                            }
                        }
                        crate::ui::selection::BoxAction::Add(ids) => {
                            let mut last_id = None;
                            let mut last_track = None;
                            for id in ids {
                                self.editor_context.selection.selected_entities.insert(id);
                                last_id = Some(id);
                                last_track = self.get_track_id(id, None);
                            }
                            if let Some(lid) = last_id {
                                self.editor_context.selection.last_selected_entity_id = Some(lid);
                                self.editor_context.selection.last_selected_track_id = last_track;
                                self.editor_context
                                    .interaction
                                    .preview_selected_instance_path = None;
                            }
                        }
                        crate::ui::selection::BoxAction::Remove(ids) => {
                            for id in ids {
                                self.editor_context.selection.selected_entities.remove(&id);
                            }
                            self.editor_context
                                .interaction
                                .preview_selected_instance_path = None;
                        }
                    }
                }
                self.editor_context.interaction.preview_selection_drag_start = None;
            }
        }
    }

    fn get_clips_in_box(&self, selection_rect: Rect) -> Vec<Uuid> {
        let mut found = Vec::new();
        let mut seen = HashSet::new();

        for gc in self.gui_clips {
            let Some(corners) = self.get_clip_screen_corners(gc) else {
                continue;
            };

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

            if selection_rect.intersects(clip_screen_rect) && seen.insert(gc.id()) {
                found.push(gc.id());
            }
        }
        found
    }

    fn selected_visual(&self, entity_id: Uuid) -> Option<&PreviewClip> {
        let instance_path =
            if Some(entity_id) == self.editor_context.selection.last_selected_entity_id {
                self.editor_context
                    .interaction
                    .preview_selected_instance_path
                    .as_deref()
            } else {
                None
            };
        visual_for_selection(self.gui_clips, entity_id, instance_path)
    }

    fn get_track_id(&self, entity_id: Uuid, instance_path: Option<&[Uuid]>) -> Option<Uuid> {
        visual_for_selection(self.gui_clips, entity_id, instance_path)
            .and_then(|visual| visual.track_id)
    }

    pub fn draw_text_overlay(&mut self, pending_actions: &mut Vec<PreviewAction>) {
        if let Some(id) = self.editor_context.interaction.editing_text_entity_id {
            if let Some(gc) = self.selected_visual(id) {
                let Some(corners) = self.get_clip_screen_corners(gc) else {
                    return;
                };
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
                let font_size = gc.node.properties.get_f32("size").unwrap_or(100.0);

                let zoom = self.editor_context.view.zoom;
                // Assuming uniform scale or using scale_y for height
                let scale_factor = gc.transform.scale.y as f32 * zoom;
                let effective_size = font_size * scale_factor;

                let mut text = self.editor_context.interaction.text_edit_buffer.clone();
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

                if response.changed() {
                    self.editor_context.interaction.text_edit_buffer = text.clone();

                    pending_actions.push(PreviewAction::UpdateProperty {
                        node_id: id,
                        prop_name: "text".to_string(),
                        time: self.editor_context.timeline.current_time as f64,
                        value: PropertyValue::String(text),
                    });
                }

                let finish_edit = response.lost_focus()
                    || (response.has_focus()
                        && self.ui.input(|input| input.key_pressed(egui::Key::Escape)));
                if finish_edit {
                    pending_actions.push(PreviewAction::CommitHistory);
                    self.editor_context.interaction.editing_text_entity_id = None;
                } else if !response.has_focus() {
                    response.request_focus();
                }
            }
        }
    }
}

fn inverse_map_vector(
    transform: library::rendering::renderer::Affine2D,
    vector: egui::Vec2,
) -> Option<egui::Vec2> {
    let determinant = transform.scale_x * transform.scale_y - transform.skew_x * transform.skew_y;
    if determinant.abs() <= f64::EPSILON {
        return None;
    }
    Some(egui::vec2(
        ((transform.scale_y * f64::from(vector.x) - transform.skew_x * f64::from(vector.y))
            / determinant) as f32,
        ((-transform.skew_y * f64::from(vector.x) + transform.scale_x * f64::from(vector.y))
            / determinant) as f32,
    ))
}
