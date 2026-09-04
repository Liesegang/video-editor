//! Dock adapter for the Timeline-first editor.
//!
//! Panels share one immutable authoring snapshot for a frame and submit all
//! edits through `TimelineEditorService`. No legacy graph-backed Project is
//! constructed by this adapter.

use egui_dock::TabViewer;
use egui_phosphor::regular as icons;
use library::editor::TimelineEditorService;
use library::model::authoring::AuthoringProject;
use library::plugin::PluginManager;
use library::RenderServer;

use crate::model::ui_types::Tab;
use crate::state::authoring::AuthoringUiState;
use crate::ui::timeline_first::{
    assets_panel, curve_panel, inspector_panel, preview_panel, timeline_panel,
    AuthoringPreviewRuntime,
};

pub struct AuthoringTabViewer<'a> {
    project: &'a AuthoringProject,
    state: &'a mut AuthoringUiState,
    service: &'a TimelineEditorService,
    plugins: &'a PluginManager,
    render_server: &'a RenderServer,
    preview_runtime: &'a mut AuthoringPreviewRuntime,
}

impl<'a> AuthoringTabViewer<'a> {
    pub fn new(
        project: &'a AuthoringProject,
        state: &'a mut AuthoringUiState,
        service: &'a TimelineEditorService,
        plugins: &'a PluginManager,
        render_server: &'a RenderServer,
        preview_runtime: &'a mut AuthoringPreviewRuntime,
    ) -> Self {
        Self {
            project,
            state,
            service,
            plugins,
            render_server,
            preview_runtime,
        }
    }
}

impl TabViewer for AuthoringTabViewer<'_> {
    type Tab = Tab;

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab {
            Tab::Preview => preview_panel(
                ui,
                self.state,
                self.service,
                self.render_server,
                self.preview_runtime,
            ),
            Tab::Timeline => timeline_panel(ui, self.project, self.state, self.service),
            Tab::Inspector => {
                inspector_panel(ui, self.project, self.state, self.service, self.plugins)
            }
            Tab::Assets => assets_panel(ui, self.project, self.state, self.service, self.plugins),
            Tab::GraphEditor => curve_panel(ui, self.project, self.state, self.service),
            Tab::NodeEditor => {
                self.state.node_editor.panel_rect = Some(ui.max_rect().intersect(ui.clip_rect()));
                crate::ui::module_node_editor::module_node_editor_panel(
                    ui,
                    self.project,
                    self.state,
                    self.service,
                    self.plugins,
                );
            }
        }
    }

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        let (icon, title) = match tab {
            Tab::Preview => (icons::MONITOR_PLAY, "Preview"),
            Tab::Timeline => (icons::FILM_STRIP, "Timeline"),
            Tab::Inspector => (icons::SLIDERS_HORIZONTAL, "Inspector"),
            Tab::Assets => (icons::FOLDER, "Assets"),
            Tab::GraphEditor => (icons::CHART_LINE, "Curve Editor"),
            Tab::NodeEditor => (icons::SHARE_NETWORK, "Node Editor"),
        };
        format!("{icon} {title}").into()
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
