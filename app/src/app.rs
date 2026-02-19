use eframe::egui::{self, Visuals};
use egui_dock::{DockArea, DockState, Style};
use library::model::project::project::{Composition, Project};
use library::EditorService;
use log::warn;
#[allow(deprecated)]
use raw_window_handle::HasRawWindowHandle;
use std::fs;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::action::{
    handler::{handle_command, ActionContext},
    HistoryManager,
};
use crate::command::{CommandId, CommandRegistry};
use crate::config;
use crate::model::ui_types::Tab;
use crate::shortcut::ShortcutManager;
use crate::state::context::EditorContext;
use crate::ui::command_palette::CommandPalette;
use crate::ui::dialogs::composition_dialog::CompositionDialog;
use crate::ui::dialogs::export_dialog::ExportDialog;
use crate::ui::dialogs::settings_dialog::SettingsDialog;
use crate::ui::tab_viewer::{create_initial_dock_state, AppTabViewer};
use library::RenderServer;

pub struct RuViEApp {
    pub editor_context: EditorContext,
    pub dock_state: DockState<Tab>,
    pub project_service: EditorService,
    pub project: Arc<RwLock<Project>>,
    pub history_manager: HistoryManager,
    shortcut_manager: ShortcutManager,
    command_registry: CommandRegistry,
    pub app_config: config::AppConfig,

    // Dialogs
    pub settings_dialog: SettingsDialog,
    pub composition_dialog: CompositionDialog,
    pub export_dialog: ExportDialog,
    pub command_palette: CommandPalette,

    pub triggered_action: Option<CommandId>,
    pub render_server: Arc<RenderServer>,
}

impl RuViEApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let app_config = config::load_config();
        setup_theme(&cc.egui_ctx, &app_config);
        setup_fonts(&cc.egui_ctx);

        let command_registry = CommandRegistry::new(&app_config);
        let (default_project, default_comp_id) = create_default_project();
        let plugin_manager = setup_plugin_manager(&app_config);

        let cache_manager = Arc::new(library::cache::CacheManager::new());
        let project_service = EditorService::new(
            Arc::clone(&default_project),
            plugin_manager.clone(),
            cache_manager.clone(),
        );

        let mut editor_context = EditorContext::new(default_comp_id); // Pass default_comp_id
        editor_context.selection.composition_id = Some(default_comp_id); // Select the default composition
        editor_context.available_fonts = library::rendering::skia_utils::get_available_fonts();

        let render_server = Arc::new(RenderServer::new(
            plugin_manager.clone(),
            cache_manager.clone(),
        ));

        let mut app = Self {
            editor_context,
            dock_state: create_initial_dock_state(),
            project_service,
            project: default_project,
            history_manager: HistoryManager::new(),
            shortcut_manager: ShortcutManager::new(),
            command_registry: command_registry.clone(),
            app_config: app_config.clone(),
            settings_dialog: SettingsDialog::new(
                command_registry.clone(),
                app_config.clone(),
                plugin_manager.clone(),
            ),
            triggered_action: None,
            composition_dialog: CompositionDialog::new(),
            export_dialog: ExportDialog::new(plugin_manager.clone(), cache_manager.clone()),
            command_palette: CommandPalette::new(),
            render_server,
        };

        let initial_state = app.project_service.with_project(|p| p.clone());
        app.history_manager.push_project_state(initial_state);

        setup_gpu_sharing(&app.render_server, cc);

        cc.egui_ctx.request_repaint();
        app
    }
}

impl eframe::App for RuViEApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.triggered_action = None;
        let mut is_listening_for_shortcut = false;

        // --- Draw UI and Collect Inputs ---

        // 2. Menu Bar
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            let main_ui_enabled = !self.settings_dialog.is_open
                && !self.settings_dialog.show_close_warning
                && !self.editor_context.keyframe_dialog.is_open;
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
        // 3. Settings Window & Unsaved Changes Dialog
        let (is_listening, result) = self.settings_dialog.show(ctx);
        if is_listening {
            is_listening_for_shortcut = true;
        }
        if let Some(crate::ui::dialogs::settings_dialog::SettingsResult::Save) = result {
            self.command_registry = self.settings_dialog.command_registry.clone();
            self.app_config = self.settings_dialog.config.clone();

            // Apply theme when config changes
            crate::ui::theme::apply_theme(ctx, &self.app_config);

            // Apply new config
            config::save_config(&self.app_config);
        }

        if self.composition_dialog.is_open {
            self.composition_dialog.show(ctx);
        }

        if self.export_dialog.is_open {
            let active_comp_id = self.editor_context.selection.composition_id;
            self.export_dialog
                .show(ctx, &self.project, &self.project_service, active_comp_id);
        }

        if self.editor_context.keyframe_dialog.is_open {
            crate::ui::dialogs::keyframe_dialog::show_keyframe_dialog(
                ctx,
                &mut self.editor_context,
                &mut self.history_manager,
                &mut self.project_service,
                &self.project,
            );
        }

        // Palette
        if let Some(cmd_id) = self.command_palette.show(ctx, &self.command_registry) {
            self.triggered_action = Some(cmd_id);
        }

        // 6. Confirmation Dialog
        if let Some(dialog) = &mut self.editor_context.interaction.general.active_confirmation {
            if let Some(action) = dialog.show(ctx) {
                match action {
                    crate::ui::dialogs::confirmation::ConfirmationAction::DeleteAsset(id) => {
                        if let Err(e) = self.project_service.remove_asset_fully(id) {
                            log::error!("Failed to remove asset: {}", e);
                        } else {
                            // Push history
                            let current_state = self.project_service.with_project(|p| p.clone());
                            self.history_manager.push_project_state(current_state);
                        }
                    }
                    crate::ui::dialogs::confirmation::ConfirmationAction::DeleteComposition(id) => {
                        if let Err(e) = self.project_service.remove_composition_fully(id) {
                            log::error!("Failed to remove composition: {}", e);
                        } else {
                            // Clear selection if needed
                            if self.editor_context.selection.composition_id == Some(id) {
                                self.editor_context.selection.composition_id = None;
                                self.editor_context.selection.selected_entities.clear();
                            }
                            let current_state = self.project_service.with_project(|p| p.clone());
                            self.history_manager.push_project_state(current_state);
                        }
                    }
                    _ => {
                        log::warn!("Unhandled confirmation action: {:?}", action);
                    }
                }
                // Reset dialog logic is handled inside show() which sets is_open=false,
                // but we can set the Option to None if we want to clean up.
                // For now, keeping it is fine as is_open controls visibility.
            }
        }

        // 7. Generic Error Modal
        if let Some(error_msg) = self
            .editor_context
            .interaction
            .general
            .active_modal_error
            .clone()
        {
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
                            self.editor_context.interaction.general.active_modal_error = None;
                        }
                    });
                });
            if !open {
                // Window closed via X button
                self.editor_context.interaction.general.active_modal_error = None;
            }
        }

        // 1. Shortcuts (continued)
        // Only handle shortcuts if no modal window is open and not listening, to prevent conflicts
        let main_ui_enabled = !self.settings_dialog.is_open
            && !self.settings_dialog.show_close_warning
            && !self.composition_dialog.is_open
            && !self.export_dialog.is_open
            && !self.editor_context.keyframe_dialog.is_open
            && !self.command_palette.is_open;
        if main_ui_enabled && !is_listening_for_shortcut {
            if let Some(action_id) = self.shortcut_manager.handle_shortcuts(
                ctx,
                &self.command_registry,
                &mut self.editor_context,
            ) {
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

            if action == CommandId::Export {
                self.export_dialog.open();
            } else if action == CommandId::ShowCommandPalette {
                self.command_palette.toggle();
            }

            handle_command(ctx, action, context, &mut trigger_settings);

            if trigger_settings {
                self.settings_dialog
                    .open(&self.command_registry, &self.app_config);
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
                    &self.command_registry,
                );
                DockArea::new(&mut self.dock_state)
                    .style(Style::from_egui(ui.style().as_ref()))
                    .show_leaf_collapse_buttons(false)
                    .show_inside(ui, &mut tab_viewer);
            });
        });

        if ctx.input(|i| i.pointer.any_released()) {
            self.editor_context.interaction.timeline.dragged_item = None;
        }

        // Always pump audio to keep buffer full if playing (or pre-buffer)
        if self.editor_context.timeline.is_playing {
            self.project_service.pump_audio();
        }

        if self.editor_context.timeline.is_playing {
            // Audio Master Clock Sync
            // We trust the audio engine's time as the source of truth.
            let audio_time = self.project_service.get_audio_engine().get_current_time();

            // Cast to f32 for UI text/logic, but careful with precision for long videos?
            // editor_context uses f32 for current_time.
            self.editor_context.timeline.current_time = audio_time as f32;

            ctx.request_repaint();
        } else {
            // Reset accumulator when not playing to avoid jump on resume
            self.editor_context.timeline.playback_accumulator = 0.0;
        }
    }
}

fn setup_theme(ctx: &egui::Context, config: &config::AppConfig) {
    let mut visuals = Visuals::dark();
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(255, 120, 0);
    ctx.set_visuals(visuals);
    crate::ui::theme::apply_theme(ctx, config);
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);

    // Windows specific font path for MS Gothic
    let font_path = "C:\\Windows\\Fonts\\msgothic.ttc";

    if let Ok(font_data) = fs::read(font_path) {
        fonts.font_data.insert(
            "my_font".to_owned(),
            egui::FontData::from_owned(font_data)
                .tweak(egui::FontTweak {
                    scale: 1.2,
                    ..Default::default()
                })
                .into(),
        );

        // Add my_font to the proportional and monospace families
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "my_font".to_owned());
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .insert(0, "my_font".to_owned());

        ctx.set_fonts(fonts);
    } else {
        warn!("Warning: Failed to load font from {}", font_path);
        // Fallback to default egui fonts if MS Gothic fails to load
        ctx.set_fonts(fonts);
    }
}

fn create_default_project() -> (Arc<RwLock<Project>>, Uuid) {
    let default_project = Arc::new(RwLock::new(Project::new("Default Project")));
    // Add a default composition when the app starts
    let (default_comp, root_track) = Composition::new("Main Composition", 1920, 1080, 30.0, 60.0);
    let default_comp_id = default_comp.id;
    {
        let mut proj = default_project.write().unwrap();
        proj.add_node(library::model::project::Node::Track(root_track));
        proj.add_composition(default_comp);
    }
    (default_project, default_comp_id)
}

fn setup_plugin_manager(app_config: &config::AppConfig) -> Arc<library::plugin::PluginManager> {
    let plugin_manager = Arc::new(library::plugin::PluginManager::default());

    // Load plugins from configured paths
    for path in &app_config.plugins.paths {
        if let Err(e) = plugin_manager.load_sksl_plugins_from_directory(path) {
            log::error!("Failed to load SkSL plugins from {}: {}", path, e);
        }
    }

    // Apply saved loader priority
    if !app_config.plugins.loader_priority.is_empty() {
        plugin_manager.set_loader_priority(app_config.plugins.loader_priority.clone());
    }
    plugin_manager
}

fn setup_gpu_sharing(render_server: &Arc<RenderServer>, cc: &eframe::CreationContext<'_>) {
    // Zero-Copy GPU Sharing: Capture the main thread's OpenGL context handle
    // and pass it to the background render server. This enables sharing of textures.
    if let Some(handle) = library::rendering::skia_utils::get_current_context_handle() {
        #[allow(deprecated)]
        let hwnd = if let Ok(raw_handle) = cc.raw_window_handle() {
            #[allow(unused_variables)]
            let _ = raw_handle; // Silence usage warning on non-windows
            #[cfg(target_os = "windows")]
            match raw_handle {
                raw_window_handle::RawWindowHandle::Win32(h) => Some(h.hwnd.get() as isize),
                _ => None,
            }
            #[cfg(not(target_os = "windows"))]
            None
        } else {
            None
        };

        log::info!(
            "MyApp: Capturing main GL context handle: {}, HWND: {:?}",
            handle,
            hwnd
        );
        render_server.set_sharing_context(handle, hwnd);
    } else {
        log::warn!(
            "MyApp: Failed to capture main GL context handle. Preview might fall back to CPU copy."
        );
    }
}
