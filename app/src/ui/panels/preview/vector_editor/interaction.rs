use crate::model::vector::{VectorDragGesture, VectorEditorState};
use egui::{Pos2, Response, Ui};
use library::model::vector::{
    move_handle, move_vertices, set_point_type, HandleType, PointType, VectorPath,
};
use library::rendering::renderer::Affine2D;

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
}

pub struct VectorEditorInteraction<'a> {
    pub state: &'a mut VectorEditorState,
    /// Ephemeral value derived from the authoritative Project property for
    /// this frame. It is written back through a PreviewAction when changed.
    pub path: &'a mut VectorPath,
    /// The same evaluated local-to-composition transform used by rendering.
    pub transform: Affine2D,
    pub to_screen: Box<dyn Fn(Pos2) -> Pos2 + 'a>,
    pub to_world: Box<dyn Fn(Pos2) -> Pos2 + 'a>,
}

impl<'a> VectorEditorInteraction<'a> {
    /// Return `(changed, captured, commit_requested)`.
    pub fn handle(&mut self, ui: &Ui, response: &Response) -> (bool, bool, bool) {
        let mut changed = false;
        let mut captured = false;
        let mut commit_requested = false;

        let (mode_request, toolbar_owns_pointer) = self.point_mode_toolbar(ui, response);
        captured |= toolbar_owns_pointer;
        if let Some(point_type) = mode_request {
            let before = self.path.clone();
            let selected = sorted_selection(self.state, self.path.points.len());
            set_point_type(self.path, &selected, point_type);
            changed = *self.path != before;
            commit_requested = changed;
            captured = true;
        }

        let pointer = ui.input(|input| PointerFrame {
            position: input.pointer.hover_pos(),
            primary_pressed: input.pointer.primary_pressed(),
            primary_down: input.pointer.primary_down(),
            primary_released: input.pointer.primary_released(),
            shift: input.modifiers.shift,
            alt: input.modifiers.alt,
            escape: input.key_pressed(egui::Key::Escape),
        });

        if pointer.escape {
            if let Some(drag) = self.state.drag.take() {
                if drag.changed {
                    self.path.clone_from(&drag.original_path);
                    changed = true;
                }
                captured = true;
            }
            self.state.selected_handle = None;
            return (changed, captured, false);
        }

        if self.state.drag.is_some() {
            captured = true;
            if let Some(pointer_position) = pointer.position {
                changed |= self.update_drag(pointer_position);
            }
            if pointer.primary_released || !pointer.primary_down {
                let drag = self.state.drag.take();
                commit_requested |= drag.as_ref().is_some_and(|drag| drag.changed);
                self.state.selected_handle = None;
            }
            return (changed, captured, commit_requested);
        }

        let Some(world_to_local) = inverse(self.transform) else {
            return (changed, captured, commit_requested);
        };
        let Some(pointer_position) = pointer.position else {
            return (changed, captured, commit_requested);
        };

        if pointer.primary_pressed && !toolbar_owns_pointer {
            let hit = self.hit_target(pointer_position);
            if let Some(mut hit) = hit {
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
                let pointer_start_local = self.screen_to_local(pointer_position, world_to_local);
                let selected_indices = sorted_selection(self.state, self.path.points.len());
                self.state.selected_handle = Some((hit.point_index, hit.handle));
                self.state.focused_handle = Some((hit.point_index, hit.handle));
                self.state.drag = Some(VectorDragGesture {
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
                captured = true;
            } else if response.rect.contains(pointer_position) && !pointer.shift {
                self.state.selected_point_indices.clear();
                self.state.focused_handle = None;
            }
        }

        (changed, captured, commit_requested)
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

        let world_position = (self.to_world)(pointer_position);
        let (local_x, local_y) = drag
            .world_to_local
            .map_point(f64::from(world_position.x), f64::from(world_position.y));
        let local_delta = [
            local_x as f32 - drag.pointer_start_local[0],
            local_y as f32 - drag.pointer_start_local[1],
        ];
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
            .map(|&index| self.path.points[index].point_type);
        let common_mode = common_mode.filter(|mode| {
            selected
                .iter()
                .all(|&index| self.path.points[index].point_type == *mode)
        });
        let area = egui::Area::new(egui::Id::new("preview.vector.point_modes"))
            .order(egui::Order::Foreground)
            // Preview reserves a 32-point tool strip immediately above its
            // canvas. Keep mode controls there so the toolbar can never cover
            // an authored vertex or intercept its drag.
            .fixed_pos(response.rect.left_top() + egui::vec2(170.0, -30.0))
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Point");
                            let mut requested = None;
                            for (mode, label, id) in [
                                (PointType::Corner, "Corner / Cusp", "corner"),
                                (PointType::Smooth, "Smooth", "smooth"),
                                (PointType::Symmetric, "Symmetric", "symmetric"),
                            ] {
                                let button = ui.selectable_label(common_mode == Some(mode), label);
                                if crate::qa::is_enabled() {
                                    crate::qa::register_component_with_metadata(
                                        format!("preview.vector.mode.{id}"),
                                        "preview_vector_point_mode",
                                        button.rect,
                                        true,
                                        Some(serde_json::json!({
                                            "mode": id,
                                            "selected_point_indices": &selected,
                                            "active": common_mode == Some(mode),
                                            "action": "set_selected_vector_point_mode",
                                        })),
                                    );
                                }
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
        if crate::qa::is_enabled() {
            crate::qa::register_component_with_metadata(
                "preview.vector.point_modes",
                "preview_vector_point_mode_toolbar",
                area.response.rect,
                true,
                Some(serde_json::json!({
                    "selected_point_indices": selected,
                    "action": "choose_vector_point_mode",
                })),
            );
        }
        let owns_pointer = ui
            .input(|input| input.pointer.hover_pos())
            .is_some_and(|pointer| area.response.rect.contains(pointer));
        (area.inner, owns_pointer)
    }

    fn screen_to_local(&self, screen: Pos2, world_to_local: Affine2D) -> Pos2 {
        let world = (self.to_world)(screen);
        let (x, y) = world_to_local.map_point(f64::from(world.x), f64::from(world.y));
        Pos2::new(x as f32, y as f32)
    }

    fn local_to_screen(&self, x: f32, y: f32) -> Pos2 {
        let (world_x, world_y) = self.transform.map_point(f64::from(x), f64::from(y));
        (self.to_screen)(Pos2::new(world_x as f32, world_y as f32))
    }
}

fn update_point_selection(
    state: &mut VectorEditorState,
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

fn sorted_selection(state: &VectorEditorState, point_count: usize) -> Vec<usize> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::vector::ControlPoint;
    use std::collections::HashSet;

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
        state: &mut VectorEditorState,
        path: &mut VectorPath,
        events: Vec<egui::Event>,
    ) -> (bool, bool, bool) {
        run_frame_with_transform(context, state, path, events, Affine2D::IDENTITY)
    }

    fn run_frame_with_transform(
        context: &egui::Context,
        state: &mut VectorEditorState,
        path: &mut VectorPath,
        events: Vec<egui::Event>,
        transform: Affine2D,
    ) -> (bool, bool, bool) {
        let mut result = (false, false, false);
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
                result = VectorEditorInteraction {
                    state,
                    path,
                    transform,
                    to_screen: Box::new(|position| position),
                    to_world: Box::new(|position| position),
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
    fn raw_pointer_drag_moves_only_selected_points_from_frozen_snapshot() {
        let context = egui::Context::default();
        let mut state = VectorEditorState::default();
        let mut path = VectorPath {
            points: vec![point(40.0, 40.0), point(80.0, 40.0), point(120.0, 40.0)],
            is_closed: false,
        };

        // Build the multi-selection through real primary + Shift-primary
        // lifecycles instead of seeding UI state in the fixture.
        for (position, modifiers) in [
            (Pos2::new(40.0, 40.0), egui::Modifiers::NONE),
            (
                Pos2::new(80.0, 40.0),
                egui::Modifiers {
                    shift: true,
                    ..Default::default()
                },
            ),
        ] {
            run_frame(
                &context,
                &mut state,
                &mut path,
                vec![
                    egui::Event::PointerMoved(position),
                    pointer_button(position, true, modifiers),
                ],
            );
            run_frame(
                &context,
                &mut state,
                &mut path,
                vec![pointer_button(position, false, modifiers)],
            );
        }
        assert_eq!(state.selected_point_indices, HashSet::from([0, 1]));

        run_frame(
            &context,
            &mut state,
            &mut path,
            vec![
                egui::Event::PointerMoved(Pos2::new(40.0, 40.0)),
                pointer_button(Pos2::new(40.0, 40.0), true, egui::Modifiers::NONE),
            ],
        );
        let first = run_frame(
            &context,
            &mut state,
            &mut path,
            vec![egui::Event::PointerMoved(Pos2::new(50.0, 45.0))],
        );
        let second = run_frame(
            &context,
            &mut state,
            &mut path,
            vec![egui::Event::PointerMoved(Pos2::new(58.0, 52.0))],
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
        );

        assert_eq!(path.points[0].position, [58.0, 52.0]);
        assert_eq!(path.points[1].position, [98.0, 52.0]);
        assert_eq!(path.points[2].position, [120.0, 40.0]);
        assert_eq!(first, (true, true, false));
        assert_eq!(second, (true, true, false));
        assert_eq!(released, (true, true, true));
        assert!(state.drag.is_none());
    }

    #[test]
    fn zero_length_handles_do_not_steal_vertex_drag() {
        let context = egui::Context::default();
        let mut state = VectorEditorState {
            selected_point_indices: HashSet::from([0]),
            ..Default::default()
        };
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
        );

        assert_eq!(state.selected_handle, Some((0, HandleType::Vertex)));
    }

    #[test]
    fn active_drag_keeps_press_time_coordinate_transform_when_render_geometry_changes() {
        let context = egui::Context::default();
        let mut state = VectorEditorState::default();
        let mut path = VectorPath {
            points: vec![point(40.0, 40.0), point(80.0, 40.0)],
            is_closed: false,
        };

        run_frame_with_transform(
            &context,
            &mut state,
            &mut path,
            vec![
                egui::Event::PointerMoved(Pos2::new(40.0, 40.0)),
                pointer_button(Pos2::new(40.0, 40.0), true, egui::Modifiers::NONE),
            ],
            Affine2D::IDENTITY,
        );
        run_frame_with_transform(
            &context,
            &mut state,
            &mut path,
            vec![egui::Event::PointerMoved(Pos2::new(52.0, 47.0))],
            Affine2D {
                translate_x: 1000.0,
                translate_y: -500.0,
                ..Affine2D::IDENTITY
            },
        );

        assert_eq!(path.points[0].position, [52.0, 47.0]);
        assert_eq!(path.points[1].position, [80.0, 40.0]);
    }

    #[test]
    fn alt_vertex_drag_creates_symmetric_handles_and_requests_one_commit() {
        let context = egui::Context::default();
        let mut state = VectorEditorState::default();
        let mut path = VectorPath {
            points: vec![point(40.0, 40.0), point(80.0, 40.0)],
            is_closed: false,
        };
        let alt = egui::Modifiers {
            alt: true,
            ..Default::default()
        };

        run_frame(
            &context,
            &mut state,
            &mut path,
            vec![
                egui::Event::PointerMoved(Pos2::new(40.0, 40.0)),
                pointer_button(Pos2::new(40.0, 40.0), true, alt),
            ],
        );
        run_frame(
            &context,
            &mut state,
            &mut path,
            vec![egui::Event::PointerMoved(Pos2::new(55.0, 48.0))],
        );
        let released = run_frame(
            &context,
            &mut state,
            &mut path,
            vec![pointer_button(Pos2::new(55.0, 48.0), false, alt)],
        );

        assert_eq!(path.points[0].point_type, PointType::Symmetric);
        assert_eq!(path.points[0].handle_out, [15.0, 8.0]);
        assert_eq!(path.points[0].handle_in, [-15.0, -8.0]);
        assert!(released.2);
    }
}
