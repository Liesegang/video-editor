use crate::state::context::EditorContext;
use crate::state::context_types::{PreviewBodyDragTarget, PreviewEditTarget, SelectionTarget};
use crate::ui::panels::preview::{
    action::PreviewAction,
    clip::{
        resolve_owner_edit_target, visual_for_exact_instance, OwnerEditTargetResolution,
        PreviewClip, PreviewSpatialLayer,
    },
    gizmo,
    routing::exact_visual_for_edit_target,
};
use egui::{PointerButton, Pos2, Rect, Response, Ui};
use library::model::property::{PropertyValue, Vec2};
use std::collections::HashSet;
use uuid::Uuid;

#[derive(Clone)]
struct PreviewHit {
    owner_target: SelectionTarget,
    content_node_id: Uuid,
    editable_spatial_node_id: Option<Uuid>,
    instance_path: Vec<Uuid>,
}

impl PreviewHit {
    fn edit_target(&self) -> PreviewEditTarget {
        PreviewEditTarget {
            owner: self.owner_target,
            content_node_id: self.content_node_id,
            spatial_node_id: self.editable_spatial_node_id,
            instance_path: self.instance_path.clone(),
        }
    }
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
                state.cancel_drag();
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
            if let Some(edit_target) = self
                .editor_context
                .interaction
                .preview_edit_target
                .as_ref()
                .filter(|target| self.editor_context.selection.primary() == Some(target.owner))
            {
                if let Some(gc) = visual_for_exact_instance(
                    self.gui_clips,
                    edit_target.content_node_id,
                    edit_target.instance_path.as_slice(),
                ) {
                    if matches!(
                        gc.content_node.content(),
                        library::model::NodeContent::Generator(
                            library::model::GeneratorContent::Shape
                        )
                    ) {
                        if let Some(path_str) = gc.content_node.properties().get_string("path") {
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
                                        edit_target: edit_target.clone(),
                                        node_id: gc.content_id(),
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
                    let target = hit.owner_target;
                    // Started drag on an entity
                    // Ensure it is selected (if not modifier click)
                    // If Shift/Ctrl is held, we might be adding it to selection?
                    // Usually dragging implies selection.
                    // If not selected, select it.
                    let modifiers = self.ui.input(|i| i.modifiers);
                    let action = crate::ui::selection::SelectionAction::from_modifiers(&modifiers);
                    let mut should_drag = true;

                    match action {
                        crate::ui::selection::SelectionAction::Remove => {
                            if self.editor_context.is_selected(target) {
                                self.editor_context.remove_selection(target);
                            }
                            should_drag = false;
                        }
                        crate::ui::selection::SelectionAction::Add
                        | crate::ui::selection::SelectionAction::Toggle => {
                            if !self.editor_context.is_selected(target) {
                                self.editor_context.add_selection(target);
                            }
                        }
                        crate::ui::selection::SelectionAction::Replace => {
                            if !self.editor_context.is_selected(target) {
                                self.editor_context.select_target(target);
                            }
                        }
                    }

                    if should_drag
                        && active_tool == crate::state::context_types::PreviewTool::Select
                        && hit.editable_spatial_node_id.is_some()
                        && self.editor_context.is_selected(target)
                    {
                        self.editor_context.set_primary_selection(target);
                        self.editor_context.interaction.preview_edit_target =
                            Some(hit.edit_target());
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
            if self.editor_context.interaction.is_moving_selected_entity
                && self
                    .editor_context
                    .interaction
                    .body_drag_state
                    .as_ref()
                    .is_some_and(|state| state.has_changed)
            {
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
                    owner_target: gc.owner_target,
                    content_node_id: gc.content_id(),
                    editable_spatial_node_id: gc.editable_spatial_id(),
                    instance_path: gc.instance_path.clone(),
                });
            }
        }
        None
    }

    fn init_drag_state(&mut self, pointer_pos: Option<Pos2>) {
        if let Some(pointer_pos) = pointer_pos {
            let primary = self.editor_context.selection.primary();
            let preview_targets = collect_preview_drag_targets(
                self.gui_clips,
                self.editor_context.selection.targets(),
                primary,
                self.editor_context.interaction.preview_edit_target.as_ref(),
            );
            if preview_targets.is_empty() {
                self.editor_context.interaction.is_moving_selected_entity = false;
                self.editor_context.interaction.body_drag_state = None;
                return;
            }
            self.editor_context.interaction.body_drag_state =
                Some(crate::state::context_types::BodyDragState {
                    start_mouse_pos: pointer_pos,
                    original_positions: std::collections::HashMap::new(),
                    preview_targets,
                    has_changed: false,
                });
        }
    }

    fn handle_click_selection(&mut self, hovered_hit: Option<&PreviewHit>) {
        let hovered_target = hovered_hit.map(|hit| hit.owner_target);
        if self.editor_context.view.active_tool == crate::state::context_types::PreviewTool::Text {
            if let Some(hit) = hovered_hit {
                let id = hit.content_node_id;
                let visual =
                    visual_for_exact_instance(self.gui_clips, id, hit.instance_path.as_slice());
                let is_text = visual.is_some_and(|visual| {
                    matches!(
                        visual.content_node.content(),
                        library::model::NodeContent::Generator(
                            library::model::GeneratorContent::Text
                        )
                    )
                });
                if is_text {
                    self.editor_context.interaction.editing_text_entity_id = Some(id);
                    if let Some(gc) = visual {
                        if let Some(text) = gc.content_node.properties().get_string("text") {
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

        let action = crate::ui::selection::get_click_action(&modifiers, hovered_target);

        match action {
            crate::ui::selection::ClickAction::Select(target) => {
                let edit_target = hovered_hit
                    .filter(|hit| target == hit.owner_target)
                    .map(PreviewHit::edit_target);
                self.editor_context.select_target(target);
                self.editor_context.interaction.preview_edit_target = edit_target;
            }
            crate::ui::selection::ClickAction::Add(target) => {
                let edit_target = hovered_hit
                    .filter(|hit| target == hit.owner_target)
                    .map(PreviewHit::edit_target);
                if !self.editor_context.is_selected(target) {
                    self.editor_context.add_selection(target);
                    self.editor_context.interaction.preview_edit_target = edit_target;
                }
            }
            crate::ui::selection::ClickAction::Remove(target) => {
                if self.editor_context.is_selected(target) {
                    self.editor_context.remove_selection(target);
                }
                self.editor_context.interaction.preview_edit_target = None;
            }
            crate::ui::selection::ClickAction::Toggle(target) => {
                let edit_target = hovered_hit
                    .filter(|hit| target == hit.owner_target)
                    .map(PreviewHit::edit_target);
                self.editor_context.toggle_selection(target);
                self.editor_context.interaction.preview_edit_target = self
                    .editor_context
                    .is_selected(target)
                    .then_some(edit_target)
                    .flatten();
            }
            crate::ui::selection::ClickAction::Clear => {
                self.editor_context.clear_selection();
            }
            crate::ui::selection::ClickAction::DoNothing => {}
        }
    }

    fn handle_drag_move(
        &mut self,
        pointer_pos: Option<Pos2>,
        pending_actions: &mut Vec<PreviewAction>,
    ) {
        let current_zoom = self.editor_context.view.zoom;
        let Some(curr_mouse) = pointer_pos else {
            return;
        };
        if !current_zoom.is_finite() || current_zoom.abs() <= f32::EPSILON {
            return;
        }
        let Some(drag_state) = self.editor_context.interaction.body_drag_state.as_ref() else {
            return;
        };
        let screen_delta = curr_mouse - drag_state.start_mouse_pos;
        let world_delta = screen_delta / current_zoom;
        let stored_targets = drag_state.preview_targets.clone();
        let current_time = self.editor_context.timeline.current_time as f64;
        let mut valid_targets = Vec::with_capacity(stored_targets.len());
        let mut changed = false;

        for target in stored_targets {
            if !self
                .editor_context
                .selection
                .contains(target.edit_target.owner)
            {
                continue;
            }
            let Some(layer) = revalidate_preview_drag_target(self.gui_clips, &target) else {
                continue;
            };
            let Some(local_delta) = inverse_map_vector(layer.parent_transform, world_delta) else {
                // Once identity becomes invalid during a gesture, discard it;
                // a later UUID reuse must not revive this drag route.
                continue;
            };
            valid_targets.push(target.clone());
            if local_delta == egui::Vec2::ZERO {
                continue;
            }
            let Some(spatial_id) = target.edit_target.spatial_node_id else {
                continue;
            };
            let new_x = target.original_position[0] as f64 + local_delta.x as f64;
            let new_y = target.original_position[1] as f64 + local_delta.y as f64;
            pending_actions.push(PreviewAction::UpdateProperty {
                edit_target: target.edit_target.clone(),
                node_id: spatial_id,
                prop_name: "position".to_string(),
                time: current_time,
                value: PropertyValue::Vec2(Vec2 {
                    x: ordered_float::OrderedFloat(new_x),
                    y: ordered_float::OrderedFloat(new_y),
                }),
            });
            changed = true;
        }

        if let Some(drag_state) = &mut self.editor_context.interaction.body_drag_state {
            drag_state.preview_targets = valid_targets;
            drag_state.has_changed |= changed;
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

                    let found_nodes = self.get_owners_in_box(selection_rect);

                    match crate::ui::selection::get_box_action(&modifiers, found_nodes) {
                        crate::ui::selection::BoxAction::Replace(targets) => {
                            let primary = targets.last().copied();
                            self.editor_context.replace_selection(targets, primary);
                        }
                        crate::ui::selection::BoxAction::Add(targets) => {
                            for target in targets {
                                self.editor_context.add_selection(target);
                            }
                        }
                        crate::ui::selection::BoxAction::Remove(targets) => {
                            for target in targets {
                                self.editor_context.remove_selection(target);
                            }
                        }
                    }
                }
                self.editor_context.interaction.preview_selection_drag_start = None;
            }
        }
    }

    fn get_owners_in_box(&self, selection_rect: Rect) -> Vec<SelectionTarget> {
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

            if selection_rect.intersects(clip_screen_rect) && seen.insert(gc.owner_target) {
                found.push(gc.owner_target);
            }
        }
        found
    }

    fn selected_visual(&self, entity_id: Uuid) -> Option<&PreviewClip> {
        let edit_target = self
            .editor_context
            .interaction
            .preview_edit_target
            .as_ref()
            .filter(|target| target.content_node_id == entity_id)
            .filter(|target| self.editor_context.selection.contains(target.owner))?;
        visual_for_exact_instance(
            self.gui_clips,
            entity_id,
            edit_target.instance_path.as_slice(),
        )
    }

    pub fn draw_text_overlay(&mut self, pending_actions: &mut Vec<PreviewAction>) {
        if let Some(id) = self.editor_context.interaction.editing_text_entity_id {
            let edit_target = self
                .editor_context
                .interaction
                .preview_edit_target
                .as_ref()
                .filter(|target| target.content_node_id == id)
                .cloned();
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
                let font_size = gc
                    .content_node
                    .properties()
                    .get_f32("size")
                    .unwrap_or(100.0);

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

                    if let Some(edit_target) = edit_target {
                        pending_actions.push(PreviewAction::UpdateProperty {
                            edit_target,
                            node_id: id,
                            prop_name: "text".to_string(),
                            time: self.editor_context.timeline.current_time as f64,
                            value: PropertyValue::String(text),
                        });
                    }
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

fn collect_preview_drag_targets(
    visuals: &[PreviewClip],
    owners: &[SelectionTarget],
    primary: Option<SelectionTarget>,
    explicit_primary: Option<&PreviewEditTarget>,
) -> Vec<PreviewBodyDragTarget> {
    owners
        .iter()
        .filter_map(|owner| {
            let (target, requires_canonical_owner) = if Some(*owner) == primary {
                (
                    explicit_primary
                        .filter(|target| target.owner == *owner)?
                        .clone(),
                    false,
                )
            } else {
                let OwnerEditTargetResolution::Resolved(target) =
                    resolve_owner_edit_target(visuals, *owner)
                else {
                    return None;
                };
                (target, true)
            };
            preview_drag_target(visuals, target, requires_canonical_owner)
        })
        .collect()
}

fn preview_drag_target(
    visuals: &[PreviewClip],
    edit_target: PreviewEditTarget,
    requires_canonical_owner: bool,
) -> Option<PreviewBodyDragTarget> {
    let layer = validated_spatial_layer(visuals, &edit_target)?;
    if requires_canonical_owner
        && resolve_owner_edit_target(visuals, edit_target.owner)
            != OwnerEditTargetResolution::Resolved(edit_target.clone())
    {
        return None;
    }
    if !is_invertible(layer.parent_transform) {
        return None;
    }
    Some(PreviewBodyDragTarget {
        edit_target,
        original_position: [
            layer.transform.position.x as f32,
            layer.transform.position.y as f32,
        ],
        requires_canonical_owner,
    })
}

fn revalidate_preview_drag_target<'a>(
    visuals: &'a [PreviewClip],
    target: &PreviewBodyDragTarget,
) -> Option<&'a PreviewSpatialLayer> {
    if target.requires_canonical_owner
        && resolve_owner_edit_target(visuals, target.edit_target.owner)
            != OwnerEditTargetResolution::Resolved(target.edit_target.clone())
    {
        return None;
    }
    validated_spatial_layer(visuals, &target.edit_target)
}

fn validated_spatial_layer<'a>(
    visuals: &'a [PreviewClip],
    target: &PreviewEditTarget,
) -> Option<&'a PreviewSpatialLayer> {
    let spatial_id = target.spatial_node_id?;
    let visual = exact_visual_for_edit_target(visuals, target)?;
    visual.spatial_layer(spatial_id)
}

fn inverse_map_vector(
    transform: library::rendering::renderer::Affine2D,
    vector: egui::Vec2,
) -> Option<egui::Vec2> {
    let determinant = transform.scale_x * transform.scale_y - transform.skew_x * transform.skew_y;
    if !determinant.is_finite() || determinant.abs() <= f64::EPSILON {
        return None;
    }
    let mapped = egui::vec2(
        ((transform.scale_y * f64::from(vector.x) - transform.skew_x * f64::from(vector.y))
            / determinant) as f32,
        ((-transform.skew_y * f64::from(vector.x) + transform.scale_x * f64::from(vector.y))
            / determinant) as f32,
    );
    mapped.is_finite().then_some(mapped)
}

fn is_invertible(transform: library::rendering::renderer::Affine2D) -> bool {
    inverse_map_vector(transform, egui::Vec2::ZERO).is_some()
}

#[cfg(test)]
#[path = "interaction_tests.rs"]
mod tests;
