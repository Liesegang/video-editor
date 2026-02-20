pub(super) mod flatten;

/// Converts a screen position to timeline coordinates (frame number and row index).
pub(super) fn pos_to_timeline_location(
    pos: egui::Pos2,
    content_rect: egui::Rect,
    scroll_offset: egui::Vec2,
    pixels_per_unit: f32,
    composition_fps: f64,
    row_height: f32,
    track_spacing: f32,
) -> (u64, usize) {
    let local_x = pos.x - content_rect.min.x + scroll_offset.x;
    let time_at_pos = (local_x / pixels_per_unit).max(0.0) as f64;
    let frame = (time_at_pos * composition_fps).round() as u64;

    let local_y = pos.y - content_rect.min.y + scroll_offset.y;
    let row_index = (local_y / (row_height + track_spacing)).floor().max(0.0) as usize;

    (frame, row_index)
}
