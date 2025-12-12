use egui::{epaint::StrokeKind, Ui};
use egui_phosphor::regular as icons;
use library::model::project::asset::AssetKind;
use library::model::project::project::Project;
use library::model::project::property::PropertyValue;
use library::model::project::TrackClip;
use library::model::project::TrackClipKind;
use library::service::project_service::ProjectService;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::{
    action::HistoryManager,
    model::ui_types::{DraggedItem, TimelineClip},
    state::context::EditorContext,
};

const EDGE_DRAG_WIDTH: f32 = 5.0;

#[allow(clippy::too_many_arguments)]
pub fn show_clip_area(
    ui_content: &mut Ui,
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project_service: &mut ProjectService,
    project: &Arc<RwLock<Project>>,
    pixels_per_unit: f32,
    num_tracks: usize,
    row_height: f32,
    track_spacing: f32,
    composition_fps: f64,
) {
    let (content_rect_for_clip_area, response) =
        ui_content.allocate_at_least(ui_content.available_size(), egui::Sense::click_and_drag());

    let is_dragging_item = editor_context.dragged_item.is_some();
    let mut clicked_on_entity = false;

    // --- Data collection for entities ---
    let mut all_clips: Vec<(Uuid, TrackClip)> = Vec::new(); // all_entities -> all_clips
    let mut current_tracks: Vec<library::model::project::Track> = Vec::new();
    let selected_composition_id = editor_context.selected_composition_id;
    if let Some(comp_id) = selected_composition_id {
        if let Ok(proj_read) = project.read() {
            if let Some(comp) = proj_read.compositions.iter().find(|c| c.id == comp_id) {
                current_tracks = comp.tracks.clone();
            }
        }
    }

    for track in &current_tracks {
        for clip in &track.clips {
            // entity -> clip
            all_clips.push((track.id, clip.clone())); // all_entities -> all_clips
        }
    }
    // --- End Data collection for entities ---

    // --- Drawing of track backgrounds ---
    let painter = ui_content.painter_at(content_rect_for_clip_area);

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
    // --- End Drawing of track backgrounds ---

    // --- Main Interaction Block (for overall clip area, e.g., scroll, zoom, asset drop) ---
    if response.hovered() {
        // Scroll/Zoom interaction
        let scroll_delta = ui_content.input(|i| i.raw_scroll_delta);
        if ui_content.input(|i| i.modifiers.ctrl) && scroll_delta.y != 0.0 {
            let zoom_factor = if scroll_delta.y > 0.0 { 1.1 } else { 0.9 };

            const MAX_PIXELS_PER_FRAME_DESIRED: f32 = 20.0; // Desired pixels per frame at max zoom
            let max_h_zoom_value = (MAX_PIXELS_PER_FRAME_DESIRED * composition_fps as f32)
                / editor_context.timeline_pixels_per_second;

            editor_context.timeline_h_zoom =
                (editor_context.timeline_h_zoom * zoom_factor).clamp(0.1, max_h_zoom_value);

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

            // Clamp timeline_scroll_offset.y to prevent scrolling out of bounds vertically
            let max_scroll_y = (num_tracks as f32 * (row_height + track_spacing))
                - content_rect_for_clip_area.height();
            editor_context.timeline_scroll_offset.y = editor_context
                .timeline_scroll_offset
                .y
                .clamp(-max_scroll_y.max(0.0), 0.0);
        }

        // Logic for adding entity to track on drag-drop
        if ui_content.input(|i| i.pointer.any_released()) {
            if let Some(dragged_item) = &editor_context.dragged_item {
                if let Some(mouse_pos) = response.hover_pos() {
                    let drop_time_f64 = ((mouse_pos.x
                        - content_rect_for_clip_area.min.x
                        - editor_context.timeline_scroll_offset.x)
                        / pixels_per_unit)
                        .max(0.0) as f64;
                    let drop_track_index = ((mouse_pos.y
                        - content_rect_for_clip_area.min.y
                        - editor_context.timeline_scroll_offset.y)
                        / (row_height + track_spacing))
                        .floor() as usize;

                    let drop_in_frame = (drop_time_f64 * composition_fps).round() as u64;

                    if let Some(comp_id) = editor_context.selected_composition_id {
                        // Find the track to drop onto
                        let mut current_tracks_for_drop = Vec::new();
                        if let Ok(proj_read) = project.read() {
                            if let Some(comp) =
                                proj_read.compositions.iter().find(|c| c.id == comp_id)
                            {
                                current_tracks_for_drop = comp.tracks.clone();
                            }
                        }

                        if let Some(track) = current_tracks_for_drop.get(drop_track_index) {
                            let mut new_clip_opt: Option<TrackClip> = None;
                            let mut drop_out_frame_opt: Option<u64> = None;

                            match dragged_item {
                                DraggedItem::Asset(asset_id) => {
                                    // Retrieve asset
                                    if let Ok(proj_read) = project.read() {
                                        if let Some(asset) =
                                            proj_read.assets.iter().find(|a| a.id == *asset_id)
                                        {
                                            let duration_sec = asset.duration.unwrap_or(5.0); // Default 5s if unknown
                                            let duration_frames =
                                                (duration_sec * composition_fps).round() as u64;
                                            let drop_out = drop_in_frame + duration_frames;
                                            drop_out_frame_opt = Some(drop_out);

                                            new_clip_opt = match asset.kind {
                                                AssetKind::Video => {
                                                    let mut video_clip = TrackClip::create_video(
                                                        Some(asset.id),
                                                        &asset.path,
                                                        drop_in_frame,
                                                        drop_out,
                                                        drop_in_frame, // source_begin_frame = drop_in_frame
                                                        duration_frames, // Use asset duration
                                                    );
                                                    if let (Some(w), Some(h)) =
                                                        (asset.width, asset.height)
                                                    {
                                                        video_clip.properties.set("anchor_x".to_string(), library::model::project::property::Property::constant(library::model::project::property::PropertyValue::Number(ordered_float::OrderedFloat(w as f64 / 2.0))));
                                                        video_clip.properties.set("anchor_y".to_string(), library::model::project::property::Property::constant(library::model::project::property::PropertyValue::Number(ordered_float::OrderedFloat(h as f64 / 2.0))));
                                                    }
                                                    Some(video_clip)
                                                }
                                                AssetKind::Image => {
                                                    let mut image_clip = TrackClip::create_image(
                                                        Some(asset.id),
                                                        &asset.path,
                                                        drop_in_frame,
                                                        drop_out,
                                                    );
                                                    image_clip.source_begin_frame = 0; // Images are static, so 0 is fine, or arguably doesn't matter. But let's keep 0 as explicit.
                                                    if let (Some(w), Some(h)) =
                                                        (asset.width, asset.height)
                                                    {
                                                        image_clip.properties.set("anchor_x".to_string(), library::model::project::property::Property::constant(library::model::project::property::PropertyValue::Number(ordered_float::OrderedFloat(w as f64 / 2.0))));
                                                        image_clip.properties.set("anchor_y".to_string(), library::model::project::property::Property::constant(library::model::project::property::PropertyValue::Number(ordered_float::OrderedFloat(h as f64 / 2.0))));
                                                    }
                                                    Some(image_clip)
                                                }

                                                AssetKind::Audio => {
                                                    let mut audio_entity = TrackClip::new(
                                                        Uuid::new_v4(),
                                                        Some(asset.id),
                                                        TrackClipKind::Audio,
                                                        0, 0, 0, None, 0.0,
                                                        library::model::project::property::PropertyMap::new(),
                                                        Vec::new()
                                                    );
                                                    audio_entity.in_frame = drop_in_frame;
                                                    audio_entity.out_frame = drop_out;
                                                    audio_entity.duration_frame =
                                                        Some(duration_frames);
                                                    audio_entity.set_constant_property(
                                                        "file_path",
                                                        PropertyValue::String(asset.path.clone()),
                                                    );
                                                    Some(audio_entity)
                                                }
                                                _ => None, // Not yet supported or 'Other'
                                            };
                                        }
                                    }
                                }
                                DraggedItem::Composition(target_comp_id) => {
                                    // Create Composition Clip
                                    // We should check duration of that composition
                                    let mut duration_sec = 10.0;
                                    if let Ok(proj_read) = project.read() {
                                        if let Some(c) = proj_read
                                            .compositions
                                            .iter()
                                            .find(|c| c.id == *target_comp_id)
                                        {
                                            duration_sec = c.duration;
                                        }
                                    }

                                    let duration_frames =
                                        (duration_sec * composition_fps).round() as u64;
                                    let drop_out = drop_in_frame + duration_frames;
                                    drop_out_frame_opt = Some(drop_out);

                                    let mut comp_entity = TrackClip::new(
                                        Uuid::new_v4(),
                                        Some(*target_comp_id),
                                        TrackClipKind::Composition,
                                        0,
                                        0,
                                        0,
                                        None,
                                        0.0,
                                        library::model::project::property::PropertyMap::new(),
                                        Vec::new(),
                                    );
                                    comp_entity.in_frame = drop_in_frame;
                                    comp_entity.out_frame = drop_out;
                                    comp_entity.duration_frame = Some(duration_frames);
                                    comp_entity.set_constant_property(
                                        "composition_id",
                                        PropertyValue::String(target_comp_id.to_string()),
                                    );
                                    new_clip_opt = Some(comp_entity);
                                }
                            }

                            if let (Some(new_clip), Some(drop_out)) =
                                (new_clip_opt, drop_out_frame_opt)
                            {
                                if let Err(e) = project_service.add_clip_to_track(
                                    comp_id,
                                    track.id,
                                    new_clip,
                                    drop_in_frame,
                                    drop_out,
                                ) {
                                    log::error!("Failed to add entity to track: {:?}", e);
                                    editor_context.active_modal_error = Some(e.to_string());
                                } else {
                                    let current_state =
                                        project_service.get_project().read().unwrap().clone();
                                    history_manager.push_project_state(current_state);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    // --- End Main Interaction Block ---

    // --- Loop for drawing and interacting with entities ---
    for track_in_all_entities in &current_tracks {
        let clip_track_index = current_tracks
            .iter()
            .position(|t| t.id == track_in_all_entities.id)
            .map(|idx| idx as f32)
            .unwrap_or(0.0);

        for (entity_track_id, entity) in all_clips // all_entities -> all_clips
            .iter()
            .filter(|(t_id, _)| *t_id == track_in_all_entities.id)
        {
            // Determine Color based on kind
            let clip_color = match entity.kind {
                TrackClipKind::Video => egui::Color32::from_rgb(100, 150, 255), // Blue
                TrackClipKind::Audio => egui::Color32::from_rgb(100, 255, 150), // Green
                TrackClipKind::Image => egui::Color32::from_rgb(255, 100, 150), // Pink
                TrackClipKind::Composition => egui::Color32::from_rgb(255, 150, 255), // Magenta
                TrackClipKind::Text => egui::Color32::from_rgb(255, 200, 100),  // Orange/Yellow
                _ => egui::Color32::GRAY,
            };

            let gc = TimelineClip {
                id: entity.id,
                name: entity.kind.to_string(), // Use Display impl
                track_id: *entity_track_id,
                in_frame: entity.in_frame,   // u64
                out_frame: entity.out_frame, // u64
                timeline_duration_frames: entity.out_frame.saturating_sub(entity.in_frame), // u64
                source_begin_frame: entity.source_begin_frame, // u64
                duration_frame: entity.duration_frame, // Option<u64>
                color: clip_color,
                position: [
                    entity.properties.get_f32("position_x").unwrap_or(960.0),
                    entity.properties.get_f32("position_y").unwrap_or(540.0),
                ],
                scale_x: entity.properties.get_f32("scale_x").unwrap_or(100.0),
                scale_y: entity.properties.get_f32("scale_y").unwrap_or(100.0),
                anchor_x: entity.properties.get_f32("anchor_x").unwrap_or(0.0),
                anchor_y: entity.properties.get_f32("anchor_y").unwrap_or(0.0),
                opacity: entity.properties.get_f32("opacity").unwrap_or(100.0),
                rotation: entity.properties.get_f32("rotation").unwrap_or(0.0),
                asset_id: None, // We don't have asset_id stored on clip yet
                width: None,
                height: None,
            };

            let initial_x = content_rect_for_clip_area.min.x
                + (gc.in_frame as f32 / composition_fps as f32) * pixels_per_unit
                - editor_context.timeline_scroll_offset.x;
            let initial_y = content_rect_for_clip_area.min.y
                + editor_context.timeline_scroll_offset.y
                + clip_track_index * (row_height + track_spacing);
            let initial_clip_rect = egui::Rect::from_min_size(
                egui::pos2(initial_x, initial_y),
                egui::vec2(
                    (gc.timeline_duration_frames as f32 / composition_fps as f32) * pixels_per_unit,
                    row_height,
                ),
            );

            // --- Interaction for clips ---
            // Define clip_resp using the initial_clip_rect for hit detection
            let clip_resp = ui_content.interact(
                initial_clip_rect,
                egui::Id::new(gc.id),
                egui::Sense::click_and_drag(),
            );

            clip_resp.context_menu(|ui| {
                if ui.button(format!("{} Remove", icons::TRASH)).clicked() {
                    if let Some(comp_id) = editor_context.selected_composition_id {
                        if let Err(e) =
                            project_service.remove_clip_from_track(comp_id, gc.track_id, gc.id)
                        {
                            log::error!("Failed to remove entity: {:?}", e);
                        } else {
                            editor_context.selected_entity_id = None;
                            let current_state =
                                project_service.get_project().read().unwrap().clone();
                            history_manager.push_project_state(current_state);
                            ui.ctx().request_repaint();
                            ui.close();
                        }
                    }
                }
            });

            // Create edge responses using initial_clip_rect for hit detection
            let left_edge_rect = egui::Rect::from_min_size(
                egui::pos2(initial_clip_rect.min.x, initial_clip_rect.min.y),
                egui::vec2(EDGE_DRAG_WIDTH, initial_clip_rect.height()),
            );
            let left_edge_resp = ui_content.interact(
                left_edge_rect,
                egui::Id::new(gc.id).with("left_edge"), // Unique ID for the left edge
                egui::Sense::drag(),
            );

            let right_edge_rect = egui::Rect::from_min_size(
                egui::pos2(
                    initial_clip_rect.max.x - EDGE_DRAG_WIDTH,
                    initial_clip_rect.min.y,
                ),
                egui::vec2(EDGE_DRAG_WIDTH, initial_clip_rect.height()),
            );
            let right_edge_resp = ui_content.interact(
                right_edge_rect,
                egui::Id::new(gc.id).with("right_edge"), // Unique ID for the right edge
                egui::Sense::drag(),
            );

            // Handle edge dragging (resize) - takes precedence over full entity drag
            if left_edge_resp.drag_started() || right_edge_resp.drag_started() {
                editor_context.is_resizing_entity = true;
                editor_context.selected_entity_id = Some(gc.id);
                editor_context.selected_track_id = Some(gc.track_id);
            }

            if editor_context.is_resizing_entity && editor_context.selected_entity_id == Some(gc.id)
            {
                let mut new_in_frame = gc.in_frame;
                let mut new_out_frame = gc.out_frame;

                // Source constraints
                let source_max_out_frame = if let Some(duration) = gc.duration_frame {
                    gc.source_begin_frame.saturating_add(duration)
                } else {
                    u64::MAX
                };

                // Convert pixel delta to frame delta
                let delta_x = if left_edge_resp.dragged() {
                    left_edge_resp.drag_delta().x
                } else if right_edge_resp.dragged() {
                    right_edge_resp.drag_delta().x
                } else {
                    0.0
                };

                let dt_frames_f32 = delta_x / pixels_per_unit * composition_fps as f32;
                let dt_frames = dt_frames_f32.round() as i64;

                if left_edge_resp.dragged() {
                    new_in_frame = ((new_in_frame as i64 + dt_frames).max(0) as u64)
                        .max(gc.source_begin_frame) // Cannot go before source begin frame
                        .min(new_out_frame.saturating_sub(1)); // Minimum 1 frame duration
                } else if right_edge_resp.dragged() {
                    new_out_frame =
                        ((new_out_frame as i64 + dt_frames).max(new_in_frame as i64 + 1) as u64) // Minimum 1 frame duration
                            .min(source_max_out_frame); // Cannot go beyond source duration
                }

                // Update if there's an actual change
                if new_in_frame != gc.in_frame || new_out_frame != gc.out_frame {
                    if let (Some(comp_id), Some(track_id)) = (
                        editor_context.selected_composition_id,
                        editor_context.selected_track_id,
                    ) {
                        project_service
                            .update_clip_time(comp_id, track_id, gc.id, new_in_frame, new_out_frame)
                            .ok();
                    }
                }
            }

            if left_edge_resp.drag_stopped() || right_edge_resp.drag_stopped() {
                editor_context.is_resizing_entity = false;
                let current_state = project_service.get_project().read().unwrap().clone();
                history_manager.push_project_state(current_state);
            }

            // Calculate display position (potentially adjusted for drag preview)
            let mut display_x = initial_x;
            let mut display_y = initial_y;

            // Adjust position for dragged entity preview
            if editor_context.selected_entity_id == Some(gc.id) && clip_resp.dragged() {
                // Adjust X position based on current drag delta
                display_x += clip_resp.drag_delta().x;

                // Adjust Y position based on hovered track
                if let Some(hovered_track_id) = editor_context.dragged_entity_hovered_track_id {
                    if let Some(hovered_track_index) =
                        current_tracks.iter().position(|t| t.id == hovered_track_id)
                    {
                        display_y = content_rect_for_clip_area.min.y
                            + editor_context.timeline_scroll_offset.y
                            + hovered_track_index as f32 * (row_height + track_spacing);
                    }
                }
            }

            let drawing_clip_rect = egui::Rect::from_min_size(
                egui::pos2(display_x, display_y),
                egui::vec2(
                    (gc.timeline_duration_frames as f32 / composition_fps as f32) * pixels_per_unit,
                    row_height,
                ),
            );

            // --- Drawing for clips (always) ---
            let is_sel_entity = editor_context.selected_entity_id == Some(gc.id); // Renamed to avoid conflict
            let color = gc.color;
            let transparent_color =
                egui::Color32::from_rgba_premultiplied(color.r(), color.g(), color.b(), 150);

            painter.rect_filled(drawing_clip_rect, 4.0, transparent_color); // Use drawing_clip_rect
            if is_sel_entity {
                painter.rect_stroke(
                    drawing_clip_rect, // Use drawing_clip_rect
                    4.0,
                    egui::Stroke::new(2.0, egui::Color32::WHITE),
                    StrokeKind::Middle,
                );
            }
            painter.text(
                drawing_clip_rect.center(), // Use drawing_clip_rect
                egui::Align2::CENTER_CENTER,
                &gc.name,
                egui::FontId::default(),
                egui::Color32::BLACK,
            );
            // --- End Drawing for clips ---

            // Cursor feedback for edge resizing
            if left_edge_resp.hovered() || right_edge_resp.hovered() {
                ui_content
                    .ctx()
                    .set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if !editor_context.is_resizing_entity && clip_resp.clicked() {
                editor_context.selected_entity_id = Some(gc.id);
                editor_context.selected_track_id = Some(gc.track_id);
                clicked_on_entity = true;
            }

            if !editor_context.is_resizing_entity && clip_resp.drag_started() {
                editor_context.selected_entity_id = Some(gc.id);
                editor_context.selected_track_id = Some(gc.track_id);
                editor_context.dragged_entity_original_track_id = Some(gc.track_id); // Store original track
                editor_context.dragged_entity_hovered_track_id = Some(gc.track_id); // Initially hovered is original track
                editor_context.dragged_entity_has_moved = false; // Reset move flag
            }
            if !editor_context.is_resizing_entity
                && clip_resp.dragged()
                && editor_context.selected_entity_id == Some(gc.id)
            {
                // Mark as moved if drag delta is non-zero
                if clip_resp.drag_delta().length_sq() > 0.0 {
                    editor_context.dragged_entity_has_moved = true;
                }
                // Handle horizontal movement (frame change)
                let dt_frames_f32 =
                    clip_resp.drag_delta().x / pixels_per_unit * composition_fps as f32;
                let dt_frames = dt_frames_f32.round() as i64;

                if let Some(comp_id) = editor_context.selected_composition_id {
                    if let Some(track_id) = editor_context.selected_track_id {
                        let new_in_frame = (gc.in_frame as i64 + dt_frames).max(0) as u64;
                        let new_out_frame =
                            (gc.out_frame as i64 + dt_frames).max(new_in_frame as i64) as u64;

                        project_service
                            .update_clip_time(comp_id, track_id, gc.id, new_in_frame, new_out_frame)
                            .ok();

                        // Also update source_begin_frame to shift the source content
                        let new_source_begin_frame =
                            (gc.source_begin_frame as i64 + dt_frames).max(0) as u64;
                        project_service
                            .update_clip_source_frames(
                                comp_id,
                                track_id,
                                gc.id,
                                new_source_begin_frame,
                                gc.duration_frame,
                            )
                            .ok();
                    }
                }

                // Handle vertical movement (track change detection)
                if let Some(mouse_pos) = ui_content.ctx().pointer_latest_pos() {
                    let current_y_in_clip_area = mouse_pos.y
                        - content_rect_for_clip_area.min.y
                        - editor_context.timeline_scroll_offset.y;

                    let hovered_track_index =
                        (current_y_in_clip_area / (row_height + track_spacing)).floor() as usize;

                    if let Some(comp_id) = editor_context.selected_composition_id {
                        if let Ok(proj_read) = project.read() {
                            if let Some(comp) =
                                proj_read.compositions.iter().find(|c| c.id == comp_id)
                            {
                                if let Some(hovered_track) = comp.tracks.get(hovered_track_index) {
                                    if editor_context.dragged_entity_hovered_track_id
                                        != Some(hovered_track.id)
                                    {
                                        editor_context.dragged_entity_hovered_track_id =
                                            Some(hovered_track.id);
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if !editor_context.is_resizing_entity
                && clip_resp.drag_stopped()
                && editor_context.selected_entity_id == Some(gc.id)
            {
                let mut moved_track = false;
                if let (Some(original_track_id), Some(hovered_track_id), Some(comp_id)) = (
                    editor_context.dragged_entity_original_track_id,
                    editor_context.dragged_entity_hovered_track_id,
                    editor_context.selected_composition_id,
                ) {
                    if original_track_id != hovered_track_id {
                        // Move entity to new track
                        if let Err(e) = project_service.move_clip_to_track(
                            comp_id,
                            original_track_id,
                            hovered_track_id,
                            gc.id,
                        ) {
                            log::error!("Failed to move entity to new track: {:?}", e);
                        } else {
                            editor_context.selected_track_id = Some(hovered_track_id); // Update selected track
                            moved_track = true;
                        }
                    }
                }

                // Push history if time changed or track moves (dragged)
                if moved_track || editor_context.dragged_entity_has_moved {
                    let current_state = project_service.get_project().read().unwrap().clone();
                    history_manager.push_project_state(current_state);
                }

                // Clear drag related states
                editor_context.dragged_entity_original_track_id = None;
                editor_context.dragged_entity_hovered_track_id = None;
            }
        }
    }
    // --- End Loop for drawing and interacting with entities ---

    // Final selection clearing logic
    if !editor_context.is_resizing_entity
        && response.clicked()
        && !clicked_on_entity
        && !is_dragging_item
    {
        editor_context.selected_entity_id = None;
    }
}
