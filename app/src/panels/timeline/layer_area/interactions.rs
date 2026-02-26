use egui::Ui;
use library::project::project::Project;
use library::EditorService as ProjectService;
use std::sync::{Arc, RwLock};

use crate::{command::history::HistoryManager, context::context::EditorContext};

use super::super::geometry::TimelineGeometry;

pub(super) fn handle_drag_drop_and_context_menu(
    ui: &mut Ui,
    response: &egui::Response,
    content_rect: egui::Rect,
    editor_context: &mut EditorContext,
    project: &Arc<RwLock<Project>>,
    project_service: &mut ProjectService,
    history_manager: &mut HistoryManager,
    geo: &TimelineGeometry,
    num_tracks: usize,
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
        geo,
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
        geo,
        num_tracks,
    );
}
