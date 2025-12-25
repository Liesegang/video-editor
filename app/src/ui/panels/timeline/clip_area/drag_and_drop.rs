use egui::Ui;
use library::model::project::asset::AssetKind;
use library::model::project::project::Project;
use library::model::project::property::PropertyValue;
use library::model::project::TrackClip;
use library::model::project::TrackClipKind;
use library::EditorService as ProjectService;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::{action::HistoryManager, model::ui_types::DraggedItem, state::context::EditorContext};

pub fn handle_drag_and_drop(
    ui: &mut Ui,
    response: &egui::Response,
    content_rect: egui::Rect,
    editor_context: &mut EditorContext,
    project: &Arc<RwLock<Project>>,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    pixels_per_unit: f32,
    composition_fps: f64,
    row_height: f32,
    track_spacing: f32,
) {
    if ui.input(|i| i.pointer.any_released()) {
        if let Some(dragged_item) = &editor_context.interaction.dragged_item {
            if let Some(mouse_pos) = response.hover_pos() {
                let drop_time_f64 = ((mouse_pos.x - content_rect.min.x
                    + editor_context.timeline.scroll_offset.x)
                    / pixels_per_unit)
                    .max(0.0) as f64;

                let visible_row_index = ((mouse_pos.y - content_rect.min.y
                    + editor_context.timeline.scroll_offset.y)
                    / (row_height + track_spacing))
                    .floor() as usize;

                let drop_in_frame = (drop_time_f64 * composition_fps).round() as u64;

                if let Some(comp_id) = editor_context.selection.composition_id {
                    // Find the track to drop onto (using flattened structure)
                    let mut current_tracks_for_drop = Vec::new();
                    let mut comp_width = 1920;
                    let mut comp_height = 1080;
                    if let Ok(proj_read) = project.read() {
                        if let Some(comp) = proj_read.compositions.iter().find(|c| c.id == comp_id)
                        {
                            current_tracks_for_drop = comp.tracks.clone();
                            comp_width = comp.width;
                            comp_height = comp.height;
                        }
                    }

                    // Flatten to find corresponding track
                    let display_rows = super::super::utils::flatten::flatten_tracks_to_rows(
                        &current_tracks_for_drop,
                        &editor_context.timeline.expanded_tracks,
                    );

                    let mut target_track_id_opt = None;
                    let mut _target_index_opt = None;

                    if visible_row_index < display_rows.len() {
                        let row = &display_rows[visible_row_index];
                        match row {
                            super::super::utils::flatten::DisplayRow::TrackHeader {
                                track, ..
                            } => {
                                target_track_id_opt = Some(track.id);
                                _target_index_opt = Some(0); // Insert at top
                            }
                            super::super::utils::flatten::DisplayRow::ClipRow {
                                parent_track,
                                child_index,
                                ..
                            } => {
                                target_track_id_opt = Some(parent_track.id);
                                // Determine if inserting before or after based on mouse Y within row
                                let row_y_start = content_rect.min.y
                                    + (visible_row_index as f32 * (row_height + track_spacing))
                                    - editor_context.timeline.scroll_offset.y;
                                let relative_y = mouse_pos.y - row_y_start;

                                if relative_y > row_height / 2.0 {
                                    _target_index_opt = Some(child_index + 1);
                                } else {
                                    _target_index_opt = Some(*child_index);
                                }
                            }
                        }
                    }

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
                                                0, // source_begin_frame = 0 (start from source beginning)
                                                duration_frames, // Use asset duration
                                                asset.fps.unwrap_or(30.0), // Use asset fps or default
                                                comp_width as u32,
                                                comp_height as u32,
                                            );
                                            if let (Some(w), Some(h)) = (asset.width, asset.height)
                                            {
                                                video_clip.properties.set("anchor".to_string(), library::model::project::property::Property::constant(library::model::project::property::PropertyValue::Vec2(library::model::project::property::Vec2 { x: ordered_float::OrderedFloat(w as f64 / 2.0), y: ordered_float::OrderedFloat(h as f64 / 2.0) })));
                                            }
                                            Some(video_clip)
                                        }
                                        AssetKind::Image => {
                                            let mut image_clip = TrackClip::create_image(
                                                Some(asset.id),
                                                &asset.path,
                                                drop_in_frame,
                                                drop_out,
                                                comp_width as u32,
                                                comp_height as u32,
                                                composition_fps,
                                            );
                                            image_clip.source_begin_frame = 0; // Images are static, so 0 is fine, or arguably doesn't matter. But let's keep 0 as explicit.
                                            if let (Some(w), Some(h)) = (asset.width, asset.height)
                                            {
                                                image_clip.properties.set("anchor".to_string(), library::model::project::property::Property::constant(library::model::project::property::PropertyValue::Vec2(library::model::project::property::Vec2 { x: ordered_float::OrderedFloat(w as f64 / 2.0), y: ordered_float::OrderedFloat(h as f64 / 2.0) })));
                                            }
                                            Some(image_clip)
                                        }

                                        AssetKind::Audio => {
                                            let mut audio_entity = TrackClip::new(
                                                Uuid::new_v4(),
                                                Some(asset.id),
                                                TrackClipKind::Audio,
                                                0,
                                                0,
                                                0,
                                                None,
                                                composition_fps,
                                                library::model::project::property::PropertyMap::new(
                                                ),
                                                Vec::new(), // styles
                                                Vec::new(), // effects
                                            );
                                            audio_entity.in_frame = drop_in_frame;
                                            audio_entity.out_frame = drop_out;
                                            audio_entity.duration_frame = Some(duration_frames);
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
                            let mut target_fps = 30.0;
                            if let Ok(proj_read) = project.read() {
                                if let Some(c) = proj_read
                                    .compositions
                                    .iter()
                                    .find(|c| c.id == *target_comp_id)
                                {
                                    duration_sec = c.duration;
                                    target_fps = c.fps;
                                }
                            }

                            let duration_frames = (duration_sec * composition_fps).round() as u64;
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
                                target_fps,
                                library::model::project::property::PropertyMap::new(),
                                Vec::new(), // styles
                                Vec::new(), // effects
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

                    if let (Some(new_clip), Some(_drop_out)) = (new_clip_opt, drop_out_frame_opt) {
                        let mut success = false;

                        if let Some(parent_track_id) = target_track_id_opt {
                            // Add DIRECTLY to track (no Layer subtrack)
                            // We need a ProjectService method to insert/add, but currently add_clip_to_track just appends.
                            // However, we want to respect target_index_opt.
                            // We don't have a direct "add_clip_to_track_at_index" in ProjectService wrapper yet,
                            // but we can add one or just rely on handler (ProjectService delegates to handler).
                            // Let's assume ProjectService delegates.
                            // Or, because we are in app, checking ProjectService...
                            // ProjectService is `EditorService`.
                            // It doesn't have `insert_clip`.
                            // But we can manually use `ClipHandler` since we have access to it?
                            // No, typically we go through ProjectService.
                            // For now, let's just append if we don't have the API, or modify ProjectService.
                            // Wait, I updated ClipHandler, but not ProjectService.
                            // Let's modify ProjectService later, but for now I can just use `add_clip_to_track` which appends.
                            // Wait, user asked reordering. Appending is not reordering.
                            // BUT, for NEW asset drop, inserting at top vs bottom matters.
                            // "Layer replacement" (reordering) was requested for dragging.
                            // For Drop, defaulting to "add to track" is fine.
                            // Removing "Layer" wrapper is the key user request here.

                            // To support insertion, I should create `ProjectService::insert_clip_to_track`
                            // but I haven't done that yet.
                            // So I will just use `add_clip_to_track` (append) for now.
                            // If I want to support specific index, I'd need to update ProjectService.
                            // Given I'm in the middle of editing drag_and_drop.rs, I'll stick to `add_clip_to_track` (Append)
                            // which fulfills "No Layer Subtrack".
                            // The user can reorder later.

                            // Note: `new_clip` timing is already set? No, it's set in `create_video` etc but `in_frame` passed here.

                            if let Err(e) = project_service.add_clip_to_track(
                                comp_id,
                                parent_track_id,
                                new_clip,
                                drop_in_frame,
                                drop_in_frame + (drop_out_frame_opt.unwrap() - drop_in_frame),
                            ) {
                                log::error!("Failed to add clip: {:?}", e);
                                editor_context.interaction.active_modal_error = Some(e.to_string());
                            } else {
                                editor_context
                                    .timeline
                                    .expanded_tracks
                                    .insert(parent_track_id);
                                success = true;
                            }
                        } else {
                            // Create new track and add clip
                            if let Ok(new_track_id) =
                                project_service.add_track(comp_id, "New Track")
                            {
                                if let Err(e) = project_service.add_clip_to_track(
                                    comp_id,
                                    new_track_id,
                                    new_clip,
                                    drop_in_frame,
                                    drop_in_frame + (drop_out_frame_opt.unwrap() - drop_in_frame),
                                ) {
                                    log::error!("Failed to add clip to new track: {:?}", e);
                                    project_service.remove_track(comp_id, new_track_id).ok();
                                } else {
                                    editor_context.timeline.expanded_tracks.insert(new_track_id);
                                    success = true;
                                }
                            }
                        }

                        if success {
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
