use crate::model::vector::VectorEditorState;
use egui::{Color32, Painter, Pos2, Stroke};

pub struct VectorEditorRenderer<'a> {
    pub state: &'a VectorEditorState,
    pub transform: library::core::frame::transform::Transform,
    pub to_screen: Box<dyn Fn(Pos2) -> Pos2 + 'a>,
}

impl<'a> VectorEditorRenderer<'a> {
    pub fn draw(&self, painter: &Painter) {
        let to_screen = &self.to_screen;

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

            to_screen(Pos2::new(wx, wy))
        };

        if self.state.path.points.len() > 1 {
            for i in 0..self.state.path.points.len() {
                let current = &self.state.path.points[i];
                let next_idx = (i + 1) % self.state.path.points.len();

                if !self.state.path.is_closed && i == self.state.path.points.len() - 1 {
                    break;
                }

                let next = &self.state.path.points[next_idx];

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

        for (i, pt) in self.state.path.points.iter().enumerate() {
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

                painter.circle_filled(h_in_screen, 3.0, Color32::WHITE);
                painter.circle_stroke(h_in_screen, 3.0, Stroke::new(1.0, Color32::BLUE));

                painter.circle_filled(h_out_screen, 3.0, Color32::WHITE);
                painter.circle_stroke(h_out_screen, 3.0, Stroke::new(1.0, Color32::BLUE));
            }

            painter.circle_filled(center_screen, 4.0, Color32::WHITE);
            painter.circle_stroke(center_screen, 4.0, Stroke::new(1.0, color));
        }
    }
}
