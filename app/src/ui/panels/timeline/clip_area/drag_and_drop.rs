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
                let drop_track_index = ((mouse_pos.y - content_rect.min.y
                    + editor_context.timeline.scroll_offset.y)
                    / (row_height + track_spacing))
                    .floor() as usize;

                let drop_in_frame = (drop_time_f64 * composition_fps).round() as u64;

                if let Some(comp_id) = editor_context.selection.composition_id {
                    // Find the track to drop onto
                    let mut current_tracks_for_drop = Vec::new();
                    if let Ok(proj_read) = project.read() {
                        if let Some(comp) = proj_read.compositions.iter().find(|c| c.id == comp_id)
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
                                                    asset.fps.unwrap_or(30.0), // Use asset fps or default
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

                        if let (Some(new_clip), Some(drop_out)) = (new_clip_opt, drop_out_frame_opt)
                        {
                            if let Err(e) = project_service.add_clip_to_track(
                                comp_id,
                                track.id,
                                new_clip,
                                drop_in_frame,
                                drop_out,
                            ) {
                                log::error!("Failed to add entity to track: {:?}", e);
                                editor_context.interaction.active_modal_error = Some(e.to_string());
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
