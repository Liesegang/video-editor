use crate::model::ui_types::TimelineDisplayMode;
use crate::state::context::EditorContext;
use egui::Ui;

pub fn show_time_input(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    composition_fps: f64,
    max_duration: f64,
) -> egui::Response {
// ...
// This tool doesn't support skipping chunks easily with dots, so I have to be precise or use multi-replace.
// But signatures are at the top, usage is in the middle.
// I'll use multi_replace interaction for this file.
// Wait, I cannot use multi-replace if I'm using replace_file_content.
// I'll use replace_file_content twice or use multi_replace_file_content.
// I'll use separate calls since they are far apart. First the signature.
    let text_edit_response = ui.vertical(|ui| {
        ui.set_width(100.0); // Ensure this section takes 100px width

        // Format current_time into current_time_text_input if not editing
        if !editor_context.interaction.is_editing_current_time {
            editor_context.interaction.current_time_text_input =
                match editor_context.timeline.display_mode {
                    TimelineDisplayMode::Seconds => {
                        let minutes = (editor_context.timeline.current_time / 60.0).floor();
                        let seconds = (editor_context.timeline.current_time % 60.0).floor();
                        let ms =
                            ((editor_context.timeline.current_time % 1.0) * 100.0).floor();
                        format!("{:02}:{:02}.{:02}", minutes, seconds, ms)
                    }
                    TimelineDisplayMode::Frames => {
                        let current_frame =
                            (editor_context.timeline.current_time * composition_fps as f32)
                                .round() as i32;
                        format!("{}f", current_frame)
                    }
                    TimelineDisplayMode::SecondsAndFrames => {
                        let total_frames =
                            (editor_context.timeline.current_time * composition_fps as f32)
                                .round() as i32;
                        let seconds = total_frames / composition_fps as i32;
                        let frames = total_frames % composition_fps as i32;
                        format!("{}s {}f", seconds, frames)
                    }
                };
        }

        let response = ui.add(
            egui::TextEdit::singleline(
                &mut editor_context.interaction.current_time_text_input,
            )
            .desired_width(ui.available_width())
            .font(egui::FontId::monospace(10.0)),
        );

        if response.clicked() {
            editor_context.interaction.is_editing_current_time = true;
        }

        if editor_context.interaction.is_editing_current_time
            && (response.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)))
        {
            let input_str = editor_context.interaction.current_time_text_input.clone();
            let parsed_time_in_seconds = match editor_context.timeline.display_mode {
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
                let snapped_time = (new_time * composition_fps as f32).round() / composition_fps as f32;
                editor_context.timeline.current_time = snapped_time.clamp(0.0, max_duration as f32);
            } else {
                eprintln!("Failed to parse time input: {}", input_str);
                // Revert to current_time's formatted string
                editor_context.interaction.current_time_text_input = match editor_context
                    .timeline
                    .display_mode
                {
                    TimelineDisplayMode::Seconds => {
                        let minutes = (editor_context.timeline.current_time / 60.0).floor();
                        let seconds = (editor_context.timeline.current_time % 60.0).floor();
                        let ms =
                            ((editor_context.timeline.current_time % 1.0) * 100.0).floor();
                        format!("{:02}:{:02}.{:02}", minutes, seconds, ms)
                    }
                    TimelineDisplayMode::Frames => {
                        let current_frame =
                            (editor_context.timeline.current_time * composition_fps as f32)
                                .round() as i32;
                        format!("{}f", current_frame)
                    }
                    TimelineDisplayMode::SecondsAndFrames => {
                        let total_frames =
                            (editor_context.timeline.current_time * composition_fps as f32)
                                .round() as i32;
                        let seconds = total_frames / composition_fps as i32;
                        let frames = total_frames % composition_fps as i32;
                        format!("{}s {}f", seconds, frames)
                    }
                };
            }
            editor_context.interaction.is_editing_current_time = false;
        }

        // Return the response from the TextEdit so we can attach context menu to it
        response
    })
    .inner; // We now get the InnerResponse's inner value which is the TextEdit response

    text_edit_response.context_menu(|ui| {
        if ui.button("Seconds").clicked() {
            editor_context.timeline.display_mode = TimelineDisplayMode::Seconds;
            ui.close();
        }
        if ui.button("Frames").clicked() {
            editor_context.timeline.display_mode = TimelineDisplayMode::Frames;
            ui.close();
        }
        if ui.button("Seconds + Frames").clicked() {
            editor_context.timeline.display_mode = TimelineDisplayMode::SecondsAndFrames;
            ui.close();
        }
    });

    text_edit_response
}
