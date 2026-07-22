use eframe::egui::{self, Visuals};
use egui_dock::{DockArea, DockState, Style};
use library::model::project::{Composition, Project};
use library::{EditorService, LibraryError};
use log::warn;
#[cfg(target_os = "windows")]
use raw_window_handle::HasWindowHandle;
use std::fs;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::action::{
    activate_composition_with_history, commit_live_project_edits,
    handler::{handle_command, ActionContext},
    HistoryManager,
};
use crate::command::{CommandContext, CommandId, CommandRegistry};
use crate::config;
use crate::model::ui_types::Tab;
use crate::shortcut::ShortcutManager;
use crate::state::context::EditorContext;
use crate::ui::command_palette::CommandPalette;
use crate::ui::dialogs::composition_dialog::CompositionDialog;
use crate::ui::dialogs::export_dialog::ExportDialog;
use crate::ui::dialogs::settings_dialog::SettingsDialog;
use crate::ui::tab_viewer::{active_command_scope, create_initial_dock_state, AppTabViewer};
use crate::utils::lock::read_or_recover;
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
    pub render_server: RenderServer,
    qa_runtime: Option<crate::qa::QaRuntime>,
}

type StartupProject = (Arc<RwLock<Project>>, Uuid, Option<crate::qa::FixtureInfo>);

impl RuViEApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, LibraryError> {
        library::initialize_python_runtime()?;
        let app_config = config::load_config();
        setup_theme(&cc.egui_ctx, &app_config);
        setup_fonts(&cc.egui_ctx);

        let plugin_manager = setup_plugin_manager(&app_config);
        let command_registry = CommandRegistry::new(&app_config);
        let (default_project, default_comp_id, qa_fixture) =
            create_startup_project(&plugin_manager)?;

        let cache_manager = Arc::new(library::cache::CacheManager::new());
        let project_service = EditorService::new(
            Arc::clone(&default_project),
            plugin_manager.clone(),
            cache_manager.clone(),
        )?;

        let mut editor_context = EditorContext::new(default_comp_id);
        if let Some(fixture) = &qa_fixture {
            editor_context
                .timeline
                .expanded_tracks
                .extend(fixture.expanded_tracks.iter().copied());
            editor_context.timeline.current_time = 2.0;
        }
        editor_context.available_fonts = library::rendering::skia_utils::get_available_fonts();

        let render_server = RenderServer::new(plugin_manager.clone(), cache_manager.clone());
        let qa_runtime = match crate::qa::QaRuntime::from_env(&cc.egui_ctx) {
            Ok(runtime) => runtime,
            Err(error) => {
                log::error!("QA HTTP API is disabled: {error}");
                None
            }
        };

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
                command_registry,
                app_config,
                plugin_manager.clone(),
            ),
            triggered_action: None,
            composition_dialog: CompositionDialog::new(),
            export_dialog: ExportDialog::new(plugin_manager, cache_manager),
            command_palette: CommandPalette::new(),
            render_server,
            qa_runtime,
        };

        if let Ok(proj_read) = app.project_service.get_project().read() {
            app.history_manager.push_project_state(proj_read.clone());
        }

        setup_gpu_sharing(&app.render_server, cc);

        cc.egui_ctx.request_repaint();
        Ok(app)
    }
}

impl eframe::App for RuViEApp {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        if let Some(runtime) = self.qa_runtime.as_mut() {
            runtime.inject_for_frame(ctx, raw_input);
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        crate::qa::begin_frame(ctx);
        if let Some(runtime) = self.qa_runtime.as_ref() {
            runtime.issue_capture_for_frame(ctx);
        }
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
            let active_comp_id = self.editor_context.active_composition_id;
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

        let command_context = CommandContext {
            scope: active_command_scope(
                &self.dock_state,
                ctx.pointer_hover_pos(),
                self.editor_context.node_editor_state.panel_rect,
                self.editor_context.active_composition_id.is_some(),
            ),
            has_node_selection: self
                .editor_context
                .selection
                .targets()
                .iter()
                .any(|target| target.node_id().is_some()),
        };
        let focused_command_context = CommandContext {
            scope: active_command_scope(
                &self.dock_state,
                None,
                self.editor_context.node_editor_state.panel_rect,
                self.editor_context.active_composition_id.is_some(),
            ),
            has_node_selection: command_context.has_node_selection,
        };
        let palette_origin_context =
            CommandContext::palette_origin(command_context, focused_command_context);

        // Palette
        if let Some(cmd_id) = self.command_palette.show(ctx, &self.command_registry) {
            self.triggered_action = Some(cmd_id);
        }

        // 6. Confirmation Dialog
        if let Some(dialog) = &mut self.editor_context.interaction.active_confirmation {
            if let Some(action) = dialog.show(ctx) {
                match action {
                    crate::ui::dialogs::confirmation::ConfirmationAction::DeleteAsset(id) => {
                        commit_live_project_edits(
                            &mut self.editor_context,
                            &mut self.history_manager,
                            &self.project,
                        );
                        if let Err(e) = self.project_service.remove_asset_fully(id) {
                            log::error!("Failed to remove asset: {}", e);
                        } else {
                            // Push history
                            let project = self.project_service.get_project();
                            let current_state = read_or_recover(project.as_ref()).clone();
                            self.editor_context.reconcile_selection(&current_state);
                            self.history_manager.push_project_state(current_state);
                        }
                    }
                    crate::ui::dialogs::confirmation::ConfirmationAction::DeleteComposition(id) => {
                        commit_live_project_edits(
                            &mut self.editor_context,
                            &mut self.history_manager,
                            &self.project,
                        );
                        if let Err(e) = self.project_service.remove_composition_fully(id) {
                            log::error!("Failed to remove composition: {}", e);
                        } else {
                            // Clear selection if needed
                            if self.editor_context.active_composition_id == Some(id) {
                                activate_composition_with_history(
                                    &mut self.editor_context,
                                    None,
                                    &mut self.history_manager,
                                    &self.project,
                                );
                            }
                            let project = self.project_service.get_project();
                            let current_state = read_or_recover(project.as_ref()).clone();
                            self.editor_context.reconcile_selection(&current_state);
                            self.history_manager.push_project_state(current_state);
                        }
                    }
                }
                // Reset dialog logic is handled inside show() which sets is_open=false,
                // but we can set the Option to None if we want to clean up.
                // For now, keeping it is fine as is_open controls visibility.
            }
        }

        // 7. Generic Error Modal
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
            && !self.export_dialog.is_open
            && !self.editor_context.keyframe_dialog.is_open
            && !self.command_palette.is_open;
        if main_ui_enabled && !is_listening_for_shortcut {
            if let Some(action_id) = self.shortcut_manager.handle_shortcuts(
                ctx,
                &self.command_registry,
                &mut self.editor_context,
                command_context,
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
                self.command_palette.toggle(palette_origin_context);
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
                tab_viewer.finish_frame();
            });
        });

        if ctx.input(|i| i.pointer.any_released()) {
            self.editor_context.interaction.dragged_item = None;
        }

        // Audio follows the same selected Composition as every visual view.
        // Selection is transient UI state; authored data remains exclusively
        // in the authoritative Project.
        self.project_service.set_active_composition(
            self.editor_context.active_composition_id,
            self.editor_context.timeline.current_time as f64,
        );

        self.project_service
            .get_audio_service()
            .set_playing(self.editor_context.timeline.is_playing);
        // The pump also completes an asynchronous scrub preview while paused;
        // regular playback samples are emitted only when `is_playing` is true.
        self.project_service.pump_audio();
        if self.project_service.get_audio_service().has_pending_work() {
            ctx.request_repaint_after(std::time::Duration::from_millis(10));
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

        crate::qa::end_frame();
        if let Some(runtime) = self.qa_runtime.as_mut() {
            let plugin_manager = self.project_service.get_plugin_manager();
            runtime.answer_ui_queries(
                &self.project,
                &self.editor_context,
                &self.dock_state,
                &self.history_manager,
                plugin_manager.as_ref(),
            );
        }
    }
}

fn setup_theme(ctx: &egui::Context, config: &config::AppConfig) {
    let mut visuals = Visuals::dark();
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(255, 120, 0);
    ctx.set_visuals(visuals);
    crate::ui::theme::apply_theme(ctx, config);
    crate::ui::theme::disable_display_text_selection(ctx);
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

fn create_startup_project(
    plugin_manager: &Arc<library::plugin::PluginManager>,
) -> Result<StartupProject, LibraryError> {
    let project = Arc::new(RwLock::new(Project::new("Default Project")));
    match crate::qa::install_fixture_from_env(&project, plugin_manager) {
        Ok(Some(fixture)) => {
            return Ok((project, fixture.composition_id, Some(fixture)));
        }
        Ok(None) => {}
        Err(error) => log::error!("QA fixture is disabled: {error}"),
    }

    // Add a default composition when the app starts
    let (default_comp, root_track) = Composition::new("Main Composition", 1920, 1080, 30.0, 60.0);
    let default_comp_id = default_comp.id;
    {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("startup project lock poisoned".to_string()))?;
        proj.add_track(root_track)
            .and_then(|()| proj.add_composition(default_comp))
            .map_err(|error| LibraryError::Project(error.to_string()))?;
    }
    Ok((project, default_comp_id, None))
}

fn setup_plugin_manager(app_config: &config::AppConfig) -> Arc<library::plugin::PluginManager> {
    let plugin_manager = Arc::new(library::plugin::PluginManager::default());

    let runtime_report = plugin_manager.rescan_runtime_plugins(&app_config.plugins.paths);
    for bundle in &runtime_report.loaded_bundles {
        log::info!("Loaded runtime plugin bundle: {}", bundle.display());
    }
    for (path, error) in &runtime_report.failures {
        log::error!(
            "Failed to load runtime plugin bundle from {}: {}",
            path.display(),
            error
        );
    }

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

fn setup_gpu_sharing(render_server: &RenderServer, _cc: &eframe::CreationContext<'_>) {
    // Zero-Copy GPU Sharing: Capture the main thread's OpenGL context handle
    // and pass it to the background render server. This enables sharing of textures.
    if let Some(handle) = library::rendering::skia_utils::get_current_context_handle() {
        #[cfg(target_os = "windows")]
        let hwnd =
            _cc.window_handle()
                .ok()
                .and_then(|window_handle| match window_handle.as_raw() {
                    raw_window_handle::RawWindowHandle::Win32(h) => Some(h.hwnd.get() as isize),
                    _ => None,
                });
        #[cfg(not(target_os = "windows"))]
        let hwnd: Option<isize> = None;

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
