use egui::Ui;
use library::model::project::Project;
use library::EditorService as ProjectService;
use std::sync::{Arc, RwLock};

use crate::{action::HistoryManager, state::context::EditorContext};

#[derive(Clone, Copy)]
pub(super) struct InteractionGeometry {
    pub content_rect: egui::Rect,
    pub pixels_per_unit: f32,
    pub num_tracks: usize,
    pub row_height: f32,
    pub track_spacing: f32,
}

pub(super) fn handle_drag_drop_and_context_menu(
    ui: &mut Ui,
    response: &egui::Response,
    editor_context: &mut EditorContext,
    project: &Arc<RwLock<Project>>,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    geometry: InteractionGeometry,
) {
    // 1. Drag and Drop
    super::drag_and_drop::handle_drag_and_drop(
        ui,
        response,
        editor_context,
        project,
        project_service,
        history_manager,
        geometry,
    );

    // 2. Context Menu
    super::context_menu::handle_context_menu(
        ui,
        response,
        editor_context,
        project,
        project_service,
        history_manager,
        geometry,
    );
}
