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

use super::super::utils::flatten::flatten_tracks;

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
                    let display_tracks = flatten_tracks(&current_tracks_for_drop, &editor_context.timeline.expanded_tracks);

                    // We need to determine if we are dropping onto an existing track or "creating a new line"
                    // The original request says "drag and drop should add a new line".
                    // However, standard DAW behavior usually allows dropping ON a track to add to it, OR dropping below to create new.
                    // The request "multiple layers cannot be registered on 1 line" suggests we prefer new tracks if occupied?
                    // Or maybe it just means "if I drop somewhere, don't overlap, make a new track".
                    // Let's implement logic:
                    // 1. If dropped ON a track (row): try to add to that track.
                    // 2. If dropped BELOW all tracks: create new track at root level.
                    // 3. (Refinement) If `add_clip_to_track` fails (overlap), create a new track?
                    //    Currently `add_clip_to_track` might fail if overlaps are not allowed (I'd need to check implementation).
                    //    But the user specifically requested "drag and drop -> add new line".
                    //    Maybe ALWAYS create a new track when dropping from assets?
                    //    "レイヤーは1行に複数登録できないため" -> "Because layers cannot be registered multiple per line"
                    //    This strongly suggests: 1 Track = 1 Clip (at a time?). Or maybe just "don't overlap".
                    //    If the user implies "Add new layer", they mean "Add new Track".
                    //    Let's Try to add to the target track. If that fails or if the user intends to create a new track (maybe holding Shift?), we do that.
                    //    Actually, simpler: If we drop on a valid row, we try to add to that row. If we drop below, we add new track.
                    //    Wait, "drag and drop ... new line ... add there".
                    //    Let's assume the user wants new tracks for dropped assets to avoid conflict.
                    //    But that might be annoying.
                    //    Let's look at `add_clip_to_track` in `ProjectService`. It likely checks for overlaps.
                    //    If I interpret "drag and drop should add new line" strictly: Every drop creates a track.
                    //    But let's support dropping on existing tracks too if empty space.

                    let target_track_id_opt = if visible_row_index < display_tracks.len() {
                        Some(display_tracks[visible_row_index].track.id)
                    } else {
                        None // Dropped below visible tracks
                    };

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
                                                drop_in_frame as i64, // source_begin_frame = drop_in_frame
                                                duration_frames, // Use asset duration
                                                asset.fps.unwrap_or(30.0), // Use asset fps or default
                                                comp_width as u32,
                                                comp_height as u32,
                                            );
                                            if let (Some(w), Some(h)) =
                                                (asset.width, asset.height)
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
                                            );
                                            image_clip.source_begin_frame = 0; // Images are static, so 0 is fine, or arguably doesn't matter. But let's keep 0 as explicit.
                                            if let (Some(w), Some(h)) =
                                                (asset.width, asset.height)
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
                                                     0, 0, 0, None, 0.0,
                                                     library::model::project::property::PropertyMap::new(),
                                                     Vec::new(), // styles
                                                     Vec::new()  // effects
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

                    if let (Some(new_clip), Some(_drop_out)) = (new_clip_opt, drop_out_frame_opt)
                    {
                        // Strategy:
                        // 1. If target_track_id exists, try to add there.
                        // 2. If it fails (overlap or other error) OR target_track_id is None, create NEW track.
                        //    But for "None" (dropped at bottom), we definitively create new track.

                        let mut success = false;
                        if let Some(track_id) = target_track_id_opt {
                             if let Ok(_) = project_service.add_clip_to_track(
                                comp_id,
                                track_id,
                                new_clip.clone(),
                                new_clip.in_frame,
                                new_clip.out_frame,
                            ) {
                                success = true;
                            }
                        }

                        if !success {
                             // Create new track
                             // Where to insert?
                             // If target_track_id was some, maybe insert *after* it?
                             // Currently `add_track` appends to root.
                             // Implementing insertion at index or as child requires more service methods.
                             // For now, let's append to root.
                             // Wait, if I drop on a folder, maybe I want to add *into* the folder?
                             // Current API: add_track(comp_id, name).

                             let new_track_id = match project_service.add_track(comp_id, "New Layer") {
                                 Ok(id) => Some(id),
                                 Err(e) => {
                                     log::error!("Failed to create new track: {:?}", e);
                                     None
                                 }
                             };

                             if let Some(ntid) = new_track_id {
                                 if let Err(e) = project_service.add_clip_to_track(
                                    comp_id,
                                    ntid,
                                    new_clip.clone(),
                                    drop_in_frame, // Use original frames
                                    drop_in_frame + (new_clip.out_frame - new_clip.in_frame),
                                ) {
                                    log::error!("Failed to add entity to NEW track: {:?}", e);
                                    // Cleanup empty track?
                                    project_service.remove_track(comp_id, ntid).ok();
                                    editor_context.interaction.active_modal_error = Some(e.to_string());
                                } else {
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
