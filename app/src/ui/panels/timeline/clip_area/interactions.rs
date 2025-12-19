use egui::Ui;
use library::model::project::project::Project;
use library::EditorService as ProjectService;
use std::sync::{Arc, RwLock};

use crate::{action::HistoryManager, state::context::EditorContext};

#[allow(clippy::too_many_arguments)]
pub fn handle_drag_drop_and_context_menu(
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
    // 1. Drag and Drop
    super::drag_and_drop::handle_drag_and_drop(
        ui,
        response,
        content_rect,
        editor_context,
        project,
        project_service,
        history_manager,
        pixels_per_unit,
        composition_fps,
        row_height,
        track_spacing,
    );

    // 2. Context Menu
    super::context_menu::handle_context_menu(
        ui,
        response,
        content_rect,
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
}
