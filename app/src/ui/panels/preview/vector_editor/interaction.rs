use crate::model::vector::VectorEditorState;
use egui::{Pos2, Response, Ui};
use library::model::vector::{HandleType, PointType, VectorPath};
use library::rendering::renderer::Affine2D;

pub struct VectorEditorInteraction<'a> {
    pub state: &'a mut VectorEditorState,
    /// Ephemeral value derived from the authoritative Project property for
    /// this frame. It is written back through a PreviewAction when changed.
    pub path: &'a mut VectorPath,
    /// The same evaluated local-to-composition transform used by rendering.
    pub transform: Affine2D,
    pub to_screen: Box<dyn Fn(Pos2) -> Pos2 + 'a>,
    pub to_world: Box<dyn Fn(Pos2) -> Pos2 + 'a>, // Screen -> World (still transformed by object)
}

impl<'a> VectorEditorInteraction<'a> {
    pub fn handle(&mut self, ui: &Ui, _response: &Response) -> (bool, bool, bool) {
        // changed, captured, commit_requested
        let mut changed = false;
        let mut captured = false;
        let mut commit_requested = false;
        let Some(world_to_local) = inverse(self.transform) else {
            return (changed, captured, commit_requested);
        };

        let screen_to_local = |screen_pos: Pos2| -> Pos2 {
            let world_pos = (self.to_world)(screen_pos);
            let (local_x, local_y) =
                world_to_local.map_point(f64::from(world_pos.x), f64::from(world_pos.y));
            Pos2::new(local_x as f32, local_y as f32)
        };

        let local_to_screen = |x: f32, y: f32| -> Pos2 {
            let (world_x, world_y) = self.transform.map_point(f64::from(x), f64::from(y));
            (self.to_screen)(Pos2::new(world_x as f32, world_y as f32))
        };

        let hit_radius = 12.0;

        // Iterate keys to avoid borrow checker issues when mutating state in loop
        // Standard loop structure is tricky with ui.interact and state mutation
        // We will collect interaction results

        enum InteractionEvent {
            Select(usize, HandleType),
            Move(usize, HandleType, Pos2),
        }
        let mut events = Vec::new();

        for i in 0..self.path.points.len() {
            // Extract position to avoid holding borrow
            let (px, py) = {
                let pt = &self.path.points[i];
                (pt.position[0], pt.position[1])
            };

            let center_screen = local_to_screen(px, py);

            // Vertices
            let v_rect =
                egui::Rect::from_center_size(center_screen, egui::Vec2::splat(hit_radius * 2.0));
            let v_id = ui.make_persistent_id(format!("vert_{}", i));
            let v_response = ui.interact(v_rect, v_id, egui::Sense::drag());

            if v_response.dragged() {
                captured = true;
                if let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos()) {
                    let local_pos = screen_to_local(mouse_pos);
                    events.push(InteractionEvent::Move(i, HandleType::Vertex, local_pos));
                }
                // Auto-select on drag
                if !self.state.selected_point_indices.contains(&i) {
                    events.push(InteractionEvent::Select(i, HandleType::Vertex));
                }
                self.state.selected_handle = Some((i, HandleType::Vertex));
            } else if v_response.clicked() {
                captured = true;
                events.push(InteractionEvent::Select(i, HandleType::Vertex));
            }
            commit_requested |= v_response.drag_stopped();

            v_response.context_menu(|ui| {
                ui.label("Point Type");
                if ui
                    .radio_value(
                        &mut self.path.points[i].point_type,
                        PointType::Corner,
                        "Corner",
                    )
                    .changed()
                {
                    changed = true;
                    commit_requested = true;
                }
                if ui
                    .radio_value(
                        &mut self.path.points[i].point_type,
                        PointType::Smooth,
                        "Smooth",
                    )
                    .changed()
                {
                    changed = true;
                    commit_requested = true;
                    // Initialize handles if zero?
                    // Logic handled in update usually, or handled on drag
                }
                if ui
                    .radio_value(
                        &mut self.path.points[i].point_type,
                        PointType::Symmetric,
                        "Symmetric",
                    )
                    .changed()
                {
                    changed = true;
                    commit_requested = true;
                }
            });

            // Handles (Only if selected)
            if self.state.selected_point_indices.contains(&i) {
                // Re-borrow point for handles
                let (h_in, h_out) = {
                    let pt = &self.path.points[i];
                    (pt.handle_in, pt.handle_out)
                };

                let h_in_screen = local_to_screen(px + h_in[0], py + h_in[1]);
                let h_out_screen = local_to_screen(px + h_out[0], py + h_out[1]);

                let in_rect =
                    egui::Rect::from_center_size(h_in_screen, egui::Vec2::splat(hit_radius * 2.0));
                let in_id = ui.make_persistent_id(format!("in_{}", i));
                let in_response = ui.interact(in_rect, in_id, egui::Sense::drag());

                if in_response.dragged() {
                    captured = true;
                    if let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos()) {
                        let local_pos = screen_to_local(mouse_pos);
                        events.push(InteractionEvent::Move(i, HandleType::In, local_pos));
                    }
                    self.state.selected_handle = Some((i, HandleType::In));
                }
                commit_requested |= in_response.drag_stopped();

                let out_rect =
                    egui::Rect::from_center_size(h_out_screen, egui::Vec2::splat(hit_radius * 2.0));
                let out_id = ui.make_persistent_id(format!("out_{}", i));
                let out_response = ui.interact(out_rect, out_id, egui::Sense::drag());

                if out_response.dragged() {
                    captured = true;
                    if let Some(mouse_pos) = ui.input(|i| i.pointer.hover_pos()) {
                        let local_pos = screen_to_local(mouse_pos);
                        events.push(InteractionEvent::Move(i, HandleType::Out, local_pos));
                    }
                    self.state.selected_handle = Some((i, HandleType::Out));
                }
                commit_requested |= out_response.drag_stopped();
            }
        }

        // Apply Events
        for event in events {
            match event {
                InteractionEvent::Select(idx, h_type) => {
                    self.state.selected_handle = Some((idx, h_type));
                    if h_type == HandleType::Vertex {
                        if !ui.input(|i| i.modifiers.shift) {
                            self.state.selected_point_indices.clear();
                        }
                        self.state.selected_point_indices.insert(idx);
                    }
                }
                InteractionEvent::Move(idx, h_type, local_pos) => {
                    changed = true;
                    match h_type {
                        HandleType::Vertex => {
                            self.path.points[idx].position = [local_pos.x, local_pos.y];
                        }
                        HandleType::In => {
                            let pt = &mut self.path.points[idx];
                            pt.handle_in =
                                [local_pos.x - pt.position[0], local_pos.y - pt.position[1]];

                            if pt.point_type == PointType::Symmetric {
                                pt.handle_out = [-pt.handle_in[0], -pt.handle_in[1]];
                            } else if pt.point_type == PointType::Smooth {
                                let len_out =
                                    (pt.handle_out[0].powi(2) + pt.handle_out[1].powi(2)).sqrt();
                                if len_out > 0.001 {
                                    let len_in =
                                        (pt.handle_in[0].powi(2) + pt.handle_in[1].powi(2)).sqrt();
                                    if len_in > 0.001 {
                                        pt.handle_out = [
                                            -pt.handle_in[0] / len_in * len_out,
                                            -pt.handle_in[1] / len_in * len_out,
                                        ];
                                    }
                                }
                            }
                        }
                        HandleType::Out => {
                            let pt = &mut self.path.points[idx];
                            pt.handle_out =
                                [local_pos.x - pt.position[0], local_pos.y - pt.position[1]];

                            if pt.point_type == PointType::Symmetric {
                                pt.handle_in = [-pt.handle_out[0], -pt.handle_out[1]];
                            } else if pt.point_type == PointType::Smooth {
                                let len_in =
                                    (pt.handle_in[0].powi(2) + pt.handle_in[1].powi(2)).sqrt();
                                if len_in > 0.001 {
                                    let len_out = (pt.handle_out[0].powi(2)
                                        + pt.handle_out[1].powi(2))
                                    .sqrt();
                                    if len_out > 0.001 {
                                        pt.handle_in = [
                                            -pt.handle_out[0] / len_out * len_in,
                                            -pt.handle_out[1] / len_out * len_in,
                                        ];
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if ui.input(|i| i.pointer.any_released()) {
            self.state.selected_handle = None;
        }

        (changed, captured, commit_requested)
    }
}

fn inverse(transform: Affine2D) -> Option<Affine2D> {
    let determinant = transform.scale_x * transform.scale_y - transform.skew_x * transform.skew_y;
    if determinant.abs() <= f64::EPSILON {
        return None;
    }

    let scale_x = transform.scale_y / determinant;
    let skew_x = -transform.skew_x / determinant;
    let skew_y = -transform.skew_y / determinant;
    let scale_y = transform.scale_x / determinant;
    Some(Affine2D {
        scale_x,
        skew_x,
        translate_x: -scale_x * transform.translate_x - skew_x * transform.translate_y,
        skew_y,
        scale_y,
        translate_y: -skew_y * transform.translate_x - scale_y * transform.translate_y,
    })
}
