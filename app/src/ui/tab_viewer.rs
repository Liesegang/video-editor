use egui::Ui;
use egui_dock::{TabViewer, DockState};
use std::sync::{Arc, RwLock}; // Added
use library::model::project::project::Project; // Added

use crate::{
    action::HistoryManager,
    model::ui_types::Tab,
    state::context::EditorContext,
    ui::panels::{assets, inspector, preview, timeline},
};
use library::service::project_service::ProjectService;


pub struct AppTabViewer<'a> {
    editor_context: &'a mut EditorContext,
    history_manager: &'a mut HistoryManager,
    project_service: &'a mut ProjectService, // Changed to non-mut reference
    project: &'a Arc<RwLock<Project>>, // Added
    // Add other shared state here
}

impl<'a> AppTabViewer<'a> {
    pub fn new(
        editor_context: &'a mut EditorContext,
        history_manager: &'a mut HistoryManager,
        project_service: &'a mut ProjectService, // Changed to non-mut reference
        project: &'a Arc<RwLock<Project>>, // Added
    ) -> Self {
        Self {
            editor_context,
            history_manager,
            project_service,
            project,
        }
    }
}

impl<'a> TabViewer for AppTabViewer<'a> {
    type Tab = Tab;

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        match tab {
            Tab::Preview => preview::preview_panel(ui, self.editor_context, self.history_manager, self.project_service, self.project),
            Tab::Timeline => timeline::timeline_panel(ui, self.editor_context, self.history_manager, self.project_service, self.project),
            Tab::Inspector => inspector::inspector_panel(ui, self.editor_context, self.history_manager, self.project_service, self.project),
            Tab::Assets => assets::assets_panel(ui, self.editor_context, self.history_manager),
        }
    }

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        match tab {
            Tab::Preview => "Preview".into(),
            Tab::Timeline => "Timeline".into(),
            Tab::Inspector => "Inspector".into(),
            Tab::Assets => "Assets".into(),
        }
    }

    // Optional: add functions for on_close, etc.
}

pub fn create_initial_dock_state() -> DockState<Tab> {
    let mut dock_state = DockState::new(vec![Tab::Preview]);
    let surface = dock_state.main_surface_mut();

    // 1. Split off the timeline at the bottom (30% of height)
    let [main_area, _] = surface.split_below(egui_dock::NodeIndex::root(), 0.7, vec![Tab::Timeline]);

    // 2. Split off the inspector on the right (20% of width)
    // The remaining area is 80% wide, so we split at 0.8
    let [main_area, _] = surface.split_right(main_area, 0.8, vec![Tab::Inspector]);

    // 3. Split off the assets on the left (20% of original width)
    // The remaining area is 80% wide. 0.2 / 0.8 = 0.25
    surface.split_left(main_area, 0.25, vec![Tab::Assets]);

    dock_state
}