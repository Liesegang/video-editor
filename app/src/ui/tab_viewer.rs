use egui::Ui;
use egui_dock::{DockState, TabViewer};
use egui_phosphor::regular as icons;
use library::model::project::Project;
use std::sync::{Arc, RwLock};

use crate::command::{CommandRegistry, CommandScope};
use crate::ui::dialogs::composition_dialog::CompositionDialog;
use crate::{
    action::{activate_composition_with_history, HistoryManager},
    model::ui_types::Tab,
    state::context::EditorContext,
    ui::panels::{assets, inspector, node_editor, preview, timeline},
};
use library::EditorService;
use library::RenderServer;

pub struct AppTabViewer<'a> {
    editor_context: &'a mut EditorContext,
    history_manager: &'a mut HistoryManager,
    project_service: &'a mut EditorService,
    project: &'a Arc<RwLock<Project>>,
    composition_dialog: &'a mut CompositionDialog,
    render_server: &'a RenderServer,
    command_registry: &'a CommandRegistry,
    node_editor_rendered_this_frame: bool,
}

impl<'a> AppTabViewer<'a> {
    pub fn new(
        editor_context: &'a mut EditorContext,
        history_manager: &'a mut HistoryManager,
        project_service: &'a mut EditorService,
        project: &'a Arc<RwLock<Project>>,
        composition_dialog: &'a mut CompositionDialog,
        render_server: &'a RenderServer,
        command_registry: &'a CommandRegistry,
    ) -> Self {
        Self {
            editor_context,
            history_manager,
            project_service,
            project,
            composition_dialog,
            render_server,
            command_registry,
            node_editor_rendered_this_frame: false,
        }
    }

    pub fn finish_frame(&mut self) {
        if !self.node_editor_rendered_this_frame {
            node_editor::flush_pending_continuous_edit(
                self.project,
                self.history_manager,
                &mut self.editor_context.node_editor_state,
            );
            self.editor_context.node_editor_state.panel_rect = None;
            self.editor_context.node_editor_state.pending_layout_command = None;
        }
    }
}

impl<'a> TabViewer for AppTabViewer<'a> {
    type Tab = Tab;

    fn ui(&mut self, ui: &mut Ui, tab: &mut Self::Tab) {
        match tab {
            Tab::Preview => preview::preview_panel(
                ui,
                self.editor_context,
                self.history_manager,
                self.project_service,
                self.project,
                self.render_server,
                self.command_registry,
            ),
            Tab::Timeline => timeline::timeline_panel(
                ui,
                self.editor_context,
                self.history_manager,
                self.project_service,
                self.project,
                self.command_registry,
            ),
            Tab::Inspector => inspector::inspector_panel(
                ui,
                self.editor_context,
                self.history_manager,
                self.project_service,
                self.project,
            ),
            Tab::Assets => assets::assets_panel(
                ui,
                self.editor_context,
                self.history_manager,
                self.project_service,
                self.project,
                self.composition_dialog,
            ),
            Tab::GraphEditor => {
                crate::ui::panels::graph_editor::graph_editor_panel(
                    ui,
                    self.editor_context,
                    self.history_manager,
                    self.project_service,
                    self.project,
                    self.command_registry,
                );
            }
            Tab::NodeEditor => {
                self.node_editor_rendered_this_frame = true;
                self.editor_context.node_editor_state.panel_rect =
                    Some(ui.max_rect().intersect(ui.clip_rect()));
                node_editor::node_editor_panel(
                    ui,
                    self.project,
                    self.project_service,
                    self.history_manager,
                    self.editor_context,
                    self.command_registry,
                );

                // Handle Navigation Requests
                if let Some(target_comp_id) =
                    self.editor_context.node_editor_state.pending_navigation
                {
                    activate_composition_with_history(
                        self.editor_context,
                        Some(target_comp_id),
                        self.history_manager,
                        self.project,
                    );
                    // Also switch tab to Timeline? Or stay in Node Editor?
                    // User probably wants to see the graph of the new container, so stay in Node Editor.
                    // But if it's a "Composite", maybe they want Timeline?
                    // For "Container Node" editing, Node Editor is primary.

                    self.editor_context.node_editor_state.pending_navigation = None;
                }
            }
        }
    }

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        match tab {
            Tab::Preview => format!("{} {}", icons::MONITOR_PLAY, "Preview").into(),
            Tab::Timeline => format!("{} {}", icons::FILM_STRIP, "Timeline").into(),
            Tab::Inspector => format!("{} {}", icons::WRENCH, "Inspector").into(),
            Tab::Assets => format!("{} {}", icons::FOLDER, "Assets").into(),
            Tab::GraphEditor => format!("{} {}", icons::CHART_LINE, "Graph Editor").into(),
            Tab::NodeEditor => format!("{} {}", icons::SHARE_NETWORK, "Node Editor").into(),
        }
    }

    fn on_tab_button(&mut self, tab: &mut Self::Tab, response: &egui::Response) {
        let slug = tab.name().to_ascii_lowercase().replace(' ', "_");
        crate::qa::register_component_with_metadata(
            format!("dock.tab:{slug}"),
            "dock_tab",
            response.rect,
            response.enabled(),
            Some(serde_json::json!({
                "label": tab.name(),
                "hovered": response.hovered(),
            })),
        );
    }
}

pub fn active_command_scope(
    dock_state: &DockState<Tab>,
    pointer_hover_pos: Option<egui::Pos2>,
    node_editor_rect: Option<egui::Rect>,
    has_active_composition: bool,
) -> CommandScope {
    if !has_active_composition {
        return CommandScope::Global;
    }
    if let Some(pointer) = pointer_hover_pos {
        return if node_editor_rect.is_some_and(|rect| rect.contains(pointer)) {
            CommandScope::NodeEditor
        } else {
            CommandScope::Global
        };
    }
    let Some((surface, node)) = dock_state.focused_leaf() else {
        return CommandScope::Global;
    };
    let Some(leaf) = dock_state[surface][node].get_leaf() else {
        return CommandScope::Global;
    };
    match leaf.tabs.get(leaf.active.0) {
        Some(Tab::NodeEditor) => CommandScope::NodeEditor,
        _ => CommandScope::Global,
    }
}

pub fn create_initial_dock_state() -> DockState<Tab> {
    let mut dock_state = DockState::new(vec![Tab::Preview]);
    let surface = dock_state.main_surface_mut();

    // 1. Split off the timeline at the bottom (30% of height)
    let [main_area, _] = surface.split_below(
        egui_dock::NodeIndex::root(),
        0.7,
        vec![Tab::Timeline, Tab::GraphEditor, Tab::NodeEditor],
    );

    // 2. Split off the inspector on the right (20% of width)
    // The remaining area is 80% wide, so we split at 0.8
    let [main_area, _] = surface.split_right(main_area, 0.8, vec![Tab::Inspector]);

    // 3. Split off the assets on the left (20% of original width)
    // The remaining area is 80% wide. 0.2 / 0.8 = 0.25
    surface.split_left(main_area, 0.25, vec![Tab::Assets]);

    dock_state
}

#[cfg(test)]
mod tests {
    use super::{active_command_scope, create_initial_dock_state};
    use crate::command::CommandScope;
    use crate::model::ui_types::Tab;

    #[test]
    fn node_editor_commands_require_the_focused_node_editor_leaf() {
        let mut dock = create_initial_dock_state();
        assert_eq!(
            active_command_scope(&dock, None, None, true),
            CommandScope::Global
        );

        let (surface, node, tab) = dock.find_tab(&Tab::NodeEditor).expect("node editor tab");
        dock.set_active_tab((surface, node, tab));
        dock.set_focused_node_and_surface((surface, node));
        assert_eq!(
            active_command_scope(&dock, None, None, true),
            CommandScope::NodeEditor
        );

        let (surface, node, tab) = dock.find_tab(&Tab::Preview).expect("preview tab");
        dock.set_active_tab((surface, node, tab));
        dock.set_focused_node_and_surface((surface, node));
        assert_eq!(
            active_command_scope(&dock, None, None, true),
            CommandScope::Global
        );
    }

    #[test]
    fn pointer_hover_scope_overrides_stale_dock_focus_in_both_directions() {
        let mut dock = create_initial_dock_state();
        let node_rect = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(100.0, 100.0));
        let (node_surface, node_leaf, node_tab) =
            dock.find_tab(&Tab::NodeEditor).expect("node editor tab");
        dock.set_active_tab((node_surface, node_leaf, node_tab));

        dock.set_focused_node_and_surface((node_surface, node_leaf));
        assert_eq!(
            active_command_scope(&dock, Some(egui::pos2(150.0, 50.0)), Some(node_rect), true,),
            CommandScope::Global
        );

        let (preview_surface, preview_leaf, _) = dock.find_tab(&Tab::Preview).expect("preview tab");
        dock.set_focused_node_and_surface((preview_surface, preview_leaf));
        assert_eq!(
            active_command_scope(&dock, Some(egui::pos2(50.0, 50.0)), Some(node_rect), true,),
            CommandScope::NodeEditor
        );
        assert_eq!(
            active_command_scope(&dock, Some(egui::pos2(50.0, 50.0)), Some(node_rect), false,),
            CommandScope::Global
        );
    }
}
