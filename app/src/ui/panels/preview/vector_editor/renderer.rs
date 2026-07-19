use crate::model::vector::VectorEditorState;
use egui::{Color32, Painter, Pos2, Stroke};
use library::model::vector::VectorPath;
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
