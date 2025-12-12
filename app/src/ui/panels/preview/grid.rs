use egui::{Painter, Rect, Vec2};

pub fn draw_grid(painter: &Painter, rect: Rect, pan: Vec2, zoom: f32) {
    let grid_size = 100.0 * zoom;

    if grid_size > 10.0 {
        let (_cols, _rows) = (
            (rect.width() / grid_size).ceil() as usize + 2,
            (rect.height() / grid_size).ceil() as usize + 2,
        );
        let start_x = rect.min.x + ((pan.x % grid_size) + grid_size) % grid_size;
        let start_y = rect.min.y + ((pan.y % grid_size) + grid_size) % grid_size;
        let grid_color = egui::Color32::from_gray(50);

        // Calculate the first visible line's coordinate for x and y
        let first_visible_x = ((rect.min.x - start_x) / grid_size).floor();
        let first_visible_y = ((rect.min.y - start_y) / grid_size).floor();

        // Draw vertical lines
        for i in (first_visible_x as i32)..=((rect.max.x - start_x) / grid_size).ceil() as i32 {
            let x = start_x + (i as f32) * grid_size;
            painter.line_segment(
                [egui::pos2(x, rect.min.y), egui::pos2(x, rect.max.y)],
                egui::Stroke::new(1.0, grid_color),
            );
        }

        // Draw horizontal lines
        for i in (first_visible_y as i32)..=((rect.max.y - start_y) / grid_size).ceil() as i32 {
            let y = start_y + (i as f32) * grid_size;
            painter.line_segment(
                [egui::pos2(rect.min.x, y), egui::pos2(rect.max.x, y)],
                egui::Stroke::new(1.0, grid_color),
            );
        }
    }
}
