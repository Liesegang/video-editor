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
                    // ===== PHASE 1: Read all needed data, extract owned values =====
                    let mut root_track_ids: Vec<Uuid> = Vec::new();
                    let mut comp_width = 1920u64;
                    let mut comp_height = 1080u64;
                    let mut target_track_id_opt: Option<Uuid> = None;
                    let mut new_clip_opt: Option<TrackClip> = None;
                    let mut drop_out_frame_opt: Option<u64> = None;
                    let mut calculated_insert_index: Option<usize> = None;

                    {
                        // Scope to ensure proj_read is dropped before service calls
                        let proj_read = match project.read() {
                            Ok(p) => p,
                            Err(_) => return,
                        };

                        // Get composition info
                        if let Some(comp) = proj_read.compositions.iter().find(|c| c.id == comp_id)
                        {
                            root_track_ids.push(comp.root_track_id);
                            comp_width = comp.width;
                            comp_height = comp.height;
                        }

                        // Flatten to find corresponding track - extract only IDs
                        let display_rows = super::super::utils::flatten::flatten_tracks_to_rows(
                            &proj_read,
                            &root_track_ids,
                            &editor_context.timeline.expanded_tracks,
                        );

                        if visible_row_index < display_rows.len() {
                            let row = &display_rows[visible_row_index];
                            match row {
                                super::super::utils::flatten::DisplayRow::TrackHeader {
                                    track,
                                    ..
                                } => {
                                    target_track_id_opt = Some(track.id);
                                }
                                super::super::utils::flatten::DisplayRow::ClipRow {
                                    parent_track,
                                    ..
                                } => {
                                    target_track_id_opt = Some(parent_track.id);
                                }
                            }
                        }

                        // Build the clip based on dragged item
                        match dragged_item {
                            DraggedItem::Asset(asset_id) => {
                                if let Some(asset) =
                                    proj_read.assets.iter().find(|a| a.id == *asset_id)
                                {
                                    if visible_row_index < display_rows.len() {
                                        let row = &display_rows[visible_row_index];
                                        match row {
                                            super::super::utils::flatten::DisplayRow::TrackHeader {
                                                track,
                                                ..
                                            } => {
                                                target_track_id_opt = Some(track.id);
                                            }
                                            super::super::utils::flatten::DisplayRow::ClipRow {
                                                parent_track,
                                                ..
                                            } => {
                                                target_track_id_opt = Some(parent_track.id);
                                            }
                                        }

                                        // Calculate Index
                                        if let Some(tid) = target_track_id_opt {
                                            if let Some(header_idx) = display_rows.iter().position(|r| r.track_id() == tid && matches!(r, super::super::utils::flatten::DisplayRow::TrackHeader{..})) {
                                                 let raw_index = visible_row_index as isize - header_idx as isize - 1;
                                                 if let Some(track) = proj_read.get_track(tid) {
                                                     let clip_count = track.child_ids.iter().filter(|id| matches!(proj_read.get_node(**id), Some(library::model::project::Node::Clip(_)))).count();
                                                     let max_index = clip_count as isize;
                                                     let inverted = max_index - raw_index;
                                                     calculated_insert_index = Some(inverted.clamp(0, max_index) as usize);
                                                 }
                                            }
                                        }
                                    }
                                    let duration_sec = asset.duration.unwrap_or(5.0);
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
                                                0,
                                                duration_frames,
                                                asset.fps.unwrap_or(30.0),
                                                comp_width as u32,
                                                comp_height as u32,
                                            );
                                            if let (Some(w), Some(h)) = (asset.width, asset.height)
                                            {
                                                video_clip.properties.set(
                                                    "anchor".to_string(),
                                                    library::model::project::property::Property::constant(
                                                        library::model::project::property::PropertyValue::Vec2(
                                                            library::model::project::property::Vec2 {
                                                                x: ordered_float::OrderedFloat(w as f64 / 2.0),
                                                                y: ordered_float::OrderedFloat(h as f64 / 2.0),
                                                            },
                                                        ),
                                                    ),
                                                );
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
                                            image_clip.source_begin_frame = 0;
                                            if let (Some(w), Some(h)) = (asset.width, asset.height)
                                            {
                                                image_clip.properties.set(
                                                    "anchor".to_string(),
                                                    library::model::project::property::Property::constant(
                                                        library::model::project::property::PropertyValue::Vec2(
                                                            library::model::project::property::Vec2 {
                                                                x: ordered_float::OrderedFloat(w as f64 / 2.0),
                                                                y: ordered_float::OrderedFloat(h as f64 / 2.0),
                                                            },
                                                        ),
                                                    ),
                                                );
                                            }
                                            Some(image_clip)
                                        }
                                        AssetKind::Audio => Some(TrackClip::create_audio(
                                            Some(asset.id),
                                            &asset.path,
                                            drop_in_frame,
                                            drop_out,
                                            0,
                                            duration_frames,
                                            composition_fps,
                                        )),
                                        _ => None,
                                    };
                                }
                            }
                            DraggedItem::Composition(target_comp_id) => {
                                let mut duration_sec = 10.0;
                                let mut target_fps = 30.0;
                                if let Some(c) = proj_read
                                    .compositions
                                    .iter()
                                    .find(|c| c.id == *target_comp_id)
                                {
                                    duration_sec = c.duration;
                                    target_fps = c.fps;
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
                                    target_fps,
                                    library::model::project::property::PropertyMap::new(),
                                    Vec::new(),
                                    Vec::new(),
                                    Vec::new(),
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
                    } // proj_read is now dropped

                    // ===== PHASE 2: Call service methods (needs write lock) =====
                    if let (Some(new_clip), Some(_drop_out)) = (new_clip_opt, drop_out_frame_opt) {
                        let mut success = false;

                        if let Some(parent_track_id) = target_track_id_opt {
                            if let Err(e) = project_service.add_clip_to_track(
                                comp_id,
                                parent_track_id,
                                new_clip,
                                drop_in_frame,
                                drop_in_frame + (drop_out_frame_opt.unwrap() - drop_in_frame),
                                calculated_insert_index,
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
                            if let Ok(new_track_id) =
                                project_service.add_track(comp_id, "New Track")
                            {
                                if let Err(e) = project_service.add_clip_to_track(
                                    comp_id,
                                    new_track_id,
                                    new_clip,
                                    drop_in_frame,
                                    drop_in_frame + (drop_out_frame_opt.unwrap() - drop_in_frame),
                                    calculated_insert_index,
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
