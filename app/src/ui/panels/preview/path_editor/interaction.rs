use egui::{Pos2, Response, Ui};
use egui_phosphor::regular as icons;
use library::model::vector::{
    move_handle, move_vertices, set_point_type, HandleType, PointType, VectorPath,
};
use library::rendering::renderer::Affine2D;
use pan_zoom_ui::CanvasTransform;

use crate::state::path_editor::{PathDragGesture, PathEditorState};

const VERTEX_HIT_RADIUS: f32 = 12.0;
const HANDLE_HIT_RADIUS: f32 = 10.0;
const HANDLE_VISIBLE_EPSILON: f32 = 1.0e-3;
const DRAG_THRESHOLD_POINTS: f32 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct HitTarget {
    point_index: usize,
    handle: HandleType,
}

#[derive(Clone, Copy, Debug, Default)]
struct PointerFrame {
    position: Option<Pos2>,
    primary_pressed: bool,
    primary_down: bool,
    primary_released: bool,
    shift: bool,
    alt: bool,
    escape: bool,
    space: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct InteractionResult {
    pub changed: bool,
    pub captured: bool,
    pub commit_requested: bool,
}

pub(super) struct PathEditorInteraction<'a> {
    pub state: &'a mut PathEditorState,
    /// Ephemeral projection of the authoritative Project Path for this frame.
    pub path: &'a mut VectorPath,
    /// Exact evaluated local-to-composition transform used by rendering.
    pub transform: Affine2D,
    /// Shared Preview camera used by content, grid, hit testing, and overlay.
    pub canvas: CanvasTransform,
}

impl PathEditorInteraction<'_> {
    pub fn handle(&mut self, ui: &Ui, response: &Response) -> InteractionResult {
        let mut result = InteractionResult::default();
        let (mode_request, toolbar_owns_pointer) = self.point_mode_toolbar(ui, response);
        result.captured |= toolbar_owns_pointer;
        if let Some(point_type) = mode_request {
            let before = self.path.clone();
            let selected = sorted_selection(self.state, self.path.points.len());
            set_point_type(self.path, &selected, point_type);
            result.changed = *self.path != before;
            result.commit_requested = result.changed;
            result.captured = true;
        }

        let pointer = ui.input(|input| PointerFrame {
            position: input.pointer.hover_pos(),
            primary_pressed: input.pointer.primary_pressed(),
            primary_down: input.pointer.primary_down(),
            primary_released: input.pointer.primary_released(),
            shift: input.modifiers.shift,
            alt: input.modifiers.alt,
            escape: input.key_pressed(egui::Key::Escape),
            space: input.key_down(egui::Key::Space),
        });

        if pointer.escape {
            if let Some(drag) = self.state.drag.take() {
                if drag.changed {
                    self.path.clone_from(&drag.original_path);
                    result.changed = true;
                }
                result.captured = true;
            }
            self.state.selected_handle = None;
            return result;
        }

        if self.state.drag.is_some() {
            result.captured = true;
            if let Some(pointer_position) = pointer.position {
                result.changed |= self.update_drag(pointer_position);
            }
            if pointer.primary_released || !pointer.primary_down {
                let drag = self.state.drag.take();
                result.commit_requested |= drag.as_ref().is_some_and(|drag| drag.changed);
                self.state.selected_handle = None;
            }
            return result;
        }

        let Some(world_to_local) = self.transform.inverse() else {
            return result;
        };
        let Some(pointer_position) = pointer.position else {
            return result;
        };

        if pointer.primary_pressed && !toolbar_owns_pointer && !pointer.space {
            if let Some(mut hit) = self.hit_target(pointer_position) {
                update_point_selection(
                    self.state,
                    hit.point_index,
                    pointer.shift,
                    self.path.points.len(),
                );
                let create_handles = hit.handle == HandleType::Vertex && pointer.alt;
                if create_handles {
                    hit.handle = HandleType::Out;
                }
                let Some(pointer_start_local) =
                    self.screen_to_local(pointer_position, world_to_local)
                else {
                    return result;
                };
                let selected_indices = sorted_selection(self.state, self.path.points.len());
                self.state.selected_handle = Some((hit.point_index, hit.handle));
                self.state.focused_handle = Some((hit.point_index, hit.handle));
                self.state.drag = Some(PathDragGesture {
                    target: (hit.point_index, hit.handle),
                    original_path: self.path.clone(),
                    selected_indices,
                    pointer_start_screen: [pointer_position.x, pointer_position.y],
                    pointer_start_local: [pointer_start_local.x, pointer_start_local.y],
                    world_to_local,
                    break_coupling: pointer.alt && !create_handles,
                    create_handles,
                    changed: false,
                });
                result.captured = true;
            } else if response.rect.contains(pointer_position) && !pointer.shift {
                self.state.selected_point_indices.clear();
                self.state.focused_handle = None;
            }
        }

        result
    }

    fn update_drag(&mut self, pointer_position: Pos2) -> bool {
        let Some(drag) = self.state.drag.as_mut() else {
            return false;
        };
        let screen_delta = [
            pointer_position.x - drag.pointer_start_screen[0],
            pointer_position.y - drag.pointer_start_screen[1],
        ];
        if screen_delta[0].hypot(screen_delta[1]) < DRAG_THRESHOLD_POINTS {
            return false;
        }

        let Some(world_position) = self.canvas.screen_to_world(pointer_position) else {
            return false;
        };
        let (local_x, local_y) = drag
            .world_to_local
            .map_point(f64::from(world_position.x), f64::from(world_position.y));
        let local_delta = [
            local_x as f32 - drag.pointer_start_local[0],
            local_y as f32 - drag.pointer_start_local[1],
        ];
        if !local_delta.into_iter().all(f32::is_finite) {
            return false;
        }
        self.path.clone_from(&drag.original_path);

        let (point_index, handle) = drag.target;
        if handle == HandleType::Vertex {
            move_vertices(self.path, &drag.selected_indices, local_delta);
        } else if let Some(point) = self.path.points.get_mut(point_index) {
            let original = &drag.original_path.points[point_index];
            let original_handle = match handle {
                HandleType::In => original.handle_in,
                HandleType::Out => original.handle_out,
                HandleType::Vertex => return false,
            };
            if drag.create_handles {
                point.point_type = PointType::Symmetric;
            }
            move_handle(
                point,
                handle,
                [
                    original_handle[0] + local_delta[0],
                    original_handle[1] + local_delta[1],
                ],
                drag.break_coupling,
            );
        }
        drag.changed = *self.path != drag.original_path;
        drag.changed
    }

    fn hit_target(&self, pointer: Pos2) -> Option<HitTarget> {
        let mut nearest_handle = None;
        let mut nearest_handle_distance = HANDLE_HIT_RADIUS;
        for point_index in sorted_selection(self.state, self.path.points.len()) {
            let point = &self.path.points[point_index];
            for (handle, relative) in [
                (HandleType::In, point.handle_in),
                (HandleType::Out, point.handle_out),
            ] {
                if vector_length(relative) <= HANDLE_VISIBLE_EPSILON {
                    continue;
                }
                let position = self.local_to_screen(
                    point.position[0] + relative[0],
                    point.position[1] + relative[1],
                );
                let distance = position.distance(pointer);
                if distance <= nearest_handle_distance {
                    nearest_handle_distance = distance;
                    nearest_handle = Some(HitTarget {
                        point_index,
                        handle,
                    });
                }
            }
        }
        if nearest_handle.is_some() {
            return nearest_handle;
        }

        self.path
            .points
            .iter()
            .enumerate()
            .filter_map(|(point_index, point)| {
                let distance = self
                    .local_to_screen(point.position[0], point.position[1])
                    .distance(pointer);
                (distance <= VERTEX_HIT_RADIUS).then_some((distance, point_index))
            })
            .min_by(|left, right| left.0.total_cmp(&right.0))
            .map(|(_, point_index)| HitTarget {
                point_index,
                handle: HandleType::Vertex,
            })
    }

    fn point_mode_toolbar(&mut self, ui: &Ui, response: &Response) -> (Option<PointType>, bool) {
        let selected = sorted_selection(self.state, self.path.points.len());
        if selected.is_empty() {
            return (None, false);
        }
        let common_mode = selected
            .first()
            .map(|&index| self.path.points[index].point_type)
            .filter(|mode| {
                selected
                    .iter()
                    .all(|&index| self.path.points[index].point_type == *mode)
            });
        let area = egui::Area::new(egui::Id::new("preview.path.point_modes"))
            .order(egui::Order::Foreground)
            .fixed_pos(response.rect.left_top() + egui::vec2(166.0, -31.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::NONE
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.style_mut().spacing.item_spacing = egui::vec2(3.0, 0.0);
                            let mut requested = None;
                            for (mode, icon, label, id) in [
                                (PointType::Corner, icons::SQUARE, "Corner / Cusp", "corner"),
                                (PointType::Smooth, icons::CIRCLE, "Smooth", "smooth"),
                                (
                                    PointType::Symmetric,
                                    icons::DIAMOND,
                                    "Symmetric",
                                    "symmetric",
                                ),
                            ] {
                                let button = ui
                                    .add(
                                        egui::Button::new(egui::RichText::new(icon).size(16.0))
                                            .selected(common_mode == Some(mode)),
                                    )
                                    .on_hover_text(label);
                                crate::qa::register_component_with_metadata(
                                    format!("preview.vector.mode.{id}"),
                                    "preview_path_point_mode",
                                    button.rect,
                                    button.enabled(),
                                    Some(serde_json::json!({
                                        "mode": id,
                                        "selected_point_indices": &selected,
                                        "active": common_mode == Some(mode),
                                        "action": "set_selected_path_point_mode",
                                    })),
                                );
                                if button.clicked() {
                                    requested = Some(mode);
                                }
                            }
                            requested
                        })
                        .inner
                    })
                    .inner
            });
        crate::qa::register_component_with_metadata(
            "preview.vector.point_modes",
            "preview_path_point_mode_toolbar",
            area.response.rect,
            true,
            Some(serde_json::json!({
                "selected_point_indices": selected,
                "action": "choose_path_point_mode",
            })),
        );
        let owns_pointer = ui
            .input(|input| input.pointer.hover_pos())
            .is_some_and(|pointer| area.response.rect.contains(pointer));
        (area.inner, owns_pointer)
    }

    fn screen_to_local(&self, screen: Pos2, world_to_local: Affine2D) -> Option<Pos2> {
        let world = self.canvas.screen_to_world(screen)?;
        let (x, y) = world_to_local.map_point(f64::from(world.x), f64::from(world.y));
        (x.is_finite() && y.is_finite()).then(|| Pos2::new(x as f32, y as f32))
    }

    fn local_to_screen(&self, x: f32, y: f32) -> Pos2 {
        let (world_x, world_y) = self.transform.map_point(f64::from(x), f64::from(y));
        self.canvas
            .world_to_screen(Pos2::new(world_x as f32, world_y as f32))
    }
}

fn update_point_selection(
    state: &mut PathEditorState,
    point_index: usize,
    extend: bool,
    point_count: usize,
) {
    state
        .selected_point_indices
        .retain(|index| *index < point_count);
    if !extend && !state.selected_point_indices.contains(&point_index) {
        state.selected_point_indices.clear();
    }
    state.selected_point_indices.insert(point_index);
}

fn sorted_selection(state: &PathEditorState, point_count: usize) -> Vec<usize> {
    let mut selected = state
        .selected_point_indices
        .iter()
        .copied()
        .filter(|index| *index < point_count)
        .collect::<Vec<_>>();
    selected.sort_unstable();
    selected
}

fn vector_length(vector: [f32; 2]) -> f32 {
    vector[0].hypot(vector[1])
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::vector::ControlPoint;

    fn point(x: f32, y: f32) -> ControlPoint {
        ControlPoint {
            position: [x, y],
            handle_in: [0.0, 0.0],
            handle_out: [0.0, 0.0],
            point_type: PointType::Corner,
        }
    }

    fn run_frame(
        context: &egui::Context,
        state: &mut PathEditorState,
        path: &mut VectorPath,
        events: Vec<egui::Event>,
        transform: Affine2D,
    ) -> InteractionResult {
        let mut result = InteractionResult::default();
        let modifiers = events
            .iter()
            .find_map(|event| match event {
                egui::Event::PointerButton { modifiers, .. } => Some(*modifiers),
                _ => None,
            })
            .unwrap_or_default();
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 300.0),
            )),
            modifiers,
            events,
            ..Default::default()
        };
        drop(context.run(input, |context| {
            egui::CentralPanel::default().show(context, |ui| {
                let response = ui.allocate_rect(ui.max_rect(), egui::Sense::click_and_drag());
                result = PathEditorInteraction {
                    state,
                    path,
                    transform,
                    canvas: CanvasTransform::new(
                        egui::Pos2::ZERO,
                        pan_zoom_ui::CanvasState::uniform(egui::Vec2::ZERO, 1.0),
                    ),
                }
                .handle(ui, &response);
            });
        }));
        result
    }

    fn pointer_button(position: Pos2, pressed: bool, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        }
    }

    #[test]
    fn drag_uses_frozen_origin_and_requests_one_commit_on_release() {
        let context = egui::Context::default();
        let mut state = PathEditorState::default();
        let mut path = VectorPath {
            points: vec![point(40.0, 40.0), point(80.0, 40.0)],
            is_closed: false,
        };
        run_frame(
            &context,
            &mut state,
            &mut path,
            vec![
                egui::Event::PointerMoved(Pos2::new(40.0, 40.0)),
                pointer_button(Pos2::new(40.0, 40.0), true, egui::Modifiers::NONE),
            ],
            Affine2D::IDENTITY,
        );
        let moved = run_frame(
            &context,
            &mut state,
            &mut path,
            vec![egui::Event::PointerMoved(Pos2::new(58.0, 52.0))],
            Affine2D::IDENTITY,
        );
        let released = run_frame(
            &context,
            &mut state,
            &mut path,
            vec![pointer_button(
                Pos2::new(58.0, 52.0),
                false,
                egui::Modifiers::NONE,
            )],
            Affine2D::IDENTITY,
        );

        assert_eq!(path.points[0].position, [58.0, 52.0]);
        assert_eq!(path.points[1].position, [80.0, 40.0]);
        assert!(moved.changed && !moved.commit_requested);
        assert!(released.changed && released.commit_requested);
        assert!(state.drag.is_none());
    }

    #[test]
    fn evaluated_transform_is_inverted_for_local_vertex_motion() {
        let context = egui::Context::default();
        let mut state = PathEditorState::default();
        let mut path = VectorPath {
            points: vec![point(10.0, 20.0)],
            is_closed: false,
        };
        let transform = Affine2D::translate(100.0, 40.0).compose(Affine2D::scale(2.0, 4.0));
        let start = Pos2::new(120.0, 120.0);
        run_frame(
            &context,
            &mut state,
            &mut path,
            vec![
                egui::Event::PointerMoved(start),
                pointer_button(start, true, egui::Modifiers::NONE),
            ],
            transform,
        );
        run_frame(
            &context,
            &mut state,
            &mut path,
            vec![egui::Event::PointerMoved(start + egui::vec2(20.0, 40.0))],
            transform,
        );
        assert_eq!(path.points[0].position, [20.0, 30.0]);
    }

    #[test]
    fn escape_restores_ephemeral_path_without_commit() {
        let context = egui::Context::default();
        let mut state = PathEditorState::default();
        let mut path = VectorPath {
            points: vec![point(40.0, 40.0)],
            is_closed: false,
        };
        run_frame(
            &context,
            &mut state,
            &mut path,
            vec![
                egui::Event::PointerMoved(Pos2::new(40.0, 40.0)),
                pointer_button(Pos2::new(40.0, 40.0), true, egui::Modifiers::NONE),
            ],
            Affine2D::IDENTITY,
        );
        run_frame(
            &context,
            &mut state,
            &mut path,
            vec![egui::Event::PointerMoved(Pos2::new(70.0, 60.0))],
            Affine2D::IDENTITY,
        );
        let canceled = run_frame(
            &context,
            &mut state,
            &mut path,
            vec![egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: Some(egui::Key::Escape),
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            Affine2D::IDENTITY,
        );
        assert_eq!(path.points[0].position, [40.0, 40.0]);
        assert!(canceled.changed && !canceled.commit_requested);
        assert!(state.drag.is_none());
    }
}
