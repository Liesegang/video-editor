use egui::{epaint::StrokeKind, Ui};
use egui_phosphor::regular as icons;
use library::model::project::project::Project;
use library::model::project::Track;
use library::model::project::TrackEntity;
use library::service::project_service::ProjectService;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::{
    action::HistoryManager, model::assets::AssetKind, model::ui_types::TimelineDisplayMode,
    state::context::EditorContext,
};

fn show_timeline_ruler(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    _project_service: &ProjectService,
    _project: &Arc<RwLock<Project>>,
    pixels_per_unit: f32,
    scroll_offset_x: f32,
) {
    ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), |h_ui| {
        let time_display_response = h_ui
            .vertical(|ui| {
                ui.set_width(100.0); // Ensure this section takes 100px width

                // Format current_time into current_time_text_input if not editing
                if !editor_context.is_editing_current_time {
                    editor_context.current_time_text_input = match editor_context
                        .timeline_display_mode
                    {
                        TimelineDisplayMode::Seconds => {
                            let minutes = (editor_context.current_time / 60.0).floor();
                            let seconds = (editor_context.current_time % 60.0).floor();
                            let ms = ((editor_context.current_time % 1.0) * 100.0).floor();
                            format!("{:02}:{:02}.{:02}", minutes, seconds, ms)
                        }
                        TimelineDisplayMode::Frames => {
                            let current_frame =
                                (editor_context.current_time * editor_context.fps).round() as i32;
                            format!("{}f", current_frame)
                        }
                        TimelineDisplayMode::SecondsAndFrames => {
                            let total_frames =
                                (editor_context.current_time * editor_context.fps).round() as i32;
                            let seconds = total_frames / editor_context.fps as i32;
                            let frames = total_frames % editor_context.fps as i32;
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
                                .map(|f| f as f32 / editor_context.fps)
                        }
                        TimelineDisplayMode::SecondsAndFrames => {
                            // Parse "Xs Yf"
                            let re = regex::Regex::new(r"(\d+)s\s*(\d+)f").unwrap();
                            if let Some(captures) = re.captures(&input_str) {
                                let seconds = captures[1].parse::<i32>().unwrap_or(0);
                                let frames = captures[2].parse::<i32>().unwrap_or(0);
                                Some((seconds as f32) + (frames as f32 / editor_context.fps))
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
                                        (editor_context.current_time * editor_context.fps).round()
                                            as i32;
                                    format!("{}f", current_frame)
                                }
                                TimelineDisplayMode::SecondsAndFrames => {
                                    let total_frames =
                                        (editor_context.current_time * editor_context.fps).round()
                                            as i32;
                                    let seconds = total_frames / editor_context.fps as i32;
                                    let frames = total_frames % editor_context.fps as i32;
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

        if response.dragged() && !response.dragged_by(egui::PointerButton::Middle) {
            // New condition
            if let Some(pos) = response.interact_pointer_pos() {
                editor_context.current_time =
                    ((pos.x - rect.min.x + scroll_offset_x - 8.0) / pixels_per_unit).max(0.0);
            }
        }

        let (major_interval, minor_interval, display_frames_in_seconds_mode) =
            match editor_context.timeline_display_mode {
                TimelineDisplayMode::Seconds => {
                    let pixels_per_frame = pixels_per_unit / editor_context.fps;
                    if pixels_per_frame >= 30.0 {
                        // 1 frame is 30 pixels or more, show frame-accurate ticks (1 frame minor, 5 frames major)
                        (5.0 / editor_context.fps, 1.0 / editor_context.fps, true)
                    } else if pixels_per_frame >= 15.0 {
                        // 1 frame is 15 pixels or more
                        (10.0 / editor_context.fps, 1.0 / editor_context.fps, true)
                    } else if pixels_per_frame >= 5.0 {
                        // 1 frame is 5 pixels or more
                        (5.0 / editor_context.fps, 1.0 / editor_context.fps, true)
                    } else if pixels_per_unit > 150.0 {
                        // Show 0.1s intervals
                        (0.5, 0.1, false)
                    } else if pixels_per_unit > 75.0 {
                        // Show 0.5s intervals
                        (1.0, 0.5, false)
                    } else if pixels_per_unit > 30.0 {
                        // Show 1s intervals
                        (5.0, 1.0, false)
                    } else if pixels_per_unit > 15.0 {
                        // Show 5s intervals
                        (10.0, 5.0, false)
                    } else {
                        // Show 10s intervals
                        (30.0, 10.0, false)
                    }
                }
                TimelineDisplayMode::Frames | TimelineDisplayMode::SecondsAndFrames => {
                    // pixels_per_unit is pixels per frame
                    if pixels_per_unit >= 30.0 {
                        // 1 frame is 30 pixels or more, show every 1 frame as minor
                        (5.0, 1.0, false) // Every 5 frames major, every 1 frame minor. 'false' here is dummy.
                    } else if pixels_per_unit >= 15.0 {
                        // 1 frame is 15 pixels or more
                        (10.0, 1.0, false)
                    } else if pixels_per_unit >= 5.0 {
                        // 1 frame is 5 pixels or more
                        (5.0, 1.0, false)
                    } else if pixels_per_unit > 2.0 {
                        // Moderately zoomed in, show every 10 frames as major, every 5 frames as minor
                        (10.0, 5.0, false)
                    } else if pixels_per_unit > 1.0 {
                        // Less zoomed in, show every 5 frames as major, every 1 frame as minor
                        (5.0, 1.0, false)
                    } else {
                        // Very zoomed out, show every 10 frames as major, every 5 frames as minor
                        (10.0, 5.0, false)
                    }
                }
            };

        // --- Calculate visible range in timeline units ---
        let first_visible_timeline_unit = (scroll_offset_x / pixels_per_unit).max(0.0);
        let last_visible_timeline_unit = (scroll_offset_x + rect.width()) / pixels_per_unit;

        // Find the first minor tick that is visible
        let mut current_unit =
            (first_visible_timeline_unit / minor_interval).floor() * minor_interval;

        // Ensure we don't start before 0.0
        if current_unit < 0.0 {
            current_unit = 0.0;
        }

        // Iterate through all minor ticks within the visible range
        while current_unit <= last_visible_timeline_unit {
            let s = current_unit; // s is now guaranteed to be a multiple of minor_interval

            // Position of the current unit mark, relative to the *start* of the scrollable content
            let content_x = s * pixels_per_unit;

            // Position relative to the *visible area* of the ruler (rect)
            let x_pos_on_rect = content_x - scroll_offset_x;

            // Now, convert to absolute screen coordinates for the painter.
            let screen_x = rect.min.x + x_pos_on_rect + 16.0;

            if screen_x >= rect.min.x && screen_x <= rect.max.x {
                // Use a small epsilon for major interval check due to floating point arithmetic
                let is_major = (s / major_interval).fract().abs() < f32::EPSILON
                    || (s / major_interval).fract().abs() > 1.0 - f32::EPSILON;

                let (line_start_y, line_end_y);
                let text_pos_y;

                if is_major {
                    line_start_y = rect.min.y;
                    line_end_y = rect.max.y; // Full height for major
                } else {
                    line_start_y = rect.center().y; // Start from middle
                    line_end_y = rect.max.y; // Go to bottom, making it shorter and lower
                }
                text_pos_y = rect.min.y + 2.0; // Upper position for all labels

                painter.line_segment(
                    [
                        egui::pos2(screen_x, line_start_y),
                        egui::pos2(screen_x, line_end_y),
                    ],
                    egui::Stroke::new(1.0, egui::Color32::WHITE),
                );


            }
            current_unit += minor_interval;
        }
    });
}


// Helper function to show the timeline controls
fn show_timeline_controls(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    _history_manager: &mut HistoryManager, // Not used directly here
    _project_service: &ProjectService,
    _project: &Arc<RwLock<Project>>, // Used for undo/redo if implemented directly, or passed to commands
) {
    ui.horizontal(|ui| {
        // Play button
        let play_icon_enum = if editor_context.is_playing {
            icons::PAUSE
        } else {
            icons::PLAY
        };
        if ui
          .add(egui::Button::new(egui::RichText::new(play_icon_enum)))
          .clicked()
        {
            editor_context.is_playing = !editor_context.is_playing;
        }

        // Time display
        let minutes = (editor_context.current_time / 60.0).floor();
        let seconds = (editor_context.current_time % 60.0).floor();
        let ms = ((editor_context.current_time % 1.0) * 100.0).floor();
        let time_text = format!("{:02}:{:02}.{:02}", minutes, seconds, ms);
        ui.label(egui::RichText::new(time_text).monospace());

        // Spacer
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Zoom reset button
            if ui
              .add(egui::Button::new(egui::RichText::new(icons::FRAME_CORNERS)))
              .clicked()
            {
                editor_context.timeline_h_zoom = 1.0;
                editor_context.timeline_v_zoom = 1.0;
            }

            // Zoom information
            let zoom_text = format!("H-Zoom: {:.1}x", editor_context.timeline_h_zoom);
            ui.label(zoom_text);
        });
    });
}

pub fn timeline_panel(
    ui: &mut Ui,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut ProjectService,
    project: &Arc<RwLock<Project>>,
) {
    const TRACK_LIST_SIDEBAR_WIDTH: f32 = 100.0; // New constant
    let pixels_per_unit = match editor_context.timeline_display_mode {
        TimelineDisplayMode::Seconds => {
            editor_context.timeline_pixels_per_second * editor_context.timeline_h_zoom
        }
        TimelineDisplayMode::Frames | TimelineDisplayMode::SecondsAndFrames => {
            (editor_context.timeline_pixels_per_second / editor_context.fps)
              * editor_context.timeline_h_zoom
        }
    };
    let scroll_offset_x = editor_context.timeline_scroll_offset.x;

    // Use panels to divide the space correctly
    egui::TopBottomPanel::top("timeline_ruler_panel")
      .exact_height(20.0)
      .show_inside(ui, |ui| {
          show_timeline_ruler(
              ui,
              editor_context,
              project_service,
              project,
              pixels_per_unit,
              scroll_offset_x,
          );
      });

    egui::TopBottomPanel::bottom("timeline_controls_panel")
      .exact_height(40.0)
      .show_inside(ui, |ui| {
          ui.separator();
          show_timeline_controls(
              ui,
              editor_context,
              history_manager,
              project_service,
              project,
          );
      });

    let mut central_panel_rect = egui::Rect::NOTHING; // Declare before CentralPanel block
    egui::CentralPanel::default().show_inside(ui, |ui| {
        central_panel_rect = ui.available_rect_before_wrap(); // Capture the rect
        // Main timeline content
        ui.with_layout(
            egui::Layout::left_to_right(egui::Align::TOP),
            |ui_content| {
                // --- Track list sidebar ---
                let (track_list_rect, track_list_response) = ui_content.allocate_exact_size(
                    egui::vec2(TRACK_LIST_SIDEBAR_WIDTH, ui_content.available_height()),
                    egui::Sense::click_and_drag(),
                );
                let track_list_painter = ui_content.painter_at(track_list_rect);
                track_list_painter.rect_filled(
                    track_list_rect,
                    0.0,
                    ui_content.style().visuals.window_fill(),
                ); // Fill entire sidebar background

                let row_height = 30.0;
                let track_spacing = 2.0;

                let mut current_tracks: Vec<Track> = Vec::new();
                let selected_composition_id = editor_context.selected_composition_id;
                if let Some(comp_id) = selected_composition_id {
                    if let Ok(proj_read) = project.read() {
                        if let Some(comp) = proj_read.compositions.iter().find(|c| c.id == comp_id)
                        {
                            current_tracks = comp.tracks.clone();
                        }
                    }
                }
                let num_tracks = current_tracks.len();

                for (i, track) in current_tracks.iter().enumerate() {
                    let y = track_list_rect.min.y
                      + (i as f32 * (row_height + track_spacing))
                      + editor_context.timeline_scroll_offset.y;
                    let track_label_rect = egui::Rect::from_min_size(
                        egui::pos2(track_list_rect.min.x, y),
                        egui::vec2(track_list_rect.width(), row_height),
                    );

                    if track_list_rect.intersects(track_label_rect) {
                        let track_interaction_response = ui_content
                          .interact(
                              track_label_rect,
                              egui::Id::new(track.id).with("track_label_interact"),
                              egui::Sense::click(),
                          )
                          .on_hover_text(format!("Track ID: {}", track.id));
                        if track_interaction_response.clicked() {
                            editor_context.selected_track_id = Some(track.id);
                        }

                        // Draw alternating background for this row
                        track_list_painter.rect_filled(
                            track_label_rect,
                            0.0,
                            if editor_context.selected_track_id == Some(track.id) {
                                egui::Color32::from_rgb(50, 80, 120)
                            } else if i % 2 == 0 {
                                egui::Color32::from_gray(50)
                            } else {
                                egui::Color32::from_gray(60)
                            },
                        );
                        // Draw text label
                        track_list_painter.text(
                            track_label_rect.left_center() + egui::vec2(5.0, 0.0),
                            egui::Align2::LEFT_CENTER,
                            format!("Track {}", track.name), // Display track name
                            egui::FontId::monospace(10.0),
                            egui::Color32::GRAY,
                        );
                    }
                }

                // Track list right-click context menu
                track_list_response.context_menu(|ui_content| {
                    if let Some(comp_id) = editor_context.selected_composition_id {
                        if ui_content
                          .add(egui::Button::new(egui::RichText::new(format!(
                              "{} Add Track",
                              icons::PLUS
                          ))))
                          .clicked()
                        {
                            let prev_project_state =
                              project_service.get_project().read().unwrap().clone();
                            project_service
                              .add_track(comp_id, "New Track")
                              .expect("Failed to add track");
                            history_manager.push_project_state(prev_project_state);
                            ui_content.close();
                        }
                        if let Some(track_id) = editor_context.selected_track_id {
                            if ui_content
                              .add(egui::Button::new(egui::RichText::new(format!(
                                  "{} Remove Selected Track",
                                  icons::TRASH
                              ))))
                              .clicked()
                            {
                                let prev_project_state =
                                  project_service.get_project().read().unwrap().clone();
                                project_service
                                  .remove_track(comp_id, track_id)
                                  .expect("Failed to remove track");
                                editor_context.selected_track_id = None;
                                editor_context.selected_entity_id = None;
                                history_manager.push_project_state(prev_project_state);
                                ui_content.close();
                            }
                        } else {
                            ui_content.label("Select a track to remove");
                        }
                    } else {
                        ui_content.label("Select a Composition first");
                    }
                });

                ui_content.separator();

                // --- Clip area ---
                let (content_rect_for_clip_area, response) = ui_content
                  .allocate_at_least(ui_content.available_size(), egui::Sense::click_and_drag());
                // Define pixels_per_unit based on display mode
                let pixels_per_unit = match editor_context.timeline_display_mode {
                    TimelineDisplayMode::Seconds => {
                        editor_context.timeline_pixels_per_second * editor_context.timeline_h_zoom
                    }
                    TimelineDisplayMode::Frames | TimelineDisplayMode::SecondsAndFrames => {
                        (editor_context.timeline_pixels_per_second / editor_context.fps)
                          * editor_context.timeline_h_zoom
                    }
                };

                // --- Interaction ---
                if response.hovered() {
                    let scroll_delta = ui_content.input(|i| i.raw_scroll_delta);
                    if ui_content.input(|i| i.modifiers.ctrl) && scroll_delta.y != 0.0 {
                        let zoom_factor = if scroll_delta.y > 0.0 { 1.1 } else { 0.9 };
                        editor_context.timeline_h_zoom =
                          (editor_context.timeline_h_zoom * zoom_factor).clamp(0.1, 10.0);
                    } else if scroll_delta.y != 0.0 {
                        editor_context.timeline_scroll_offset.y -= scroll_delta.y;
                    }

                    if scroll_delta.x != 0.0 {
                        editor_context.timeline_scroll_offset.x -= scroll_delta.x;
                        // Clamp timeline_scroll_offset.x to prevent scrolling left past 0s
                        editor_context.timeline_scroll_offset.x =
                          editor_context.timeline_scroll_offset.x.max(0.0);
                    }
                }
                if response.dragged_by(egui::PointerButton::Middle) {
                    editor_context.timeline_scroll_offset.x -= response.drag_delta().x;
                    editor_context.timeline_scroll_offset.y += response.drag_delta().y;

                    // Clamp timeline_scroll_offset.x to prevent scrolling left past 0s
                    editor_context.timeline_scroll_offset.x =
                      editor_context.timeline_scroll_offset.x.max(0.0);
                }

                // --- Drawing ---
                let painter = ui_content.painter_at(content_rect_for_clip_area);
                // Constrain scroll offset
                let max_scroll_y = (num_tracks as f32 * (row_height + track_spacing))
                  - content_rect_for_clip_area.height();
                editor_context.timeline_scroll_offset.y = editor_context
                  .timeline_scroll_offset
                  .y
                  .clamp(-max_scroll_y.max(0.0), 0.0);

                for i in 0..num_tracks {
                    let y = content_rect_for_clip_area.min.y
                      + (i as f32 * (row_height + track_spacing))
                      + editor_context.timeline_scroll_offset.y;
                    let track_rect = egui::Rect::from_min_size(
                        egui::pos2(content_rect_for_clip_area.min.x, y),
                        egui::vec2(content_rect_for_clip_area.width(), row_height),
                    );
                    painter.rect_filled(
                        track_rect,
                        0.0,
                        if i % 2 == 0 {
                            egui::Color32::from_gray(50)
                        } else {
                            egui::Color32::from_gray(60)
                        },
                    );
                }

                // Logic for adding entity to track on drag-drop
                if ui_content.input(|i| i.pointer.any_released()) {
                    if let Some(asset_index) = editor_context.dragged_asset {
                        if let Some(mouse_pos) = response.hover_pos() {
                            let drop_time = ((mouse_pos.x
                              - content_rect_for_clip_area.min.x
                              - editor_context.timeline_scroll_offset.x)
                              / pixels_per_unit)
                              .max(0.0);
                            let drop_track_index = ((mouse_pos.y
                              - content_rect_for_clip_area.min.y
                              - editor_context.timeline_scroll_offset.y)
                              / (row_height + track_spacing))
                              .floor()
                              as usize;

                            if let Some(comp_id) = editor_context.selected_composition_id {
                                if let Some(track) = current_tracks.get(drop_track_index) {
                                    if let Some(asset) = editor_context.assets.get(asset_index) {
                                        // Handle dropping a Composition asset
                                        if let AssetKind::Composition(_nested_comp_id) = asset.kind
                                        {
                                            let prev_project_state = project_service
                                              .get_project()
                                              .read()
                                              .unwrap()
                                              .clone();
                                            if let Err(e) = project_service.add_entity_to_track(
                                                comp_id,
                                                track.id,
                                                &format!("Nested Comp: {}", asset.name),
                                                drop_time as f64,
                                                (drop_time + asset.duration) as f64,
                                            ) {
                                                eprintln!(
                                                    "Failed to add nested composition entity: {:?}",
                                                    e
                                                );
                                            } else {
                                                history_manager
                                                  .push_project_state(prev_project_state);
                                            }
                                        } else {
                                            let prev_project_state = project_service
                                              .get_project()
                                              .read()
                                              .unwrap()
                                              .clone();
                                            if let Err(e) = project_service.add_entity_to_track(
                                                comp_id,
                                                track.id,
                                                &asset.name, // Use asset name as entity type for now
                                                drop_time as f64,
                                                (drop_time + asset.duration) as f64,
                                            ) {
                                                eprintln!("Failed to add entity to track: {:?}", e);
                                            } else {
                                                history_manager
                                                  .push_project_state(prev_project_state);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                let is_dragging_asset = editor_context.dragged_asset.is_some();
                let mut clicked_on_entity = false;

                if !is_dragging_asset
                  && response.dragged()
                  && !response.dragged_by(egui::PointerButton::Middle)
                {
                    // Added condition
                    if let Some(pos) = response.interact_pointer_pos() {
                        editor_context.current_time = ((pos.x - content_rect_for_clip_area.min.x
                          + editor_context.timeline_scroll_offset.x)
                          / pixels_per_unit)
                          .max(0.0);
                    }
                }
                if !is_dragging_asset
                  && response.drag_stopped()
                  && !response.dragged_by(egui::PointerButton::Middle)
                {
                    // Added condition
                    if let Some(pos) = response.interact_pointer_pos() {
                        editor_context.current_time = ((pos.x - content_rect_for_clip_area.min.x
                          + editor_context.timeline_scroll_offset.x)
                          / pixels_per_unit)
                          .max(0.0);
                    }
                }

                // --- Data collection for entities (new block) ---
                let mut all_entities: Vec<(Uuid, TrackEntity)> = Vec::new(); // (track_id, entity)
                for track in &current_tracks {
                    for entity in &track.entities {
                        all_entities.push((track.id, entity.clone()));
                    }
                }
                // --- End Data collection for entities ---

                // ... (rest of the timeline_panel function) ...

                // Draw entities (clips) from the Project model
                // if let Ok(proj_read) = project.read() { // REMOVED THIS BLOCK
                //     if let Some(comp) = editor_context.get_current_composition(&proj_read) { // REMOVED THIS BLOCK
                for track_in_all_entities in &current_tracks {
                    // Iterate over the pre-collected tracks
                    let clip_track_index = current_tracks
                      .iter()
                      .position(|t| t.id == track_in_all_entities.id)
                      .map(|idx| idx as f32)
                      .unwrap_or(0.0);

                    for (entity_track_id, entity) in all_entities
                      .iter()
                      .filter(|(t_id, _)| *t_id == track_in_all_entities.id)
                    {
                        let asset_index = 0; // Temporary: should derive from entity properties
                        let asset = editor_context.assets.get(asset_index);

                        if let Some(a) = asset {
                            let gc = crate::model::ui_types::GuiClip {
                                id: entity.id,
                                name: entity.entity_type.clone(),
                                track_id: *entity_track_id,
                                start_time: entity.start_time as f32,
                                duration: (entity.end_time - entity.start_time) as f32,
                                color: a.color,
                                position: [
                                    entity.properties.get_f32("position_x").unwrap_or(960.0),
                                    entity.properties.get_f32("position_y").unwrap_or(540.0),
                                ],
                                scale: entity.properties.get_f32("scale").unwrap_or(100.0),
                                opacity: entity.properties.get_f32("opacity").unwrap_or(100.0),
                                rotation: entity.properties.get_f32("rotation").unwrap_or(0.0),
                                asset_index,
                            };

                            let x = content_rect_for_clip_area.min.x
                              + gc.start_time * pixels_per_unit
                              - editor_context.timeline_scroll_offset.x;
                            let y = content_rect_for_clip_area.min.y
                              + editor_context.timeline_scroll_offset.y
                              + clip_track_index * (row_height + track_spacing);
                            let clip_rect = egui::Rect::from_min_size(
                                egui::pos2(x, y),
                                egui::vec2(gc.duration * pixels_per_unit, row_height),
                            );

                            let clip_resp = ui_content.interact(
                                clip_rect,
                                egui::Id::new(gc.id),
                                egui::Sense::click_and_drag(),
                            );
                            if clip_resp.clicked() {
                                editor_context.selected_entity_id = Some(gc.id);
                                editor_context.selected_track_id = Some(gc.track_id);
                                clicked_on_entity = true;
                            }

                            if clip_resp.drag_started() {
                                editor_context.selected_entity_id = Some(gc.id);
                                editor_context.selected_track_id = Some(gc.track_id);
                                if editor_context.last_project_state_before_drag.is_none() {
                                    editor_context.last_project_state_before_drag =
                                      Some(project_service.get_project().read().unwrap().clone());
                                }
                            }
                            if clip_resp.dragged()
                              && editor_context.selected_entity_id == Some(gc.id)
                            {
                                let dt = clip_resp.drag_delta().x as f64 / pixels_per_unit as f64;

                                if let Some(comp_id) = editor_context.selected_composition_id {
                                    if let Some(track_id) = editor_context.selected_track_id {
                                        project_service
                                          .with_track_mut(comp_id, track_id, |track_mut| {
                                              if let Some(entity_mut) = track_mut
                                                .entities
                                                .iter_mut()
                                                .find(|e| e.id == gc.id)
                                              {
                                                  entity_mut.start_time =
                                                    (entity_mut.start_time + dt).max(0.0);
                                                  entity_mut.end_time = (entity_mut.end_time
                                                    + dt)
                                                    .max(entity_mut.start_time);
                                              }
                                          })
                                          .ok();
                                    }
                                }
                            }
                            if clip_resp.drag_stopped()
                              && editor_context.selected_entity_id == Some(gc.id)
                            {
                                if let Some(initial_state) =
                                  editor_context.last_project_state_before_drag.take()
                                {
                                    let current_state =
                                      project_service.get_project().read().unwrap().clone();
                                    if initial_state != current_state {
                                        history_manager.push_project_state(initial_state);
                                    }
                                }
                            }

                            let is_sel = editor_context.selected_entity_id == Some(gc.id);
                            let color = gc.color;
                            let transparent_color = egui::Color32::from_rgba_premultiplied(
                                color.r(),
                                color.g(),
                                color.b(),
                                150,
                            );

                            painter.rect_filled(clip_rect, 4.0, transparent_color);
                            if is_sel {
                                painter.rect_stroke(
                                    clip_rect,
                                    4.0,
                                    egui::Stroke::new(2.0, egui::Color32::WHITE),
                                    StrokeKind::Middle,
                                );
                            }
                            painter.text(
                                clip_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                &gc.name,
                                egui::FontId::default(),
                                egui::Color32::BLACK,
                            );
                        }
                    }
                }

                if clicked_on_entity {
                    // Selected entity is already set
                } else if response.clicked() && !is_dragging_asset {
                    editor_context.selected_entity_id = None;
                }
            },
        );
    });

    // Draw cursor after all panels are laid out
    let cx = central_panel_rect.min.x
      + TRACK_LIST_SIDEBAR_WIDTH // Add the width of the track list sidebar
      - scroll_offset_x
      + editor_context.current_time * pixels_per_unit
      + 24.0;

    let full_timeline_area = ui.available_rect_before_wrap(); // This is the total area for the timeline_panel

    // The line should span from the top of the ruler to the bottom of the central panel.
    // The top of the ruler is full_timeline_area.min.y
    // The bottom of the central panel is central_panel_rect.max.y
    let ruler_y_min = ui.clip_rect().min.y;
    let ruler_y_max = ruler_y_min + 20.0; // Exact height of ruler panel

    let central_panel_y_min = central_panel_rect.min.y;
    let central_panel_y_max = central_panel_rect.max.y;

    // Draw the ruler segment of the cursor (full width)
    if cx >= full_timeline_area.min.x + 132.0 && cx <= full_timeline_area.max.x {
        ui.painter().line_segment(
            [egui::pos2(cx, ruler_y_min), egui::pos2(cx, ruler_y_max + 12.0)],
            egui::Stroke::new(2.0, egui::Color32::RED),
        );
    }

    // Draw the central panel segment of the cursor (restricted width)
    // This segment should start from the right of the sidebar.
    if cx >= central_panel_rect.min.x + TRACK_LIST_SIDEBAR_WIDTH + 24.0
      && cx <= central_panel_rect.max.x
    {
        ui.painter().line_segment(
            [egui::pos2(cx, central_panel_y_min - 12.0), egui::pos2(cx, central_panel_y_max + 8.0)],
            egui::Stroke::new(2.0, egui::Color32::RED),
        );
    }
}
