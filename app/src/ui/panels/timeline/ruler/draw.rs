use super::ticks;
use crate::state::context::EditorContext;
use egui::{Painter, Rect};

pub fn draw_ruler_marks(
    painter: &Painter,
    rect: Rect,
    _editor_context: &EditorContext,
    scroll_offset_x: f32,
    pixels_per_unit: f32,
    composition_fps: f64,
) {
    let is_frame_display_mode = pixels_per_unit / composition_fps as f32 > 10.0;
    let pixels_per_frame = pixels_per_unit / composition_fps as f32;

    if is_frame_display_mode {
        // --- Frame Mode: Iterate by Frames ---
        let (major_interval_frames, minor_interval_frames, _) =
            ticks::get_frame_intervals(pixels_per_frame, composition_fps as f32);
        let maj_frames = major_interval_frames as i32;
        let min_frames = (minor_interval_frames as i32).max(1);

        let first_visible_frame = (scroll_offset_x / pixels_per_frame).floor() as i32;
        let last_visible_frame =
            ((scroll_offset_x + rect.width()) / pixels_per_frame).ceil() as i32;

        let mut current_frame = (first_visible_frame / min_frames) * min_frames;
        if current_frame < 0 {
            current_frame = 0;
        }

        while current_frame <= last_visible_frame {
            let content_x = current_frame as f32 * pixels_per_frame;
            let screen_x = rect.min.x + (content_x - scroll_offset_x);

            if screen_x >= rect.min.x && screen_x <= rect.max.x {
                // Check for "Real Time" Second Boundary (e.g. Frame 2997 for 100s at 29.97fps)
                let time_sec = current_frame as f64 / composition_fps;
                let nearest_sec = time_sec.round();
                let is_integer_second = (time_sec - nearest_sec).abs() < 0.0001; // Epsilon check

                let is_major_frame = current_frame % maj_frames == 0;

                let (line_start_y, line_end_y, stroke_color, label_text) = if is_integer_second {
                    // It is a second boundary (Priority)
                    (
                        rect.min.y,
                        rect.max.y,
                        egui::Color32::WHITE,
                        Some(format!("{}s", nearest_sec as i64)),
                    )
                } else if is_major_frame {
                    // It is a major frame tick (e.g. 10f)
                    let fps_int = composition_fps.round() as i64;
                    let max_fps_int = if fps_int > 0 { fps_int } else { 1 };
                    let frame_disp = current_frame as i64 % max_fps_int;

                    (
                        rect.min.y + rect.height() * 0.2, // Slightly shorter than second ticks
                        rect.max.y,
                        egui::Color32::from_gray(200),
                        Some(format!("{}f", frame_disp)),
                    )
                } else {
                    // Minor frame tick
                    (
                        rect.max.y - (rect.height() * 0.33),
                        rect.max.y,
                        egui::Color32::from_gray(100),
                        None,
                    )
                };

                painter.line_segment(
                    [
                        egui::pos2(screen_x, line_start_y),
                        egui::pos2(screen_x, line_end_y),
                    ],
                    egui::Stroke::new(1.0, stroke_color),
                );

                if let Some(text) = label_text {
                    painter.text(
                        egui::pos2(screen_x + 3.0, rect.min.y + 2.0),
                        egui::Align2::LEFT_TOP,
                        text,
                        egui::FontId::proportional(9.0),
                        stroke_color,
                    );
                }
            }
            current_frame += min_frames;
        }
    } else {
        // --- Time Mode: Iterate by Seconds (Snapped to Frames) ---
        let nice_steps = &[0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0];
        let (maj_sec, min_sec) = ticks::get_nice_time_intervals(pixels_per_unit, nice_steps, 50.0);

        let first_visible_sec = (scroll_offset_x / pixels_per_unit).max(0.0) as f64;
        let last_visible_sec = ((scroll_offset_x + rect.width()) / pixels_per_unit) as f64;

        // Iterate via integers to avoid accumulation error: i * min_sec
        let start_i = (first_visible_sec / min_sec as f64).floor() as i64;
        let end_i = (last_visible_sec / min_sec as f64).ceil() as i64;

        for i in start_i..=end_i {
            if i < 0 {
                continue;
            }
            let s = i as f64 * min_sec as f64;

            // KEY: Snap time to nearest integer frame
            let frame = (s * composition_fps).round() as i64;
            let content_x = frame as f32 * pixels_per_frame;
            let screen_x = rect.min.x + (content_x - scroll_offset_x);

            if screen_x >= rect.min.x && screen_x <= rect.max.x {
                // Determine major/minor based on 's'
                // Check if 's' is multiple of 'maj_sec'
                // Use integer arithmetic on 'i' relative to (maj/min) ratio
                let step_ratio = (maj_sec / min_sec).round() as i64;
                let is_major = if step_ratio > 0 {
                    i % step_ratio == 0
                } else {
                    true
                };

                let (line_start_y, line_end_y, stroke_color, label_text) = if is_major {
                    // Major Tick (Round Seconds)
                    let sec_int = s.round() as i64;
                    // Only label as "Xs" if it's close to integer second
                    let is_whole_sec = (s - s.round()).abs() < 0.001;
                    let text = if is_whole_sec {
                        format!("{}s", sec_int)
                    } else {
                        // Fallback for weird fractional majors? Should default to frames if needed,
                        // but nice_time_intervals usually returns clean fractions.
                        // Display as frames if not whole second
                        let fps_int = composition_fps.round() as i64;
                        let max_fps_int = if fps_int > 0 { fps_int } else { 1 };
                        let f_rem = frame % max_fps_int;
                        format!("{}f", f_rem)
                    };

                    (rect.min.y, rect.max.y, egui::Color32::WHITE, Some(text))
                } else {
                    (
                        rect.max.y - (rect.height() * 0.33),
                        rect.max.y,
                        egui::Color32::from_gray(150),
                        None,
                    )
                };

                painter.line_segment(
                    [
                        egui::pos2(screen_x, line_start_y),
                        egui::pos2(screen_x, line_end_y),
                    ],
                    egui::Stroke::new(1.0, stroke_color),
                );

                if let Some(text) = label_text {
                    painter.text(
                        egui::pos2(screen_x + 3.0, rect.min.y + 2.0),
                        egui::Align2::LEFT_TOP,
                        text,
                        egui::FontId::proportional(9.0),
                        stroke_color,
                    );
                }
            }
        }
    }
}
