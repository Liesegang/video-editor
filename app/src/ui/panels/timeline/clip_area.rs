use egui::{epaint::StrokeKind, Ui};
use library::model::project::project::Project;
use library::model::project::TrackEntity;
use library::service::project_service::ProjectService;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::{
    action::HistoryManager, model::assets::AssetKind, model::ui_types::GuiClip,
    state::context::EditorContext,
};

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

    let is_dragging_asset = editor_context.dragged_asset.is_some();
    let mut clicked_on_entity = false;

    // --- Data collection for entities ---
    let mut all_entities: Vec<(Uuid, TrackEntity)> = Vec::new();
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
        for entity in &track.entities {
            all_entities.push((track.id, entity.clone()));
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
                        .floor() as usize;

                    if let Some(comp_id) = editor_context.selected_composition_id {
                        let mut current_tracks_for_drop: Vec<library::model::project::Track> =
                            Vec::new();
                        if let Ok(proj_read) = project.read() {
                            if let Some(comp) =
                                proj_read.compositions.iter().find(|c| c.id == comp_id)
                            {
                                current_tracks_for_drop = comp.tracks.clone();
                            }
                        }

                        if let Some(track) = current_tracks_for_drop.get(drop_track_index) {
                            if let Some(asset) = editor_context.assets.get(asset_index) {
                                // Handle dropping a Composition asset
                                if let AssetKind::Composition(_nested_comp_id) = asset.kind {
                                    let prev_project_state =
                                        project_service.get_project().read().unwrap().clone();
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
                                        history_manager.push_project_state(prev_project_state);
                                    }
                                } else {
                                    let prev_project_state =
                                        project_service.get_project().read().unwrap().clone();
                                    if let Err(e) = project_service.add_entity_to_track(
                                        comp_id,
                                        track.id,
                                        &asset.name, // Use asset name as entity type for now
                                        drop_time as f64,
                                        (drop_time + asset.duration) as f64,
                                    ) {
                                        eprintln!("Failed to add entity to track: {:?}", e);
                                    } else {
                                        history_manager.push_project_state(prev_project_state);
                                    }
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

        for (entity_track_id, entity) in all_entities
            .iter()
            .filter(|(t_id, _)| *t_id == track_in_all_entities.id)
        {
            let asset_index = 0; // Temporary: should derive from entity properties
            let asset = editor_context.assets.get(asset_index);

            if let Some(a) = asset {
                let gc = GuiClip {
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

                let initial_x = content_rect_for_clip_area.min.x + gc.start_time * pixels_per_unit
                    - editor_context.timeline_scroll_offset.x;
                let initial_y = content_rect_for_clip_area.min.y
                    + editor_context.timeline_scroll_offset.y
                    + clip_track_index * (row_height + track_spacing);
                let initial_clip_rect = egui::Rect::from_min_size(
                    egui::pos2(initial_x, initial_y),
                    egui::vec2(gc.duration * pixels_per_unit, row_height),
                );

                // --- Interaction for clips ---
                // Define clip_resp using the initial_clip_rect for hit detection
                let clip_resp = ui_content.interact(
                    initial_clip_rect,
                    egui::Id::new(gc.id),
                    egui::Sense::click_and_drag(),
                );

                // Calculate display position (potentially adjusted for drag preview)
                let mut display_x = initial_x;
                let mut display_y = initial_y;

                // Adjust position for dragged entity preview
                if editor_context.selected_entity_id == Some(gc.id) && clip_resp.dragged() {
                    // Adjust X position based on current drag delta
                    display_x += clip_resp.drag_delta().x;

                    // Adjust Y position based on hovered track
                    if let Some(hovered_track_id) = editor_context.dragged_entity_hovered_track_id {
                        if let Some(hovered_track_index) = current_tracks.iter().position(|t| t.id == hovered_track_id) {
                            display_y = content_rect_for_clip_area.min.y
                                + editor_context.timeline_scroll_offset.y
                                + hovered_track_index as f32 * (row_height + track_spacing);
                        }
                    }
                }

                let drawing_clip_rect = egui::Rect::from_min_size(
                    egui::pos2(display_x, display_y),
                    egui::vec2(gc.duration * pixels_per_unit, row_height),
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
                if clip_resp.clicked() {
                    editor_context.selected_entity_id = Some(gc.id);
                    editor_context.selected_track_id = Some(gc.track_id);
                    clicked_on_entity = true;
                }

                if clip_resp.drag_started() {
                    editor_context.selected_entity_id = Some(gc.id);
                    editor_context.selected_track_id = Some(gc.track_id);
                    editor_context.dragged_entity_original_track_id = Some(gc.track_id); // Store original track
                    editor_context.dragged_entity_hovered_track_id = Some(gc.track_id); // Initially hovered is original track
                    if editor_context.last_project_state_before_drag.is_none() {
                        editor_context.last_project_state_before_drag =
                            Some(project_service.get_project().read().unwrap().clone());
                    }
                }
                if clip_resp.dragged() && editor_context.selected_entity_id == Some(gc.id) {
                    // Handle horizontal movement (time change)
                    let dt = clip_resp.drag_delta().x as f64 / pixels_per_unit as f64;
                    if let Some(comp_id) = editor_context.selected_composition_id {
                        if let Some(track_id) = editor_context.selected_track_id {
                            project_service
                                .with_track_mut(comp_id, track_id, |track_mut| {
                                    if let Some(entity_mut) =
                                        track_mut.entities.iter_mut().find(|e| e.id == gc.id)
                                    {
                                        entity_mut.start_time =
                                            (entity_mut.start_time + dt).max(0.0);
                                        entity_mut.end_time =
                                            (entity_mut.end_time + dt).max(entity_mut.start_time);
                                    }
                                })
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
                                if let Some(comp) = proj_read.compositions.iter().find(|c| c.id == comp_id) {
                                    if let Some(hovered_track) = comp.tracks.get(hovered_track_index) {
                                        if editor_context.dragged_entity_hovered_track_id != Some(hovered_track.id) {
                                            editor_context.dragged_entity_hovered_track_id = Some(hovered_track.id);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                                if clip_resp.drag_stopped() && editor_context.selected_entity_id == Some(gc.id) {
                                    let mut moved_track = false;
                                    if let (Some(original_track_id), Some(hovered_track_id), Some(comp_id)) = (
                                        editor_context.dragged_entity_original_track_id,
                                        editor_context.dragged_entity_hovered_track_id,
                                        editor_context.selected_composition_id,
                                    ) {
                                        if original_track_id != hovered_track_id {
                                            // Move entity to new track
                                            if let Err(e) = project_service.move_entity_to_track(
                                                comp_id,
                                                original_track_id,
                                                hovered_track_id,
                                                gc.id,
                                            ) {
                                                eprintln!("Failed to move entity to new track: {:?}", e);
                                            } else {
                                                editor_context.selected_track_id = Some(hovered_track_id); // Update selected track
                                                moved_track = true;
                                            }
                                        }
                                    }
                
                                    if let Some(initial_state) =
                                        editor_context.last_project_state_before_drag.take()
                                    {
                                        let current_state =
                                            project_service.get_project().read().unwrap().clone();
                                        if initial_state != current_state || moved_track { // Push history if time changed or track moved
                                            history_manager.push_project_state(current_state); // Changed to push current state, not initial state
                                        }
                                    }
                
                                    // Clear drag related states
                                    editor_context.dragged_entity_original_track_id = None;
                                    editor_context.dragged_entity_hovered_track_id = None;
                                }
            }
        }
    }
    // --- End Loop for drawing and interacting with entities ---

    // Final selection clearing logic
    if response.clicked() && !clicked_on_entity && !is_dragging_asset {
        editor_context.selected_entity_id = None;
    }
}
