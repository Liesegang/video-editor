use crate::types::VectorEditorState;
use egui::{Pos2, Response, Ui};
use library::project::vector::{HandleType, PointType};
use library::runtime::transform::Transform;

pub(in crate::panels::preview) struct VectorEditorInteraction<'a> {
    pub(in crate::panels::preview) state: &'a mut VectorEditorState,
    pub(in crate::panels::preview) transform: Transform,
    pub(in crate::panels::preview) to_screen: Box<dyn Fn(Pos2) -> Pos2 + 'a>,
    pub(in crate::panels::preview) to_world: Box<dyn Fn(Pos2) -> Pos2 + 'a>, // Screen -> World (still transformed by object)
}

impl<'a> VectorEditorInteraction<'a> {
    pub(in crate::panels::preview) fn handle(
        &mut self,
        ui: &Ui,
        _response: &Response,
    ) -> (bool, bool) {
        // changed, captured
        let mut changed = false;
        let mut captured = false;

        let screen_to_local = |screen_pos: Pos2| -> Pos2 {
            let world_pos = (self.to_world)(screen_pos);
            let wx = world_pos.x - self.transform.position.x as f32;
            let wy = world_pos.y - self.transform.position.y as f32;

            let angle_rad = (self.transform.rotation as f32).to_radians();
            let cos = angle_rad.cos();
            let sin = angle_rad.sin();

            let rx = wx * cos + wy * sin;
            let ry = -wx * sin + wy * cos;

            let sx = self.transform.scale.x as f32 / 100.0;
            let sy = self.transform.scale.y as f32 / 100.0;

            let lx = rx / sx;
            let ly = ry / sy;

            Pos2::new(
                lx + self.transform.anchor.x as f32,
                ly + self.transform.anchor.y as f32,
            )
        };

        let local_to_screen = |x: f32, y: f32| -> Pos2 {
            let lx = x - self.transform.anchor.x as f32;
            let ly = y - self.transform.anchor.y as f32;

            let sx = self.transform.scale.x as f32 / 100.0;
            let sy = self.transform.scale.y as f32 / 100.0;

            let angle_rad = (self.transform.rotation as f32).to_radians();
            let cos = angle_rad.cos();
            let sin = angle_rad.sin();

            let rx = lx * sx * cos - ly * sy * sin;
            let ry = lx * sx * sin + ly * sy * cos;

            let wx = self.transform.position.x as f32 + rx;
            let wy = self.transform.position.y as f32 + ry;

            (self.to_screen)(Pos2::new(wx, wy))
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

        for i in 0..self.state.path.points.len() {
            // Extract position to avoid holding borrow
            let (px, py) = {
                let pt = &self.state.path.points[i];
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

            v_response.context_menu(|ui| {
                ui.label("Point Type");
                if ui
                    .radio_value(
                        &mut self.state.path.points[i].point_type,
                        PointType::Corner,
                        "Corner",
                    )
                    .changed()
                {
                    changed = true;
                }
                if ui
                    .radio_value(
                        &mut self.state.path.points[i].point_type,
                        PointType::Smooth,
                        "Smooth",
                    )
                    .changed()
                {
                    changed = true;
                    // Initialize handles if zero?
                    // Logic handled in update usually, or handled on drag
                }
                if ui
                    .radio_value(
                        &mut self.state.path.points[i].point_type,
                        PointType::Symmetric,
                        "Symmetric",
                    )
                    .changed()
                {
                    changed = true;
                }
            });

            // Handles (Only if selected)
            if self.state.selected_point_indices.contains(&i) {
                // Re-borrow point for handles
                let (h_in, h_out) = {
                    let pt = &self.state.path.points[i];
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
                            self.state.path.points[idx].position = [local_pos.x, local_pos.y];
                        }
                        HandleType::In => {
                            let pt = &mut self.state.path.points[idx];
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
                            let pt = &mut self.state.path.points[idx];
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

        (changed, captured)
    }
}
