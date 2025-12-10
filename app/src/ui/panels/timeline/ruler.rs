use crate::model::ui_types::TimelineDisplayMode;
use crate::state::context::EditorContext;
use egui::Ui;
use library::model::project::project::Project;
use library::service::project_service::ProjectService;
use std::sync::{Arc, RwLock};

pub fn show_timeline_ruler(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    _project_service: &ProjectService,
    _project: &Arc<RwLock<Project>>,
    pixels_per_unit: f32,
    scroll_offset_x: f32,
    composition_fps: f64,
) {
    ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |h_ui| {
        let time_display_response = h_ui
            .vertical(|ui| {
                ui.set_width(100.0); // Ensure this section takes 100px width

                // Format current_time into current_time_text_input if not editing
                if !editor_context.is_editing_current_time {
                    editor_context.current_time_text_input =
                        match editor_context.timeline_display_mode {
                            TimelineDisplayMode::Seconds => {
                                let minutes = (editor_context.current_time / 60.0).floor();
                                let seconds = (editor_context.current_time % 60.0).floor();
                                let ms = ((editor_context.current_time % 1.0) * 100.0).floor();
                                format!("{:02}:{:02}.{:02}", minutes, seconds, ms)
                            }
                            TimelineDisplayMode::Frames => {
                                let current_frame =
                                    (editor_context.current_time * composition_fps as f32).round()
                                        as i32;
                                format!("{}f", current_frame)
                            }
                            TimelineDisplayMode::SecondsAndFrames => {
                                let total_frames =
                                    (editor_context.current_time * composition_fps as f32).round()
                                        as i32;
                                let seconds = total_frames / composition_fps as i32;
                                let frames = total_frames % composition_fps as i32;
                                format!("{}s {}f", seconds, frames)
                            }
                        };
                }

                let response = ui.add(
                    egui::TextEdit::singleline(&mut editor_context.current_time_text_input)
                        .desired_width(ui.available_width())
                        .font(egui::FontId::monospace(10.0)),
                );

                if response.clicked() {
                    editor_context.is_editing_current_time = true;
                }

                if editor_context.is_editing_current_time
                    && (response.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)))
                {
                    let input_str = editor_context.current_time_text_input.clone();
                    let parsed_time_in_seconds = match editor_context.timeline_display_mode {
                        TimelineDisplayMode::Seconds => {
                            // Attempt to parse MM:SS.ms or just seconds
                            let parts: Vec<&str> = input_str.split(':').collect();
                            if parts.len() == 2 {
                                // MM:SS.ms format
                                let minutes = parts[0].parse::<f32>().unwrap_or(0.0);
                                let seconds_ms_parts: Vec<&str> = parts[1].split('.').collect();
                                let seconds = seconds_ms_parts[0].parse::<f32>().unwrap_or(0.0);
                                let ms = if seconds_ms_parts.len() == 2 {
                                    seconds_ms_parts[1].parse::<f32>().unwrap_or(0.0) / 100.0
                                } else {
                                    0.0
                                };
                                Some(minutes * 60.0 + seconds + ms)
                            } else {
                                // Just seconds (f32)
                                input_str.parse::<f32>().ok()
                            }
                        }
                        TimelineDisplayMode::Frames => {
                            // Parse as frames (integer), convert to seconds
                            input_str
                                .trim_end_matches('f')
                                .parse::<i32>()
                                .ok()
                                .map(|f| f as f32 / composition_fps as f32)
                        }
                        TimelineDisplayMode::SecondsAndFrames => {
                            // Parse "Xs Yf"
                            let re = regex::Regex::new(r"(\d+)s\s*(\d+)f").unwrap();
                            if let Some(captures) = re.captures(&input_str) {
                                let seconds = captures[1].parse::<i32>().unwrap_or(0);
                                let frames = captures[2].parse::<i32>().unwrap_or(0);
                                Some((seconds as f32) + (frames as f32 / composition_fps as f32))
                            } else {
                                None
                            }
                        }
                    };

                    if let Some(new_time) = parsed_time_in_seconds {
                        editor_context.current_time = new_time.max(0.0);
                    } else {
                        eprintln!("Failed to parse time input: {}", input_str);
                        // Revert to current_time's formatted string
                        editor_context.current_time_text_input =
                            match editor_context.timeline_display_mode {
                                TimelineDisplayMode::Seconds => {
                                    let minutes = (editor_context.current_time / 60.0).floor();
                                    let seconds = (editor_context.current_time % 60.0).floor();
                                    let ms = ((editor_context.current_time % 1.0) * 100.0).floor();
                                    format!("{:02}:{:02}.{:02}", minutes, seconds, ms)
                                }
                                TimelineDisplayMode::Frames => {
                                    let current_frame =
                                        (editor_context.current_time * composition_fps as f32)
                                            .round() as i32;
                                    format!("{}f", current_frame)
                                }
                                TimelineDisplayMode::SecondsAndFrames => {
                                    let total_frames =
                                        (editor_context.current_time * composition_fps as f32)
                                            .round() as i32;
                                    let seconds = total_frames / composition_fps as i32;
                                    let frames = total_frames % composition_fps as i32;
                                    format!("{}s {}f", seconds, frames)
                                }
                            };
                    }
                    editor_context.is_editing_current_time = false;
                }

                ui.separator();
            })
            .response;

        time_display_response.context_menu(|ui| {
            if ui.button("Seconds").clicked() {
                editor_context.timeline_display_mode = TimelineDisplayMode::Seconds;
                ui.close();
            }
            if ui.button("Frames").clicked() {
                editor_context.timeline_display_mode = TimelineDisplayMode::Frames;
                ui.close();
            }
            if ui.button("Seconds + Frames").clicked() {
                editor_context.timeline_display_mode = TimelineDisplayMode::SecondsAndFrames;
                ui.close();
            }
        });

        // --- The actual ruler ---
        let (rect, response) = h_ui.allocate_at_least(h_ui.available_size(), egui::Sense::drag());
        let painter = h_ui.painter_at(rect); // Painter for the allocated rect within h_ui
        painter.rect_filled(
            rect,
            0.0,
            h_ui.style().visuals.widgets.noninteractive.bg_fill,
        );

        if response.dragged() && response.dragged_by(egui::PointerButton::Primary) {
            // New condition
            if let Some(pos) = response.interact_pointer_pos() {
                editor_context.current_time =
                    ((pos.x - rect.min.x + scroll_offset_x - 14.0) / pixels_per_unit).max(0.0);
            }
        }

        let (major_interval, minor_interval, is_frame_display_mode) =
            match editor_context.timeline_display_mode {
                TimelineDisplayMode::Seconds => {
                    let pixels_per_frame = pixels_per_unit / composition_fps as f32;
                    let mut is_frame_display_mode = false; // Initially false

                    let (major_interval_val, minor_interval_val) = if pixels_per_frame >= 30.0 {
                        is_frame_display_mode = true; // Now in frame display mode
                        (5.0 / composition_fps as f32, 1.0 / composition_fps as f32)
                    // 5 frames major, 1 frame minor (converted to seconds for return)
                    } else if pixels_per_frame >= 15.0 {
                        is_frame_display_mode = true; // Now in frame display mode
                        (10.0 / composition_fps as f32, 1.0 / composition_fps as f32)
                    // 10 frames major, 1 frame minor (converted to seconds for return)
                    } else {
                        // Dynamic second-based intervals
                        let nice_steps = &[0.1, 0.5, 1.0, 2.0, 5.0, 10.0, 30.0, 60.0];
                        get_nice_time_intervals(pixels_per_unit, nice_steps, 50.0)
                    };
                    (
                        major_interval_val,
                        minor_interval_val,
                        is_frame_display_mode,
                    )
                }
                TimelineDisplayMode::Frames | TimelineDisplayMode::SecondsAndFrames => {
                    get_frame_intervals(pixels_per_unit, composition_fps as f32)
                }
            };

        fn get_nice_time_intervals(
            pixels_per_unit: f32,
            nice_steps: &[f32],
            target_pixels_per_major_tick: f32,
        ) -> (f32, f32) {
            let mut major_interval = nice_steps[0];

            for &step in nice_steps.iter() {
                let pixels_for_this_step = step * pixels_per_unit;
                if pixels_for_this_step > target_pixels_per_major_tick {
                    major_interval = step;
                    break;
                }
            }

            let minor_interval = if major_interval >= 10.0 {
                major_interval / 5.0
            } else if major_interval >= 5.0 {
                major_interval / 5.0
            } else if major_interval >= 2.0 {
                major_interval / 5.0
            } else {
                // major_interval is 1.0 or smaller
                major_interval / 5.0 // This will give 0.5s or 0.5 frames
            };

            (major_interval, minor_interval)
        }

        fn get_frame_intervals(pixels_per_unit: f32, _fps: f32) -> (f32, f32, bool) {
            // bool indicates if it's purely frames
            // pixels_per_unit is pixels per frame
            let nice_steps = &[1.0, 2.0, 5.0, 10.0, 25.0, 50.0, 100.0];
            let (major_interval_frames, minor_interval_frames) =
                get_nice_time_intervals(pixels_per_unit, nice_steps, 50.0);

            // Return frame intervals directly
            (
                major_interval_frames,
                minor_interval_frames,
                true, // This mode is purely frames
            )
        }

        // Determine the actual interval values in their native units
        let (true_major_interval_val, true_minor_interval_val);
        if is_frame_display_mode {
            // Intervals are in frames, but major_interval and minor_interval currently hold them converted to seconds.
            // Need to convert them back to frames (f32 for now, will cast to i32 in loop)
            true_major_interval_val = major_interval * composition_fps as f32;
            true_minor_interval_val = minor_interval * composition_fps as f32;
        } else {
            // Intervals are already in seconds
            true_major_interval_val = major_interval;
            true_minor_interval_val = minor_interval;
        }

        if is_frame_display_mode {
            let pixels_per_frame = pixels_per_unit / composition_fps as f32;
            let first_visible_frame = (scroll_offset_x / pixels_per_frame).round() as i32;
            let last_visible_frame =
                ((scroll_offset_x + rect.width()) / pixels_per_frame).round() as i32;

            let major_interval_frames = true_major_interval_val.round() as i32;
            let minor_interval_frames = true_minor_interval_val.round() as i32;

            let mut current_frame =
                (first_visible_frame / minor_interval_frames) * minor_interval_frames;
            if current_frame < 0 {
                current_frame = 0;
            }

            while current_frame <= last_visible_frame {
                let s = current_frame as f32 / composition_fps as f32; // Convert current_frame to seconds for context

                // Position of the current unit mark, relative to the *start* of the scrollable content
                let content_x = current_frame as f32 * pixels_per_frame;

                // Position relative to the *visible area* of the ruler (rect)
                let x_pos_on_rect = content_x - scroll_offset_x;

                // Now, convert to absolute screen coordinates for the painter.
                let screen_x = rect.min.x + x_pos_on_rect + 16.0;

                if screen_x >= rect.min.x && screen_x <= rect.max.x {
                    // Check if the current frame is a major tick (integer arithmetic)
                    let is_major = current_frame % major_interval_frames == 0;

                    let (line_start_y, line_end_y, stroke_color);
                    if is_major {
                        line_start_y = rect.min.y;
                        line_end_y = rect.max.y; // Full height for major
                        stroke_color = egui::Color32::WHITE;

                        // Determine label text
                        let label_text;
                        if s.abs() < 1.0 {
                            // If current tick is less than 1 second from origin
                            let total_frames_for_label =
                                (s * composition_fps as f32).round() as i32;
                            let frames_in_second_for_label =
                                total_frames_for_label % composition_fps as i32;
                            if total_frames_for_label == 0 {
                                // Special case for 0s
                                label_text = format!("{}s", 0);
                            } else {
                                label_text = format!("{}f", frames_in_second_for_label);
                            }
                        } else {
                            // 1 second or more
                            label_text = format!("{}s", s.round() as i32);
                        }

                        // Position and draw the label
                        let text_pos = egui::pos2(screen_x + 3.0, rect.min.y + 2.0); // Shift right by 3.0, slightly below the top of the ruler
                        painter.text(
                            text_pos,
                            egui::Align2::LEFT_TOP, // Changed to LEFT_TOP for left-alignment
                            label_text,
                            egui::FontId::proportional(9.0), // Smaller font for labels
                            egui::Color32::WHITE,
                        );
                    } else {
                        line_start_y = rect.max.y - (rect.height() * 0.33); // Minor ticks are 1/3rd height from bottom
                        line_end_y = rect.max.y; // Go to bottom
                        stroke_color = egui::Color32::from_gray(150); // Brighter gray for minor ticks
                    }

                    painter.line_segment(
                        [
                            egui::pos2(screen_x, line_start_y),
                            egui::pos2(screen_x, line_end_y),
                        ],
                        egui::Stroke::new(1.0, stroke_color),
                    );
                }
                current_frame += minor_interval_frames;
            }
        } else {
            // Second-based iteration
            let first_visible_timeline_unit = (scroll_offset_x / pixels_per_unit).max(0.0);
            let last_visible_timeline_unit = (scroll_offset_x + rect.width()) / pixels_per_unit;

            // Find the first minor tick that is visible
            let mut s = (first_visible_timeline_unit / minor_interval).floor() * minor_interval;

            // Ensure we don't start before 0.0
            if s < 0.0 {
                s = 0.0;
            }

            // Iterate through all minor ticks within the visible range
            while s <= last_visible_timeline_unit {
                // Position of the current unit mark, relative to the *start* of the scrollable content
                let content_x = s * pixels_per_unit;

                // Position relative to the *visible area* of the ruler (rect)
                let x_pos_on_rect = content_x - scroll_offset_x;

                // Now, convert to absolute screen coordinates for the painter.
                let screen_x = rect.min.x + x_pos_on_rect + 16.0;

                if screen_x >= rect.min.x && screen_x <= rect.max.x {
                    // Check if the current unit is a major tick
                    let is_major =
                        ((s / major_interval).round() * major_interval - s).abs() < 0.001;

                    let (line_start_y, line_end_y, stroke_color);
                    if is_major {
                        line_start_y = rect.min.y;
                        line_end_y = rect.max.y; // Full height for major
                        stroke_color = egui::Color32::WHITE;

                        // Determine label text
                        let label_text;
                        if s.abs() < 1.0 {
                            // If current tick is less than 1 second from origin
                            let total_frames_for_label =
                                (s * composition_fps as f32).round() as i32;
                            let frames_in_second_for_label =
                                total_frames_for_label % composition_fps as i32;
                            if total_frames_for_label == 0 {
                                // Special case for 0s
                                label_text = format!("{}s", 0);
                            } else {
                                label_text = format!("{}f", frames_in_second_for_label);
                            }
                        } else {
                            // 1 second or more
                            label_text = format!("{}s", s.round() as i32);
                        }
                        // Position and draw the label
                        let text_pos = egui::pos2(screen_x + 3.0, rect.min.y + 2.0); // Shift right by 3.0, slightly below the top of the ruler
                        painter.text(
                            text_pos,
                            egui::Align2::LEFT_TOP, // Changed to LEFT_TOP for left-alignment
                            label_text,
                            egui::FontId::proportional(9.0), // Smaller font for labels
                            egui::Color32::WHITE,
                        );
                    } else {
                        line_start_y = rect.max.y - (rect.height() * 0.5); // Minor ticks are 1/3rd height from bottom
                        line_end_y = rect.max.y; // Go to bottom
                        stroke_color = egui::Color32::from_gray(150); // Brighter gray for minor ticks
                    }

                    painter.line_segment(
                        [
                            egui::pos2(screen_x, line_start_y),
                            egui::pos2(screen_x, line_end_y),
                        ],
                        egui::Stroke::new(1.0, stroke_color),
                    );
                }
                s += minor_interval;
            }
        }
    });
}
