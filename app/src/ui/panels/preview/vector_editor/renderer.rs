use crate::model::vector::VectorEditorState;
use egui::{Color32, Painter, Pos2, Rect, Stroke, Vec2};
use library::model::vector::{HandleType, PointType, VectorPath};
use library::rendering::renderer::Affine2D;

pub struct VectorEditorRenderer<'a> {
    pub state: &'a VectorEditorState,
    pub path: &'a VectorPath,
    /// The same evaluated local-to-composition transform used by rendering.
    pub transform: Affine2D,
    pub to_screen: Box<dyn Fn(Pos2) -> Pos2 + 'a>,
}

impl<'a> VectorEditorRenderer<'a> {
    pub fn draw(&self, painter: &Painter) {
        let to_screen = &self.to_screen;

        let local_to_screen = |x: f32, y: f32| -> Pos2 {
            let (world_x, world_y) = self.transform.map_point(f64::from(x), f64::from(y));
            to_screen(Pos2::new(world_x as f32, world_y as f32))
        };

        if self.path.points.len() > 1 {
            for i in 0..self.path.points.len() {
                let current = &self.path.points[i];
                let next_idx = (i + 1) % self.path.points.len();

                if !self.path.is_closed && i == self.path.points.len() - 1 {
                    break;
                }

                let next = &self.path.points[next_idx];

                let p0 = local_to_screen(current.position[0], current.position[1]);
                let p3 = local_to_screen(next.position[0], next.position[1]);

                let c1 = local_to_screen(
                    current.position[0] + current.handle_out[0],
                    current.position[1] + current.handle_out[1],
                );
                let c2 = local_to_screen(
                    next.position[0] + next.handle_in[0],
                    next.position[1] + next.handle_in[1],
                );

                let shape = egui::epaint::CubicBezierShape::from_points_stroke(
                    [p0, c1, c2, p3],
                    false,
                    Color32::TRANSPARENT,
                    Stroke::new(1.0, Color32::from_rgb(0, 100, 255)),
                );
                painter.add(shape);
            }
        }

        for (i, pt) in self.path.points.iter().enumerate() {
            let center_screen = local_to_screen(pt.position[0], pt.position[1]);
            let is_selected = self.state.selected_point_indices.contains(&i);

            let color = if is_selected {
                Color32::RED
            } else {
                Color32::BLUE
            };

            if is_selected {
                let h_in_screen = local_to_screen(
                    pt.position[0] + pt.handle_in[0],
                    pt.position[1] + pt.handle_in[1],
                );
                let h_out_screen = local_to_screen(
                    pt.position[0] + pt.handle_out[0],
                    pt.position[1] + pt.handle_out[1],
                );

                painter.line_segment(
                    [h_in_screen, center_screen],
                    Stroke::new(1.0, Color32::GRAY),
                );
                painter.line_segment(
                    [center_screen, h_out_screen],
                    Stroke::new(1.0, Color32::GRAY),
                );

                self.draw_handle(painter, i, HandleType::In, pt.handle_in, h_in_screen);
                self.draw_handle(painter, i, HandleType::Out, pt.handle_out, h_out_screen);
            }

            draw_vertex(painter, center_screen, pt.point_type, color, is_selected);
            if crate::qa::is_enabled() {
                crate::qa::register_component_with_metadata(
                    format!("preview.vector.point:{i}"),
                    "preview_vector_point",
                    Rect::from_center_size(center_screen, Vec2::splat(24.0)),
                    true,
                    Some(serde_json::json!({
                        "point_index": i,
                        "point_type": point_type_name(pt.point_type),
                        "selected": is_selected,
                        "local_position": {"x": pt.position[0], "y": pt.position[1]},
                        "action": "select_or_drag_vector_point",
                    })),
                );
            }
        }
    }

    fn draw_handle(
        &self,
        painter: &Painter,
        point_index: usize,
        handle: HandleType,
        relative: [f32; 2],
        screen_position: Pos2,
    ) {
        if relative[0].hypot(relative[1]) <= 1.0e-3 {
            return;
        }
        let focused = self.state.focused_handle == Some((point_index, handle));
        let fill = if focused {
            Color32::from_rgb(255, 190, 40)
        } else {
            Color32::WHITE
        };
        painter.circle_filled(screen_position, 4.0, fill);
        painter.circle_stroke(
            screen_position,
            4.0,
            Stroke::new(1.5, Color32::from_rgb(0, 130, 255)),
        );
        if crate::qa::is_enabled() {
            let handle_name = match handle {
                HandleType::In => "in",
                HandleType::Out => "out",
                HandleType::Vertex => return,
            };
            crate::qa::register_component_with_metadata(
                format!("preview.vector.handle_{handle_name}:{point_index}"),
                "preview_vector_handle",
                Rect::from_center_size(screen_position, Vec2::splat(20.0)),
                true,
                Some(serde_json::json!({
                    "point_index": point_index,
                    "handle": handle_name,
                    "focused": focused,
                    "relative": {"x": relative[0], "y": relative[1]},
                    "action": "drag_vector_handle",
                    "alt_action": "break_handle_coupling",
                })),
            );
        }
    }
}

fn draw_vertex(
    painter: &Painter,
    center: Pos2,
    point_type: PointType,
    stroke_color: Color32,
    selected: bool,
) {
    let fill = if selected {
        Color32::from_rgb(255, 245, 210)
    } else {
        Color32::WHITE
    };
    match point_type {
        PointType::Corner => {
            let rect = Rect::from_center_size(center, Vec2::splat(8.0));
            painter.rect_filled(rect, 0.0, fill);
            painter.rect_stroke(
                rect,
                0.0,
                Stroke::new(1.5, stroke_color),
                egui::StrokeKind::Middle,
            );
        }
        PointType::Smooth => {
            painter.circle_filled(center, 4.5, fill);
            painter.circle_stroke(center, 4.5, Stroke::new(1.5, stroke_color));
        }
        PointType::Symmetric => {
            let points = vec![
                center + Vec2::new(0.0, -5.0),
                center + Vec2::new(5.0, 0.0),
                center + Vec2::new(0.0, 5.0),
                center + Vec2::new(-5.0, 0.0),
            ];
            painter.add(egui::Shape::convex_polygon(
                points,
                fill,
                Stroke::new(1.5, stroke_color),
            ));
        }
    }
}

const fn point_type_name(point_type: PointType) -> &'static str {
    match point_type {
        PointType::Corner => "corner",
        PointType::Smooth => "smooth",
        PointType::Symmetric => "symmetric",
    }
}
