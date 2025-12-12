use egui::Ui;
use library::model::project::asset::AssetKind;
use library::model::project::project::Project;
use library::model::project::property::PropertyValue;
use library::model::project::TrackClip;
use library::model::project::TrackClipKind;
use library::service::project_service::ProjectService;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::{action::HistoryManager, model::ui_types::DraggedItem, state::context::EditorContext};

#[allow(clippy::too_many_arguments)]
pub fn handle_area_interaction(
    ui: &mut Ui,
    response: &egui::Response,
    content_rect: egui::Rect,
    editor_context: &mut EditorContext,
    project: &Arc<RwLock<Project>>,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    pixels_per_unit: f32,
    composition_fps: f64,
    num_tracks: usize,
    row_height: f32,
    track_spacing: f32,
) {
    if response.hovered() {
        // Scroll/Zoom interaction
        let scroll_delta = ui.input(|i| i.raw_scroll_delta);
        if ui.input(|i| i.modifiers.ctrl) && scroll_delta.y != 0.0 {
            let zoom_factor = if scroll_delta.y > 0.0 { 1.1 } else { 0.9 };

            const MAX_PIXELS_PER_FRAME_DESIRED: f32 = 20.0; // Desired pixels per frame at max zoom
            let max_h_zoom_value = (MAX_PIXELS_PER_FRAME_DESIRED * composition_fps as f32)
                / editor_context.timeline.pixels_per_second;

            // Calculate min zoom to fit the entire duration
            let mut duration_sec = 60.0; // Default fallback
            if let Ok(proj) = project.read() {
                if let Some(comp) = editor_context.get_current_composition(&proj) {
                    duration_sec = comp.duration;
                }
            }
            let min_possible_zoom = content_rect.width()
                / (duration_sec as f32 * editor_context.timeline.pixels_per_second);
            // Allow zooming out slightly more than fitting (e.g. 0.8x of tight fit) or at least 0.01
            let min_h_zoom_value = min_possible_zoom.min(0.01);

            // Zoom to Mouse Cursor Logic
            if let Some(mouse_pos) = response.hover_pos() {
                let mouse_x_in_content = mouse_pos.x - content_rect.min.x;
                let old_scroll_x = editor_context.timeline.scroll_offset.x;

                // Calculate the exact time under the mouse cursor before zooming
                let time_at_mouse = (mouse_x_in_content + old_scroll_x) / pixels_per_unit;

                // Apply Zoom
                let old_zoom = editor_context.timeline.h_zoom;
                let new_zoom = (old_zoom * zoom_factor).clamp(min_h_zoom_value, max_h_zoom_value);
                editor_context.timeline.h_zoom = new_zoom;

                // Calculate new scroll offset to keep the same time under the mouse
                // new_pixels_per_unit = pixels_per_second * new_zoom
                // We can derive new_pixels_per_unit from the ratio of zooms to avoid recalculating PPS
                let zoom_ratio = new_zoom / old_zoom;
                let new_pixels_per_unit = pixels_per_unit * zoom_ratio;

                let new_scroll_x = (time_at_mouse * new_pixels_per_unit) - mouse_x_in_content;

                editor_context.timeline.scroll_offset.x = new_scroll_x.max(0.0);
            } else {
                // Fallback if no mouse position (shouldn't happen with hover, but safe fallback)
                editor_context.timeline.h_zoom = (editor_context.timeline.h_zoom * zoom_factor)
                    .clamp(min_h_zoom_value, max_h_zoom_value);
            }

            if scroll_delta.x != 0.0 {
                editor_context.timeline.scroll_offset.x -= scroll_delta.x;
                // Clamp timeline_scroll_offset.x to prevent scrolling left past 0s
                editor_context.timeline.scroll_offset.x =
                    editor_context.timeline.scroll_offset.x.max(0.0);
            }
        }
        if response.dragged_by(egui::PointerButton::Middle) {
            editor_context.timeline.scroll_offset.x -= response.drag_delta().x;
            editor_context.timeline.scroll_offset.y += response.drag_delta().y;

            // Clamp timeline_scroll_offset.x to prevent scrolling left past 0s
            editor_context.timeline.scroll_offset.x =
                editor_context.timeline.scroll_offset.x.max(0.0);

            // Clamp timeline_scroll_offset.y to prevent scrolling out of bounds vertically
            let max_scroll_y =
                (num_tracks as f32 * (row_height + track_spacing)) - content_rect.height();
            editor_context.timeline.scroll_offset.y = editor_context
                .timeline
                .scroll_offset
                .y
                .clamp(-max_scroll_y.max(0.0), 0.0);
        }

        // Logic for adding entity to track on drag-drop
        if ui.input(|i| i.pointer.any_released()) {
            if let Some(dragged_item) = &editor_context.interaction.dragged_item {
                if let Some(mouse_pos) = response.hover_pos() {
                    let drop_time_f64 = ((mouse_pos.x
                        - content_rect.min.x
                        - editor_context.timeline.scroll_offset.x)
                        / pixels_per_unit)
                        .max(0.0) as f64;
                    let drop_track_index = ((mouse_pos.y
                        - content_rect.min.y
                        - editor_context.timeline.scroll_offset.y)
                        / (row_height + track_spacing))
                        .floor() as usize;

                    let drop_in_frame = (drop_time_f64 * composition_fps).round() as u64;

                    if let Some(comp_id) = editor_context.selection.composition_id {
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
                                    editor_context.interaction.active_modal_error =
                                        Some(e.to_string());
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

    // Capture right-click position BEFORE the context menu opens/draws
    if response.hovered() && ui.input(|i| i.pointer.button_pressed(egui::PointerButton::Secondary))
    {
        if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
            editor_context.interaction.context_menu_open_pos = Some(pos);
        }
    }

    // Context Menu for adding Text/Shape
    response.context_menu(|ui| {
        let mut drop_in_frame =
            (editor_context.timeline.current_time * composition_fps as f32).round() as u64;
        let mut drop_track_index_opt = None;

        // Try to recover clicked position
        if let Some(pos) = editor_context.interaction.context_menu_open_pos {
            // Re-calculate frame and track from pos
            let local_x = pos.x - content_rect.min.x - editor_context.timeline.scroll_offset.x;
            let time_at_click = (local_x / pixels_per_unit).max(0.0);
            drop_in_frame = (time_at_click * composition_fps as f32).round() as u64;

            let local_y = pos.y - content_rect.min.y - editor_context.timeline.scroll_offset.y;
            let track_idx = (local_y / (row_height + track_spacing)).floor() as isize;
            if track_idx >= 0 && track_idx < num_tracks as isize {
                drop_track_index_opt = Some(track_idx as usize);
            }
        }

        if ui.button("Add Text Layer").clicked() {
            let duration_sec = 5.0; // Default duration
            let duration_frames = (duration_sec * composition_fps).round() as u64;
            let drop_out_frame = drop_in_frame + duration_frames;

            let text_clip =
                TrackClip::create_text("this is sample text", drop_in_frame, drop_out_frame);

            // Determine Target Track
            let mut track_id_opt = None;
            if let Ok(proj_read) = project.read() {
                if let Some(comp_id) = editor_context.selection.composition_id {
                    if let Some(comp) = proj_read.compositions.iter().find(|c| c.id == comp_id) {
                        // If we have a calculated track index, use it
                        if let Some(idx) = drop_track_index_opt {
                            if let Some(track) = comp.tracks.get(idx) {
                                track_id_opt = Some(track.id);
                            }
                        }
                        // Fallback to first track or track 0 if invalid index
                        if track_id_opt.is_none() {
                            if let Some(first_track) = comp.tracks.first() {
                                track_id_opt = Some(first_track.id);
                            }
                        }
                    }
                }
            }

            if let Some(track_id) = track_id_opt {
                if let Some(comp_id) = editor_context.selection.composition_id {
                    if let Err(e) = project_service.add_clip_to_track(
                        comp_id,
                        track_id,
                        text_clip,
                        drop_in_frame,
                        drop_out_frame,
                    ) {
                        log::error!("Failed to add text clip: {}", e);
                    } else {
                        let current_state = project_service.get_project().read().unwrap().clone();
                        history_manager.push_project_state(current_state);
                    }
                }
            }
            ui.close();
        }

        if ui.button("Add Shape Layer").clicked() {
            let duration_sec = 5.0; // Default duration
            let duration_frames = (duration_sec * composition_fps).round() as u64;
            let drop_out_frame = drop_in_frame + duration_frames;

            let shape_clip = TrackClip::create_shape(drop_in_frame, drop_out_frame);

            // Determine Target Track
            let mut track_id_opt = None;
            if let Ok(proj_read) = project.read() {
                if let Some(comp_id) = editor_context.selection.composition_id {
                    if let Some(comp) = proj_read.compositions.iter().find(|c| c.id == comp_id) {
                        // If we have a calculated track index, use it
                        if let Some(idx) = drop_track_index_opt {
                            if let Some(track) = comp.tracks.get(idx) {
                                track_id_opt = Some(track.id);
                            }
                        }
                        // Fallback
                        if track_id_opt.is_none() {
                            if let Some(first_track) = comp.tracks.first() {
                                track_id_opt = Some(first_track.id);
                            }
                        }
                    }
                }
            }

            if let Some(track_id) = track_id_opt {
                if let Some(comp_id) = editor_context.selection.composition_id {
                    if let Err(e) = project_service.add_clip_to_track(
                        comp_id,
                        track_id,
                        shape_clip,
                        drop_in_frame,
                        drop_out_frame,
                    ) {
                        log::error!("Failed to add shape clip: {}", e);
                    } else {
                        let current_state = project_service.get_project().read().unwrap().clone();
                        history_manager.push_project_state(current_state);
                    }
                }
            }
            ui.close();
        }
    });
}
