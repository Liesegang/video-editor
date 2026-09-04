//! Timeline-first application shell.
//!
//! The GUI owns exactly one `TimelineEditorService`. Every panel reads the
//! same immutable snapshot and sends commands back to that service; the old
//! graph-backed Project is intentionally absent from this entry point.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{self, Visuals};
use egui_dock::{DockArea, DockState, NodeIndex, Style};
use egui_phosphor::regular as icons;
use library::editor::{TimelineEditorService, TIMELINE_FIRST_E2E_FIXTURE};
use library::model::authoring::{AuthoringProject, ProjectRevision, TimelineId};
use library::plugin::PluginManager;
use library::RenderRequestId;
use library::{LibraryError, RenderServer};
use log::warn;
#[cfg(target_os = "windows")]
use raw_window_handle::HasWindowHandle;

use crate::command::{CommandContext, CommandId, CommandRegistry, CommandScope};
use crate::config;
use crate::model::ui_types::Tab;
use crate::state::authoring::{AuthoringSelection, AuthoringUiState};
use crate::ui::authoring_tab_viewer::AuthoringTabViewer;
use crate::ui::command_palette::CommandPalette;
use crate::ui::dialogs::settings_dialog::{SettingsDialog, SettingsResult};
use crate::ui::dialogs::unsaved_changes::{GuardedProjectAction, UnsavedChoice};
use crate::ui::timeline_first::AuthoringPreviewRuntime;

const QA_FIXTURE_ENV: &str = "RUVIE_QA_FIXTURE";

pub struct RuViEApp {
    pub service: TimelineEditorService,
    pub state: AuthoringUiState,
    pub dock_state: DockState<Tab>,
    plugins: Arc<PluginManager>,
    render_server: RenderServer,
    preview_runtime: AuthoringPreviewRuntime,
    export_runtime: AuthoringExportRuntime,
    command_registry: CommandRegistry,
    command_palette: CommandPalette,
    settings_dialog: SettingsDialog,
    app_config: config::AppConfig,
    saved_revision: Option<ProjectRevision>,
    deferred_guarded_action: Option<GuardedProjectAction>,
    unsaved_action: Option<GuardedProjectAction>,
    close_without_prompt: bool,
    qa_runtime: Option<crate::qa::QaRuntime>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkspacePreset {
    Edit,
    Motion,
    Data,
    Logic,
    Diagnostics,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppAction {
    Command(CommandId),
    Workspace(WorkspacePreset),
}

#[derive(Default)]
struct AuthoringExportRuntime {
    pending: Option<(RenderRequestId, String)>,
    next_request: u64,
}

impl RuViEApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Result<Self, LibraryError> {
        library::initialize_python_runtime()?;
        let app_config = config::load_config();
        setup_theme(&cc.egui_ctx, &app_config);
        setup_fonts(&cc.egui_ctx);

        let plugins = setup_plugin_manager(&app_config);
        let (service, fixture_timeline) = startup_service(plugins.as_ref())?;
        let project = service.snapshot()?;
        let active_timeline_id = fixture_timeline.unwrap_or(project.root_timeline_id);
        let mut state = AuthoringUiState::new(project.root_timeline_id);
        if active_timeline_id != project.root_timeline_id {
            state.active_timeline_id = active_timeline_id;
            state.active_instance_path = None;
        }
        initialize_timeline_view(&project, &mut state);

        let cache_manager = Arc::new(library::cache::CacheManager::new());
        let render_server = RenderServer::new(Arc::clone(&plugins), cache_manager);
        setup_gpu_sharing(&render_server, cc);

        let qa_runtime = match crate::qa::QaRuntime::from_env(&cc.egui_ctx) {
            Ok(runtime) => runtime,
            Err(error) => {
                log::error!("QA HTTP API is disabled: {error}");
                None
            }
        };
        let command_registry = CommandRegistry::new(&app_config);
        let settings_dialog = SettingsDialog::new(
            command_registry.clone(),
            app_config.clone(),
            Arc::clone(&plugins),
        );
        let saved_revision = Some(service.revision()?);

        cc.egui_ctx.request_repaint();
        Ok(Self {
            service,
            state,
            dock_state: create_workspace(WorkspacePreset::Edit),
            plugins,
            render_server,
            preview_runtime: AuthoringPreviewRuntime::default(),
            export_runtime: AuthoringExportRuntime {
                next_request: 1_u64 << 62,
                ..AuthoringExportRuntime::default()
            },
            command_registry,
            command_palette: CommandPalette::new(),
            settings_dialog,
            app_config,
            saved_revision,
            deferred_guarded_action: None,
            unsaved_action: None,
            close_without_prompt: false,
            qa_runtime,
        })
    }

    fn command_context(&self) -> CommandContext {
        CommandContext {
            scope: if self
                .dock_state
                .find_tab(&Tab::NodeEditor)
                .is_some_and(|position| {
                    self.dock_state
                        .focused_leaf()
                        .is_some_and(|focused| focused.0 == position.0 && focused.1 == position.1)
                }) {
                CommandScope::NodeEditor
            } else {
                CommandScope::Global
            },
            has_node_selection: !self.state.node_editor.selected_nodes.is_empty(),
        }
    }

    fn keyboard_action(&self, context: &egui::Context) -> Option<AppAction> {
        let text_input = context.wants_keyboard_input();
        self.command_registry.commands.iter().find_map(|command| {
            let _ = command.shortcut?;
            if text_input && !command.allow_when_focused {
                return None;
            }
            let available = command.is_available_in(self.command_context());
            (available && command_triggered(context, command))
                .then_some(AppAction::Command(command.id))
        })
    }

    fn handle_action(&mut self, context: &egui::Context, action: AppAction) {
        if let AppAction::Workspace(preset) = action {
            self.dock_state = create_workspace(preset);
            self.state.status = format!("{} workspace", workspace_name(preset));
            return;
        }
        let AppAction::Command(command) = action else {
            return;
        };
        let result = match command {
            CommandId::NewProject => {
                self.defer_guarded_action(context, GuardedProjectAction::NewProject);
                Ok(())
            }
            CommandId::LoadProject => {
                self.defer_guarded_action(context, GuardedProjectAction::OpenProject);
                Ok(())
            }
            CommandId::Save => self.save_project(),
            CommandId::SaveAs => self.save_project_as(),
            CommandId::Export => self.export_active_timeline_video(),
            CommandId::Quit => {
                self.defer_guarded_action(context, GuardedProjectAction::Quit);
                Ok(())
            }
            CommandId::Undo => self.undo(),
            CommandId::Redo => self.redo(),
            CommandId::Delete => self.delete_selection(),
            CommandId::Settings => {
                self.settings_dialog
                    .open(&self.command_registry, &self.app_config);
                Ok(())
            }
            CommandId::ResetLayout => {
                self.dock_state = create_workspace(WorkspacePreset::Edit);
                Ok(())
            }
            CommandId::TogglePanel(tab) => {
                toggle_panel(&mut self.dock_state, tab);
                Ok(())
            }
            CommandId::TogglePlayback => {
                self.state
                    .timeline
                    .set_playing(!self.state.timeline.is_playing);
                Ok(())
            }
            CommandId::ShowCommandPalette => {
                self.command_palette.toggle(self.command_context());
                Ok(())
            }
            CommandId::NodeEditorCleanLayout
            | CommandId::NodeEditorCleanLayoutSelection
            | CommandId::NodeEditorCleanLayoutContainer
            | CommandId::NodeEditorCleanLayoutAll => {
                self.state.node_editor.pending_layout_command = Some(command);
                focus_or_open_tab(&mut self.dock_state, Tab::NodeEditor);
                Ok(())
            }
        };
        if let Err(error) = result {
            self.state.error = Some(error.to_string());
        }
    }

    fn defer_guarded_action(&mut self, context: &egui::Context, action: GuardedProjectAction) {
        if self.deferred_guarded_action.is_some() || self.unsaved_action.is_some() {
            return;
        }
        context.memory_mut(|memory| {
            if let Some(id) = memory.focused() {
                memory.surrender_focus(id);
            }
        });
        self.deferred_guarded_action = Some(action);
        context.request_repaint();
    }

    fn has_unsaved_changes(&self) -> bool {
        self.service
            .revision()
            .ok()
            .zip(self.saved_revision)
            .is_some_and(|(current, saved)| current != saved)
    }

    fn execute_guarded_action(
        &mut self,
        context: &egui::Context,
        action: GuardedProjectAction,
    ) -> Result<(), LibraryError> {
        match action {
            GuardedProjectAction::NewProject => self.new_project(),
            GuardedProjectAction::OpenProject => self.open_project(),
            GuardedProjectAction::Quit => {
                self.close_without_prompt = true;
                context.send_viewport_cmd(egui::ViewportCommand::Close);
                Ok(())
            }
        }
    }

    fn process_guarded_action(&mut self, context: &egui::Context) {
        if let Some(action) = self.deferred_guarded_action.take() {
            if self.has_unsaved_changes() {
                self.unsaved_action = Some(action);
            } else if let Err(error) = self.execute_guarded_action(context, action) {
                self.state.error = Some(error.to_string());
            }
        }

        let Some(action) = self.unsaved_action else {
            return;
        };
        let project_name = self.service.snapshot().map_or_else(
            |_| "Current project".to_string(),
            |project| project.name.clone(),
        );
        let Some(choice) =
            crate::ui::dialogs::unsaved_changes::show(context, &project_name, action)
        else {
            return;
        };
        match choice {
            UnsavedChoice::Save => match self.save_project() {
                Ok(()) if !self.has_unsaved_changes() => {
                    self.unsaved_action = None;
                    if let Err(error) = self.execute_guarded_action(context, action) {
                        self.state.error = Some(error.to_string());
                    }
                }
                Ok(()) => {}
                Err(error) => self.state.error = Some(error.to_string()),
            },
            UnsavedChoice::Discard => {
                self.unsaved_action = None;
                if let Err(error) = self.execute_guarded_action(context, action) {
                    self.state.error = Some(error.to_string());
                }
            }
            UnsavedChoice::Cancel => self.unsaved_action = None,
        }
    }

    fn new_project(&mut self) -> Result<(), LibraryError> {
        let service = TimelineEditorService::create_default("Untitled Project")?;
        self.install_service(service, None)
    }

    fn open_project(&mut self) -> Result<(), LibraryError> {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("RuViE Project", &["ruvie", "json"])
            .pick_file()
        else {
            return Ok(());
        };
        let service = TimelineEditorService::open(&path)?;
        self.install_service(service, Some(path))
    }

    fn install_service(
        &mut self,
        service: TimelineEditorService,
        path: Option<PathBuf>,
    ) -> Result<(), LibraryError> {
        let project = service.snapshot()?;
        self.service = service;
        self.state = AuthoringUiState::new(project.root_timeline_id);
        initialize_timeline_view(&project, &mut self.state);
        self.preview_runtime = AuthoringPreviewRuntime::default();
        self.export_runtime = AuthoringExportRuntime {
            next_request: 1_u64 << 62,
            ..AuthoringExportRuntime::default()
        };
        self.saved_revision = Some(self.service.revision()?);
        self.state.status = path.map_or_else(
            || "New Project".to_string(),
            |path| format!("Opened {}", path.display()),
        );
        Ok(())
    }

    fn save_project(&mut self) -> Result<(), LibraryError> {
        if self.service.project_path()?.is_none() {
            return self.save_project_as();
        }
        self.service.save()?;
        self.saved_revision = Some(self.service.revision()?);
        self.state.status = "Project saved".to_string();
        Ok(())
    }

    fn save_project_as(&mut self) -> Result<(), LibraryError> {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("RuViE Project", &["ruvie"])
            .set_file_name("project.ruvie")
            .save_file()
        else {
            return Ok(());
        };
        self.service.save_as(&path)?;
        self.saved_revision = Some(self.service.revision()?);
        self.state.status = format!("Saved {}", path.display());
        Ok(())
    }

    fn undo(&mut self) -> Result<(), LibraryError> {
        if self.service.undo()?.is_some() {
            self.state.status = "Undo".to_string();
            self.reconcile()?;
        }
        Ok(())
    }

    fn redo(&mut self) -> Result<(), LibraryError> {
        if self.service.redo()?.is_some() {
            self.state.status = "Redo".to_string();
            self.reconcile()?;
        }
        Ok(())
    }

    fn delete_selection(&mut self) -> Result<(), LibraryError> {
        if let Some(AuthoringSelection::Item(item_id)) = self.state.selection.primary() {
            self.service.delete_item(item_id)?;
            self.state.selection.clear();
            self.state.inspector.invalidate();
            self.state.status = "Clip deleted".to_string();
        }
        Ok(())
    }

    fn export_active_timeline_video(&mut self) -> Result<(), LibraryError> {
        if self.export_runtime.pending.is_some() {
            return Err(LibraryError::Validation(
                "An export is already in progress".to_string(),
            ));
        }
        let (_, project, plan) = self
            .preview_runtime
            .snapshot_and_plan(&self.service)
            .map_err(LibraryError::Render)?;
        let timeline = project
            .timelines
            .get(&self.state.active_timeline_id)
            .ok_or_else(|| {
                LibraryError::Validation("The active Timeline no longer exists".to_string())
            })?;
        let file_name = format!("{}.mp4", timeline.name.replace(['/', '\\'], "-"));
        let Some(path) = rfd::FileDialog::new()
            .add_filter("MP4 Video", &["mp4"])
            .add_filter("Matroska Video", &["mkv"])
            .set_file_name(file_name)
            .save_file()
        else {
            return Ok(());
        };
        let output_path = path.to_str().map(str::to_owned).ok_or_else(|| {
            LibraryError::Validation("Export path is not valid Unicode".to_string())
        })?;
        let request_id = RenderRequestId::new(self.export_runtime.next_request);
        self.export_runtime.next_request = self.export_runtime.next_request.wrapping_add(1);
        if !self
            .render_server
            .send_authoring_video_export_request_at_instance(
                request_id,
                project,
                plan,
                self.state.active_timeline_id,
                self.state.active_instance_path.clone(),
                output_path.clone(),
            )
        {
            return Err(LibraryError::Runtime(
                "Export worker is busy; try again after the current export finishes".to_string(),
            ));
        }
        self.export_runtime.pending = Some((request_id, output_path.clone()));
        self.state.status = format!("Exporting {output_path}");
        Ok(())
    }

    fn poll_export(&mut self, context: &egui::Context) {
        while let Ok(result) = self.render_server.poll_authoring_export_result() {
            let expected = self
                .export_runtime
                .pending
                .as_ref()
                .is_some_and(|(request_id, _)| *request_id == result.request_id);
            if !expected {
                continue;
            }
            self.export_runtime.pending = None;
            match result.output {
                Ok(()) => {
                    self.state.error = None;
                    self.state.status = format!(
                        "Exported {} frames to {}",
                        result.frames_exported, result.output_path
                    );
                }
                Err(error) => self.state.error = Some(format!("Export failed: {error}")),
            }
        }
        if self.export_runtime.pending.is_some() {
            context.request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    fn reconcile(&mut self) -> Result<(), LibraryError> {
        let project = self.service.snapshot()?;
        self.state.reconcile(&project);
        Ok(())
    }

    fn update_playback(&mut self, project: &AuthoringProject) {
        if !self.state.timeline.is_playing {
            return;
        }
        let Some(timeline) = project.timelines.get(&self.state.active_timeline_id) else {
            self.state.timeline.set_playing(false);
            return;
        };
        let Some((started, anchor_frame)) = self.state.timeline.playback_anchor else {
            self.state.timeline.playback_anchor =
                Some((std::time::Instant::now(), self.state.timeline.current_frame));
            return;
        };
        let elapsed_frames = (started.elapsed().as_secs_f64() * timeline.fps.to_f64()).floor();
        let elapsed_frames = if elapsed_frames.is_finite() && elapsed_frames >= 0.0 {
            elapsed_frames.min(i64::MAX as f64) as i64
        } else {
            0
        };
        let frame = anchor_frame.saturating_add(elapsed_frames);
        let end_frame = (timeline.duration.to_seconds_f64() * timeline.fps.to_f64())
            .ceil()
            .clamp(0.0, i64::MAX as f64) as i64;
        if frame >= end_frame {
            self.state.timeline.seek_frame(end_frame.saturating_sub(1));
            self.state.timeline.set_playing(false);
        } else {
            self.state.timeline.current_frame = frame;
        }
    }

    fn show_settings(&mut self, context: &egui::Context) {
        let (_, result) = self.settings_dialog.show(context);
        if matches!(
            result,
            Some(SettingsResult::Save | SettingsResult::RestoreDefaults)
        ) {
            self.command_registry = self.settings_dialog.command_registry.clone();
            self.app_config = self.settings_dialog.config.clone();
            crate::ui::theme::apply_theme(context, &self.app_config);
            config::save_config(&self.app_config);
        }
    }
}

fn command_triggered(context: &egui::Context, command: &crate::command::Command) -> bool {
    let Some((modifiers, key)) = command.shortcut else {
        return false;
    };
    if !command.trigger_on_release {
        return context.input_mut(|input| input.consume_key(modifiers, key));
    }
    context.input(|input| {
        input.events.iter().any(|event| {
            matches!(
                event,
                egui::Event::Key {
                    key: event_key,
                    pressed: false,
                    modifiers: event_modifiers,
                    ..
                } if *event_key == key && modifiers_match(*event_modifiers, modifiers)
            )
        })
    })
}

fn modifiers_match(actual: egui::Modifiers, expected: egui::Modifiers) -> bool {
    if actual == expected {
        return true;
    }
    expected.command
        && actual.command
        && actual.alt == expected.alt
        && actual.shift == expected.shift
}

impl eframe::App for RuViEApp {
    fn raw_input_hook(&mut self, context: &egui::Context, raw_input: &mut egui::RawInput) {
        if let Some(runtime) = self.qa_runtime.as_mut() {
            runtime.inject_for_frame(context, raw_input);
        }
    }

    fn update(&mut self, context: &egui::Context, _frame: &mut eframe::Frame) {
        crate::qa::begin_frame(context);
        if let Some(runtime) = self.qa_runtime.as_ref() {
            runtime.issue_capture_for_frame(context);
        }

        if context.input(|input| input.viewport().close_requested()) && !self.close_without_prompt {
            context.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.defer_guarded_action(context, GuardedProjectAction::Quit);
        }

        if self.unsaved_action.is_none() {
            if let Some(action) = self.keyboard_action(context) {
                self.handle_action(context, action);
            }
        }
        if let Some(command) = self.command_palette.show(context, &self.command_registry) {
            self.handle_action(context, AppAction::Command(command));
        }
        self.show_settings(context);
        self.poll_export(context);

        let can_undo = self.service.can_undo().unwrap_or(false);
        let can_redo = self.service.can_redo().unwrap_or(false);
        let menu_action = egui::TopBottomPanel::top("menu_bar")
            .show(context, |ui| {
                draw_menu_bar(
                    ui,
                    &mut self.dock_state,
                    &self.command_registry,
                    can_undo,
                    can_redo,
                )
            })
            .inner;
        if let Some(action) = menu_action {
            self.handle_action(context, action);
        }

        match self.service.snapshot() {
            Ok(project) => {
                self.state.reconcile(&project);
                self.update_playback(&project);
                egui::TopBottomPanel::bottom("status_bar")
                    .exact_height(24.0)
                    .show(context, |ui| status_bar(ui, &project, self));
                egui::CentralPanel::default().show(context, |ui| {
                    let mut viewer = AuthoringTabViewer::new(
                        &project,
                        &mut self.state,
                        &self.service,
                        self.plugins.as_ref(),
                        &self.render_server,
                        &mut self.preview_runtime,
                    );
                    DockArea::new(&mut self.dock_state)
                        .style(Style::from_egui(ui.style().as_ref()))
                        .show_leaf_collapse_buttons(false)
                        .show_close_buttons(true)
                        .show_inside(ui, &mut viewer);
                });
                if std::mem::take(&mut self.state.node_editor.focus_requested) {
                    focus_or_open_tab(&mut self.dock_state, Tab::NodeEditor);
                }
                if let Some(runtime) = self.qa_runtime.as_mut() {
                    runtime.answer_authoring_ui_queries(
                        &project,
                        &self.state,
                        &self.dock_state,
                        &self.service,
                    );
                }
            }
            Err(error) => {
                self.state.error = Some(error.to_string());
                egui::CentralPanel::default().show(context, |ui| {
                    ui.centered_and_justified(|ui| ui.label("Project is unavailable"));
                });
            }
        }

        self.process_guarded_action(context);

        crate::qa::end_frame();
        if self.state.timeline.is_playing {
            context.request_repaint_after(std::time::Duration::from_millis(16));
        }
    }
}

fn draw_menu_bar(
    ui: &mut egui::Ui,
    dock_state: &mut DockState<Tab>,
    commands: &CommandRegistry,
    can_undo: bool,
    can_redo: bool,
) -> Option<AppAction> {
    let mut action = None;
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| {
            for (command, icon) in [
                (CommandId::NewProject, icons::FILE_PLUS),
                (CommandId::LoadProject, icons::FOLDER_OPEN),
                (CommandId::Save, icons::FLOPPY_DISK),
                (CommandId::SaveAs, icons::FLOPPY_DISK_BACK),
                (CommandId::Export, icons::EXPORT),
                (CommandId::Quit, icons::SIGN_OUT),
            ] {
                if command_button(ui, commands, command, icon, true) {
                    action = Some(AppAction::Command(command));
                    ui.close();
                }
            }
        });
        ui.menu_button("Edit", |ui| {
            for (command, enabled) in [
                (CommandId::Undo, can_undo),
                (CommandId::Redo, can_redo),
                (CommandId::Delete, true),
                (CommandId::Settings, true),
            ] {
                if command_button(ui, commands, command, "", enabled) {
                    action = Some(AppAction::Command(command));
                    ui.close();
                }
            }
        });
        ui.menu_button("View", |ui| {
            for tab in Tab::all() {
                let mut open = dock_state.find_tab(tab).is_some();
                if ui.checkbox(&mut open, panel_name(*tab)).changed() {
                    action = Some(AppAction::Command(CommandId::TogglePanel(*tab)));
                }
            }
            ui.separator();
            ui.menu_button("Workspace Presets", |ui| {
                for preset in [
                    WorkspacePreset::Edit,
                    WorkspacePreset::Motion,
                    WorkspacePreset::Data,
                    WorkspacePreset::Logic,
                    WorkspacePreset::Diagnostics,
                ] {
                    if ui.button(workspace_name(preset)).clicked() {
                        action = Some(AppAction::Workspace(preset));
                        ui.close();
                    }
                }
            });
            ui.separator();
            if command_button(ui, commands, CommandId::ShowCommandPalette, "", true) {
                action = Some(AppAction::Command(CommandId::ShowCommandPalette));
                ui.close();
            }
            if command_button(ui, commands, CommandId::ResetLayout, "", true) {
                action = Some(AppAction::Command(CommandId::ResetLayout));
                ui.close();
            }
        });
    });
    action
}

fn command_button(
    ui: &mut egui::Ui,
    commands: &CommandRegistry,
    id: CommandId,
    icon: &str,
    enabled: bool,
) -> bool {
    let Some(command) = commands.find(id) else {
        return false;
    };
    let text = if icon.is_empty() {
        command.text.clone()
    } else {
        format!("{icon} {}", command.text)
    };
    ui.add_enabled(
        enabled,
        egui::Button::new(text).shortcut_text(command.shortcut_text.clone()),
    )
    .clicked()
}

fn status_bar(ui: &mut egui::Ui, project: &AuthoringProject, app: &RuViEApp) {
    ui.horizontal(|ui| {
        if let Some(error) = &app.state.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        } else {
            ui.label(&app.state.status);
        }
        ui.separator();
        ui.label(&project.name);
        let dirty = app
            .service
            .revision()
            .ok()
            .zip(app.saved_revision)
            .is_some_and(|(current, saved)| current != saved);
        if dirty {
            ui.label("● Modified");
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(format!("Frame {}", app.state.timeline.current_frame));
        });
    });
}

fn create_workspace(preset: WorkspacePreset) -> DockState<Tab> {
    let center_tabs = match preset {
        WorkspacePreset::Logic => vec![Tab::NodeEditor, Tab::Preview],
        WorkspacePreset::Data => vec![Tab::Assets, Tab::Preview],
        WorkspacePreset::Diagnostics => vec![Tab::Preview, Tab::NodeEditor],
        WorkspacePreset::Edit | WorkspacePreset::Motion => vec![Tab::Preview],
    };
    let bottom_tabs = match preset {
        WorkspacePreset::Motion => vec![Tab::Timeline, Tab::GraphEditor],
        WorkspacePreset::Logic => vec![Tab::Timeline, Tab::GraphEditor],
        WorkspacePreset::Diagnostics => vec![Tab::Timeline, Tab::GraphEditor],
        WorkspacePreset::Edit | WorkspacePreset::Data => vec![Tab::Timeline],
    };
    let mut dock = DockState::new(center_tabs);
    let surface = dock.main_surface_mut();
    let bottom_ratio = if preset == WorkspacePreset::Motion {
        0.62
    } else {
        0.70
    };
    let [main, _] = surface.split_below(NodeIndex::root(), bottom_ratio, bottom_tabs);
    let [main, _] = surface.split_right(main, 0.78, vec![Tab::Inspector]);
    if preset != WorkspacePreset::Data {
        surface.split_left(main, 0.24, vec![Tab::Assets]);
    }
    dock
}

fn toggle_panel(dock: &mut DockState<Tab>, tab: Tab) {
    if let Some(position) = dock.find_tab(&tab) {
        dock.remove_tab(position);
    } else {
        dock.push_to_focused_leaf(tab);
    }
}

fn focus_or_open_tab(dock: &mut DockState<Tab>, tab: Tab) {
    if dock.find_tab(&tab).is_none() {
        dock.push_to_focused_leaf(tab);
    }
    if let Some(position) = dock.find_tab(&tab) {
        dock.set_active_tab(position);
        dock.set_focused_node_and_surface((position.0, position.1));
    }
}

fn panel_name(tab: Tab) -> &'static str {
    match tab {
        Tab::GraphEditor => "Curve Editor",
        _ => tab.name(),
    }
}

const fn workspace_name(preset: WorkspacePreset) -> &'static str {
    match preset {
        WorkspacePreset::Edit => "Edit",
        WorkspacePreset::Motion => "Motion",
        WorkspacePreset::Data => "Data",
        WorkspacePreset::Logic => "Logic",
        WorkspacePreset::Diagnostics => "Diagnostics",
    }
}

fn startup_service(
    plugins: &PluginManager,
) -> Result<(TimelineEditorService, Option<TimelineId>), LibraryError> {
    match std::env::var(QA_FIXTURE_ENV) {
        Ok(name) if name == TIMELINE_FIRST_E2E_FIXTURE => {
            let media = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("test_data")
                .join("e2e_media");
            let fixture = library::editor::build_timeline_first_e2e_fixture(&media, plugins)?;
            Ok((fixture.service, Some(fixture.info.timeline_id)))
        }
        Ok(name) => Err(LibraryError::Validation(format!(
            "Unknown Timeline-first QA fixture '{name}'"
        ))),
        Err(std::env::VarError::NotUnicode(_)) => Err(LibraryError::Validation(format!(
            "{QA_FIXTURE_ENV} is not valid Unicode"
        ))),
        Err(std::env::VarError::NotPresent) => {
            TimelineEditorService::create_default("Untitled Project").map(|service| (service, None))
        }
    }
}

fn initialize_timeline_view(project: &AuthoringProject, state: &mut AuthoringUiState) {
    if let Some(timeline) = project.timelines.get(&state.active_timeline_id) {
        state
            .timeline
            .expanded_tracks
            .extend(timeline.track_order.iter().copied());
    }
    if let Some(item_id) = project
        .items
        .values()
        .filter(|item| {
            project
                .tracks
                .get(&item.track_id)
                .is_some_and(|track| track.timeline_id == state.active_timeline_id)
        })
        .min_by_key(|item| (item.interval.start, item.layer, item.id))
        .map(|item| item.id)
    {
        state.selection.replace(AuthoringSelection::Item(item_id));
    }
}

fn setup_theme(context: &egui::Context, app_config: &config::AppConfig) {
    let mut visuals = Visuals::dark();
    visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(255, 120, 0);
    context.set_visuals(visuals);
    crate::ui::theme::apply_theme(context, app_config);
    crate::ui::theme::disable_display_text_selection(context);
}

fn setup_fonts(context: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    let font_path = "C:\\Windows\\Fonts\\msgothic.ttc";
    if let Ok(font_data) = fs::read(font_path) {
        fonts.font_data.insert(
            "ui_font".to_owned(),
            egui::FontData::from_owned(font_data)
                .tweak(egui::FontTweak {
                    scale: 1.2,
                    ..Default::default()
                })
                .into(),
        );
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .insert(0, "ui_font".to_owned());
        }
    } else {
        warn!("Failed to load {font_path}; using the bundled UI font");
    }
    context.set_fonts(fonts);
}

fn setup_plugin_manager(app_config: &config::AppConfig) -> Arc<PluginManager> {
    let plugins = Arc::new(PluginManager::default());
    let report = plugins.rescan_runtime_plugins(&app_config.plugins.paths);
    for (path, error) in report.failures {
        log::error!("Failed to load runtime plugin {}: {error}", path.display());
    }
    for path in &app_config.plugins.paths {
        if let Err(error) = plugins.load_sksl_plugins_from_directory(path) {
            log::error!("Failed to load SkSL plugins from {path}: {error}");
        }
    }
    if !app_config.plugins.loader_priority.is_empty() {
        plugins.set_loader_priority(app_config.plugins.loader_priority.clone());
    }
    plugins
}

fn setup_gpu_sharing(render_server: &RenderServer, _cc: &eframe::CreationContext<'_>) {
    let Some(handle) = library::rendering::skia_utils::get_current_context_handle() else {
        log::warn!("Preview GPU sharing is unavailable; renderer will use CPU readback");
        return;
    };
    #[cfg(target_os = "windows")]
    let hwnd = _cc
        .window_handle()
        .ok()
        .and_then(|window_handle| match window_handle.as_raw() {
            raw_window_handle::RawWindowHandle::Win32(handle) => Some(handle.hwnd.get()),
            _ => None,
        });
    #[cfg(not(target_os = "windows"))]
    let hwnd: Option<isize> = None;
    render_server.set_sharing_context(handle, hwnd);
}

#[cfg(test)]
mod tests;
