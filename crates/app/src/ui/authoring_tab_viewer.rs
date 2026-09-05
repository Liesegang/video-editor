//! Dock adapter for the authoring editor.
//!
//! Panels share one immutable authoring snapshot for a frame and submit all
//! edits through `TimelineEditorService`. No legacy graph-backed Project is
//! constructed by this adapter.

use std::sync::Arc;

use egui_dock::TabViewer;
use egui_phosphor::regular as icons;
use library::cache::CacheManager;
use library::editor::{AuthoringWaveformService, TimelineEditorService};
use library::model::authoring::AuthoringProject;
use library::plugin::PluginManager;
use library::RenderServer;

use crate::model::ui_types::Tab;
use crate::state::authoring::AuthoringUiState;
use crate::ui::media_preview::AuthoringMediaPreviewService;
use crate::ui::panels::assets::assets_panel;
use crate::ui::panels::curve_editor::curve_editor_panel;
use crate::ui::panels::inspector::inspector_panel;
use crate::ui::panels::preview::{preview_panel, AuthoringPreviewRuntime};
use crate::ui::panels::timeline::timeline_panel;

pub struct AuthoringTabViewer<'a> {
    project: &'a Arc<AuthoringProject>,
    state: &'a mut AuthoringUiState,
    service: &'a TimelineEditorService,
    plugins: &'a Arc<PluginManager>,
    cache: &'a Arc<CacheManager>,
    media_previews: &'a mut AuthoringMediaPreviewService,
    render_server: &'a RenderServer,
    preview_runtime: &'a mut AuthoringPreviewRuntime,
}

impl<'a> AuthoringTabViewer<'a> {
    #[allow(
        clippy::too_many_arguments,
        reason = "The dock TabViewer borrows the frame's authoritative editor services and mutable panel runtimes; bundling them would only move these borrow boundaries"
    )]
    pub fn new(
        project: &'a Arc<AuthoringProject>,
        state: &'a mut AuthoringUiState,
        service: &'a TimelineEditorService,
        plugins: &'a Arc<PluginManager>,
        cache: &'a Arc<CacheManager>,
        media_previews: &'a mut AuthoringMediaPreviewService,
        render_server: &'a RenderServer,
        preview_runtime: &'a mut AuthoringPreviewRuntime,
    ) -> Self {
        Self {
            project,
            state,
            service,
            plugins,
            cache,
            media_previews,
            render_server,
            preview_runtime,
        }
    }
}

impl TabViewer for AuthoringTabViewer<'_> {
    type Tab = Tab;

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        let waveform = AuthoringWaveformService::new(Arc::clone(self.cache));
        match tab {
            Tab::Preview => preview_panel(
                ui,
                self.state,
                self.service,
                self.plugins.as_ref(),
                self.render_server,
                self.preview_runtime,
            ),
            Tab::Timeline => timeline_panel(
                ui,
                self.project,
                self.state,
                self.service,
                self.plugins.as_ref(),
                &waveform,
                self.media_previews,
            ),
            Tab::Inspector => inspector_panel(
                ui,
                self.project,
                self.state,
                self.service,
                self.plugins.as_ref(),
                &waveform,
                self.media_previews,
            ),
            Tab::Assets => assets_panel(
                ui,
                self.project,
                self.state,
                self.service,
                self.plugins,
                &waveform,
                self.media_previews,
            ),
            Tab::CurveEditor => curve_editor_panel(ui, self.project, self.state, self.service),
            Tab::NodeEditor => {
                self.state.node_editor.panel_rect = Some(ui.max_rect().intersect(ui.clip_rect()));
                crate::ui::panels::node_editor::node_editor_panel(
                    ui,
                    self.project,
                    self.state,
                    self.service,
                    self.plugins.as_ref(),
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
            Tab::CurveEditor => (icons::CHART_LINE, "Curve Editor"),
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
