use egui::{Color32, Painter, Pos2, Rect, Stroke, Vec2};
use library::model::vector::{HandleType, PointType, VectorPath};
use library::rendering::renderer::Affine2D;
use pan_zoom_ui::CanvasTransform;

use crate::state::path_editor::PathEditorState;

pub(super) struct PathEditorRenderer<'a> {
    pub state: &'a PathEditorState,
    pub path: &'a VectorPath,
    pub transform: Affine2D,
    pub canvas: CanvasTransform,
}

impl PathEditorRenderer<'_> {
    pub fn draw(&self, painter: &Painter) {
        if self.path.points.len() > 1 {
            for index in 0..self.path.points.len() {
                if !self.path.is_closed && index + 1 == self.path.points.len() {
                    break;
                }
                let current = &self.path.points[index];
                let next = &self.path.points[(index + 1) % self.path.points.len()];
                let p0 = self.local_to_screen(current.position);
                let p3 = self.local_to_screen(next.position);
                let c1 = self.local_to_screen(add(current.position, current.handle_out));
                let c2 = self.local_to_screen(add(next.position, next.handle_in));
                painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                    [p0, c1, c2, p3],
                    false,
                    Color32::TRANSPARENT,
                    Stroke::new(1.5, Color32::from_rgb(46, 145, 255)),
                ));
            }
        }

        for (index, point) in self.path.points.iter().enumerate() {
            let center = self.local_to_screen(point.position);
            let selected = self.state.selected_point_indices.contains(&index);
            if selected {
                let incoming = self.local_to_screen(add(point.position, point.handle_in));
                let outgoing = self.local_to_screen(add(point.position, point.handle_out));
                painter.line_segment([incoming, center], Stroke::new(1.0, Color32::GRAY));
                painter.line_segment([center, outgoing], Stroke::new(1.0, Color32::GRAY));
                self.draw_handle(painter, index, HandleType::In, point.handle_in, incoming);
                self.draw_handle(painter, index, HandleType::Out, point.handle_out, outgoing);
            }

            let color = if selected {
                Color32::RED
            } else {
                Color32::from_rgb(0, 105, 255)
            };
            draw_vertex(painter, center, point.point_type, color, selected);
            crate::qa::register_component_with_metadata(
                format!("preview.vector.point:{index}"),
                "preview_path_point",
                Rect::from_center_size(center, Vec2::splat(24.0)),
                true,
                Some(serde_json::json!({
                    "contour_index": 0,
                    "point_index": index,
                    "point_type": point_type_name(point.point_type),
                    "selected": selected,
                    "local_position": {"x": point.position[0], "y": point.position[1]},
                    "action": "select_or_drag_path_point",
                    "alt_action": "create_symmetric_handles",
                })),
            );
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
        let handle_name = match handle {
            HandleType::In => "in",
            HandleType::Out => "out",
            HandleType::Vertex => return,
        };
        crate::qa::register_component_with_metadata(
            format!("preview.vector.handle_{handle_name}:{point_index}"),
            "preview_path_handle",
            Rect::from_center_size(screen_position, Vec2::splat(20.0)),
            true,
            Some(serde_json::json!({
                "contour_index": 0,
                "point_index": point_index,
                "handle": handle_name,
                "focused": focused,
                "relative": {"x": relative[0], "y": relative[1]},
                "action": "drag_path_handle",
                "alt_action": "break_handle_coupling",
            })),
        );
    }

    fn local_to_screen(&self, point: [f32; 2]) -> Pos2 {
        let (x, y) = self
            .transform
            .map_point(f64::from(point[0]), f64::from(point[1]));
        self.canvas.world_to_screen(Pos2::new(x as f32, y as f32))
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
            painter.add(egui::Shape::convex_polygon(
                vec![
                    center + Vec2::new(0.0, -5.0),
                    center + Vec2::new(5.0, 0.0),
                    center + Vec2::new(0.0, 5.0),
                    center + Vec2::new(-5.0, 0.0),
                ],
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

fn add(left: [f32; 2], right: [f32; 2]) -> [f32; 2] {
    [left[0] + right[0], left[1] + right[1]]
}
