use egui::Ui;
use library::model::project::project::Project;
use library::model::project::TrackClip;
use library::service::project_service::ProjectService;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::{action::HistoryManager, state::context::EditorContext};

mod background;
mod clips;
mod interactions;

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
) -> (egui::Rect, egui::Response) {
    let (content_rect_for_clip_area, response) =
        ui_content.allocate_at_least(ui_content.available_size(), egui::Sense::click_and_drag());

    let is_dragging_item = editor_context.interaction.dragged_item.is_some();

    // --- Data collection for entities ---
    let mut all_clips: Vec<(Uuid, TrackClip)> = Vec::new(); // all_entities -> all_clips
    let mut current_tracks: Vec<library::model::project::Track> = Vec::new();
    let selected_composition_id = editor_context.selection.composition_id;
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

    // Get Composition Duration
    let mut current_comp_duration = 300.0; // Default fallback
    if let Some(comp_id) = selected_composition_id {
        if let Ok(proj_read) = project.read() {
            if let Some(comp) = proj_read.compositions.iter().find(|c| c.id == comp_id) {
                current_comp_duration = comp.duration;
            }
        }
    }

    background::draw_track_backgrounds(
        &painter,
        content_rect_for_clip_area,
        num_tracks,
        row_height,
        track_spacing,
        editor_context.timeline.scroll_offset.y,
        editor_context.timeline.scroll_offset.x,
        pixels_per_unit,
        current_comp_duration,
    );

    // --- Main Interaction Block (for overall clip area, e.g., scroll, zoom, asset drop) ---
    interactions::handle_area_interaction(
        ui_content,
        &response,
        content_rect_for_clip_area,
        editor_context,
        project,
        project_service,
        history_manager,
        pixels_per_unit,
        composition_fps,
        num_tracks,
        row_height,
        track_spacing,
    );

    // --- Loop for drawing and interacting with entities ---
    let clicked_on_entity = clips::draw_clips(
        ui_content,
        content_rect_for_clip_area,
        editor_context,
        project_service,
        history_manager,
        &current_tracks,
        &all_clips,
        project,
        pixels_per_unit,
        row_height,
        track_spacing,
        composition_fps,
    );

    // Final selection clearing logic
    if !editor_context.interaction.is_resizing_entity
        && response.clicked()
        && !clicked_on_entity
        && !is_dragging_item
    {
        editor_context.selection.entity_id = None;
    }

    (content_rect_for_clip_area, response)
}
