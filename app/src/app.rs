use eframe::egui::{self, Visuals};
use egui_dock::{DockArea, DockState, Style};
use library::model::project::project::{Composition, Project};
use library::service::project_service::ProjectService;
use std::sync::{Arc, RwLock};

use crate::action::{
    handler::{handle_command, ActionContext},
    HistoryManager,
};
use crate::command::{CommandId, CommandRegistry};
use crate::config;
use crate::model::ui_types::Tab;
use crate::shortcut::ShortcutManager;
use crate::state::context::EditorContext;
use crate::ui::dialogs::composition_dialog::CompositionDialog;
use crate::ui::dialogs::settings_dialog::SettingsDialog;
use crate::ui::tab_viewer::{create_initial_dock_state, AppTabViewer};
use crate::utils;
use library::RenderServer;

pub struct MyApp {
    pub editor_context: EditorContext,
    pub dock_state: DockState<Tab>,
    pub project_service: ProjectService,
    pub project: Arc<RwLock<Project>>,
    pub history_manager: HistoryManager,
    shortcut_manager: ShortcutManager,
    command_registry: CommandRegistry,

    // Dialogs
    pub settings_dialog: SettingsDialog,
    pub composition_dialog: CompositionDialog,

    pub triggered_action: Option<CommandId>,
    pub render_server: Arc<RenderServer>,
}

impl MyApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Customize egui
        let mut visuals = Visuals::dark();
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(255, 120, 0);
        cc.egui_ctx.set_visuals(visuals);

        // Setup fonts
        utils::setup_fonts(&cc.egui_ctx);

        let shortcut_config = config::load_config();
        let command_registry = CommandRegistry::new(&shortcut_config);

        let default_project = Arc::new(RwLock::new(Project::new("Default Project")));
        // Add a default composition when the app starts
        let default_comp = Composition::new("Main Composition", 1920, 1080, 30.0, 60.0);
        let default_comp_id = default_comp.id;
        default_project
            .write()
            .unwrap()
            .add_composition(default_comp);

        let plugin_manager = library::create_plugin_manager();
        let cache_manager = Arc::new(library::cache::CacheManager::new());
        let project_service =
            ProjectService::new(Arc::clone(&default_project), plugin_manager.clone());

        let mut editor_context = EditorContext::new(default_comp_id); // Pass default_comp_id
        editor_context.selection.composition_id = Some(default_comp_id); // Select the default composition

        let entity_converter_registry = plugin_manager.get_entity_converter_registry();
        let render_server = Arc::new(RenderServer::new(
            plugin_manager,
            cache_manager,
            entity_converter_registry,
        ));

        let mut app = Self {
            editor_context,
            dock_state: create_initial_dock_state(),
            project_service,
            project: default_project,
            history_manager: HistoryManager::new(),
            shortcut_manager: ShortcutManager::new(),
            command_registry: command_registry.clone(),
            settings_dialog: SettingsDialog::new(command_registry),
            triggered_action: None,
            composition_dialog: CompositionDialog::new(),
            render_server,
        };
        if let Ok(proj_read) = app.project_service.get_project().read() {
            app.history_manager.push_project_state(proj_read.clone());
        }
        cc.egui_ctx.request_repaint(); // Request repaint after initial state setup
        app
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.triggered_action = None;
        let mut is_listening_for_shortcut = false;

        // --- Draw UI and Collect Inputs ---

        // 2. Menu Bar
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            let main_ui_enabled =
                !self.settings_dialog.is_open && !self.settings_dialog.show_close_warning && !self.editor_context.keyframe_dialog.is_open;
            // Disable menu bar if a modal is open
            ui.add_enabled_ui(main_ui_enabled, |ui| {
                crate::ui::menu::menu_bar(
                    ui,
                    &self.command_registry,
                    &mut self.dock_state,
                    &mut self.triggered_action,
                );
            });
        });

        // 3. Settings Window & Unsaved Changes Dialog
        if self.settings_dialog.show(ctx) {
            is_listening_for_shortcut = true;
        }

        if self.composition_dialog.is_open {
            self.composition_dialog.show(ctx);
        }

        if self.editor_context.keyframe_dialog.is_open {
            crate::ui::dialogs::keyframe_dialog::show_keyframe_dialog(
                ctx,
                &mut self.editor_context,
                &mut self.project_service,
                &self.project,
            );
        }

        // 6. Generic Error Modal
        if let Some(error_msg) = self.editor_context.interaction.active_modal_error.clone() {
            let mut open = true;
            egui::Window::new("⚠ Error")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label(&error_msg);
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui.button("OK").clicked() {
                            self.editor_context.interaction.active_modal_error = None;
                        }
                    });
                });
            if !open {
                // Window closed via X button
                self.editor_context.interaction.active_modal_error = None;
            }
        }

        // 1. Shortcuts (continued)
        // Only handle shortcuts if no modal window is open and not listening, to prevent conflicts
        let main_ui_enabled = !self.settings_dialog.is_open
            && !self.settings_dialog.show_close_warning
            && !self.composition_dialog.is_open
            && !self.editor_context.keyframe_dialog.is_open;
        if main_ui_enabled && !is_listening_for_shortcut {
            if let Some(action_id) = self
                .shortcut_manager
                .handle_shortcuts(ctx, &self.command_registry)
            {
                self.triggered_action = Some(action_id);
            }
        }

        // --- Deferred Action Execution ---
        if let Some(action) = self.triggered_action {
            let mut trigger_settings = false;
            let context = ActionContext {
                editor_context: &mut self.editor_context,
                project_service: &mut self.project_service,
                history_manager: &mut self.history_manager,
                dock_state: &mut self.dock_state,
            };

            handle_command(ctx, action, context, &mut trigger_settings);

            if trigger_settings {
                self.settings_dialog.open(&self.command_registry);
            }
            ctx.request_repaint();
        }

        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Ready");
                ui.separator();
                ui.label(format!(
                    "Time: {:.2}",
                    self.editor_context.timeline.current_time
                ));
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let main_ui_enabled =
                !self.settings_dialog.is_open && !self.settings_dialog.show_close_warning;
            ui.add_enabled_ui(main_ui_enabled, |ui| {
                let mut tab_viewer = AppTabViewer::new(
                    &mut self.editor_context,
                    &mut self.history_manager,
                    &mut self.project_service,
                    &self.project,
                    &mut self.composition_dialog,
                    &self.render_server,
                );
                DockArea::new(&mut self.dock_state)
                    .style(Style::from_egui(ui.style().as_ref()))
                    .show_leaf_collapse_buttons(false)
                    .show_inside(ui, &mut tab_viewer);
            });
        });

        if ctx.input(|i| i.pointer.any_released()) {
            self.editor_context.interaction.dragged_item = None;
        }

        if self.editor_context.timeline.is_playing {
            let fps = if let Ok(proj_read) = self.project.read() {
                self.editor_context
                    .get_current_composition(&proj_read)
                    .map(|c| c.fps)
                    .unwrap_or(30.0)
            } else {
                30.0
            };
            let frame_duration = 1.0 / fps as f32;

            self.editor_context.timeline.playback_accumulator += ctx.input(|i| i.stable_dt);

            // Limit the accumulator to prevent spiraling if lag occurs (e.g. max 10 frames catchup)
            if self.editor_context.timeline.playback_accumulator > frame_duration * 10.0 {
                self.editor_context.timeline.playback_accumulator = frame_duration;
            }

            while self.editor_context.timeline.playback_accumulator >= frame_duration {
                self.editor_context.timeline.current_time += frame_duration;
                self.editor_context.timeline.playback_accumulator -= frame_duration;
            }
            ctx.request_repaint();
        } else {
            // Reset accumulator when not playing to avoid jump on resume
            self.editor_context.timeline.playback_accumulator = 0.0;
        }
    }
}
