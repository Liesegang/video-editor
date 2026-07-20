use egui::Ui;
use library::model::asset::AssetKind;
use library::model::project::Project;
use library::ClipBundle;
use library::EditorService as ProjectService;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::utils::lock::read_or_recover;
use crate::{
    action::HistoryManager, state::context::EditorContext, state::context_types::DragStateItem,
};

use super::interactions::InteractionGeometry;

pub(super) fn handle_drag_and_drop(
    ui: &mut Ui,
    response: &egui::Response,
    editor_context: &mut EditorContext,
    project: &Arc<RwLock<Project>>,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    geometry: InteractionGeometry,
) {
    if ui.input(|i| i.pointer.any_released()) {
        if let Some(dragged_item) = &editor_context.interaction.dragged_item {
            if let Some(mouse_pos) = response.hover_pos() {
                let drop_time_f64 = ((mouse_pos.x - geometry.content_rect.min.x
                    + editor_context.timeline.scroll_offset.x)
                    / geometry.pixels_per_unit)
                    .max(0.0) as f64;

                let visible_row_index = ((mouse_pos.y - geometry.content_rect.min.y
                    + editor_context.timeline.scroll_offset.y)
                    / (geometry.row_height + geometry.track_spacing))
                    .floor() as usize;

                if let Some(comp_id) = editor_context.selection.composition_id {
                    // ===== PHASE 1: Read all needed data, extract owned values =====
                    let mut track_ids: Vec<Uuid> = Vec::new();
                    let mut comp_width = 1920u32;
                    let mut comp_height = 1080u32;
                    let mut target_track_id_opt: Option<Uuid> = None;
                    let mut new_bundle_opt: Option<ClipBundle> = None;
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
                            track_ids = comp.track_ids.clone();
                            comp_width = comp.width as u32;
                            comp_height = comp.height as u32;
                        }

                        // Flatten to find corresponding track - extract only IDs
                        let display_rows = super::super::utils::flatten::flatten_tracks_to_rows(
                            &proj_read,
                            &track_ids,
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

                        // Build the layer based on dragged item
                        match dragged_item {
                            DragStateItem::Asset { asset_id, .. } => {
                                if let Some(asset) =
                                    proj_read.assets.iter().find(|a| a.id == *asset_id)
                                {
                                    // Recalculate target track if needed (same logic as above?)
                                    // It seems redundant but kept for safety if logic diverges

                                    // Calculate Index
                                    if let Some(tid) = target_track_id_opt {
                                        if let Some(header_idx) = display_rows.iter().position(|r| r.track_id() == tid && matches!(r, super::super::utils::flatten::DisplayRow::TrackHeader{..})) {
                                           let raw_index = visible_row_index as isize - header_idx as isize - 1;
                                           if let Some(track) = proj_read.get_track(tid) {
                                               let clip_count = track.clip_ids.len();
                                               let max_index = clip_count as isize;
                                               let inverted = max_index - raw_index;
                                               calculated_insert_index = Some(inverted.clamp(0, max_index) as usize);
                                           }
                                       }
                                    }

                                    let duration_sec = asset.duration.unwrap_or(5.0);

                                    new_bundle_opt = match asset.kind {
                                        AssetKind::Video => {
                                            let video_clip_res = project_service.create_video_clip(
                                                asset.id,
                                                &asset.path,
                                                drop_time_f64,
                                                duration_sec,
                                                0.0, // source start
                                                1.0, // speed
                                                comp_width,
                                                comp_height,
                                            );
                                            video_clip_res.ok().map(|mut video_clip| {
                                                 if let (Some(w), Some(h)) = (asset.width, asset.height)
                                                {
                                                    if let Some(node) = video_clip.primary_node_mut() {
                                                        if let Err(error) = node.set_property(
                                                            "anchor".to_string(),
                                                            library::model::property::Property::constant(
                                                                library::model::property::PropertyValue::Vec2(
                                                                    library::model::property::Vec2 {
                                                                        x: ordered_float::OrderedFloat(w as f64 / 2.0),
                                                                        y: ordered_float::OrderedFloat(h as f64 / 2.0),
                                                                    },
                                                                ),
                                                            ),
                                                        ) {
                                                            log::error!(
                                                                "Video factory omitted anchor property: {error}"
                                                            );
                                                        }
                                                    }
                                                }
                                                video_clip
                                            })
                                        }
                                        AssetKind::Image => {
                                            let image_clip_res = project_service.create_image_clip(
                                                asset.id,
                                                &asset.path,
                                                drop_time_f64,
                                                duration_sec,
                                                comp_width,
                                                comp_height,
                                                0.0, // _fps
                                            );
                                            image_clip_res.ok().map(|mut image_clip| {
                                                // Source start irrelevant for Image? Or kept at 0
                                                if let (Some(w), Some(h)) = (asset.width, asset.height)
                                                {
                                                    if let Some(node) = image_clip.primary_node_mut() {
                                                        if let Err(error) = node.set_property(
                                                            "anchor".to_string(),
                                                            library::model::property::Property::constant(
                                                                library::model::property::PropertyValue::Vec2(
                                                                    library::model::property::Vec2 {
                                                                        x: ordered_float::OrderedFloat(w as f64 / 2.0),
                                                                        y: ordered_float::OrderedFloat(h as f64 / 2.0),
                                                                    },
                                                                ),
                                                            ),
                                                        ) {
                                                            log::error!(
                                                                "Image factory omitted anchor property: {error}"
                                                            );
                                                        }
                                                    }
                                                }
                                                image_clip
                                            })
                                        }
                                        AssetKind::Audio => project_service
                                            .create_audio_clip(
                                                asset.id,
                                                &asset.path,
                                                drop_time_f64,
                                                duration_sec,
                                                0.0,
                                                1.0,
                                            )
                                            .ok(),
                                        _ => None,
                                    };
                                }
                            }
                            DragStateItem::Composition {
                                id: target_comp_id, ..
                            } => {
                                let mut duration_sec = 10.0;
                                if let Some(c) = proj_read
                                    .compositions
                                    .iter()
                                    .find(|c| c.id == *target_comp_id)
                                {
                                    duration_sec = c.duration;
                                }

                                new_bundle_opt = project_service
                                    .create_reference_clip(
                                        *target_comp_id,
                                        drop_time_f64,
                                        duration_sec,
                                    )
                                    .ok();
                            }
                        }
                    } // proj_read is now dropped

                    // ===== PHASE 2: Call service methods (needs write lock) =====
                    if let Some(new_bundle) = new_bundle_opt {
                        let mut success = false;

                        if let Some(parent_track_id) = target_track_id_opt {
                            if let Err(e) = project_service.add_clip_to_track(
                                comp_id,
                                parent_track_id,
                                new_bundle,
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
                                    new_bundle,
                                    calculated_insert_index,
                                ) {
                                    log::error!("Failed to add clip to new track: {:?}", e);
                                    if let Err(cleanup_error) =
                                        project_service.remove_track(comp_id, new_track_id)
                                    {
                                        log::error!(
                                            "Failed to remove empty track after clip insertion failed: {}",
                                            cleanup_error
                                        );
                                    }
                                } else {
                                    editor_context.timeline.expanded_tracks.insert(new_track_id);
                                    success = true;
                                }
                            }
                        }

                        if success {
                            let project = project_service.get_project();
                            let current_state = read_or_recover(project.as_ref()).clone();
                            history_manager.push_project_state(current_state);
                        }
                    }
                }
            }
        }
    }
}
