use std::fs;
use std::io::Write;
// use std::sync::{Arc, RwLock};
use eframe::egui;
use egui_dock::DockState;
use log::{error, info, warn};

use library::model::project::project::Project;
use library::service::project_service::ProjectService;

use crate::action::HistoryManager;
use crate::command::CommandId;
use crate::model::ui_types::Tab;
use crate::state::context::EditorContext;

pub struct ActionContext<'a> {
    pub editor_context: &'a mut EditorContext,
    pub project_service: &'a mut ProjectService,
    pub history_manager: &'a mut HistoryManager,
    pub dock_state: &'a mut DockState<Tab>,
}

pub fn handle_command(
    ctx: &egui::Context,
    action: CommandId,
    context: ActionContext,
    trigger_settings: &mut bool,
) {
    match action {
        CommandId::Settings => {
            *trigger_settings = true;
        }
        CommandId::Export | CommandId::ShowCommandPalette => {
            // Handled in MyApp::update explicitly to open dialog
        }
        CommandId::NewProject => {
            // Logic to request new project - strictly speaking, this modifies MyApp state heavily.
            // For now, let's keep it simple or bubble up specific requests?
            // "New Project" resets everything. It might be better to return an enum or result indicating "NewProjectRequested".
            // But for now, we can try to implement it here if we have enough access.
            // MyApp::new_project logic:
            let mut new_project = Project::new("New Project");
            let default_comp = library::model::project::project::Composition::new(
                "Main Composition",
                1920,
                1080,
                30.0,
                60.0,
            );
            let new_comp_id = default_comp.id;
            new_project.add_composition(default_comp);
            context.project_service.set_project(new_project);

            context.editor_context.selection.composition_id = Some(new_comp_id);
            context.editor_context.selection.track_id = None;
            context.editor_context.selection.entity_id = None;
            context.editor_context.timeline.current_time = 0.0;

            // history_manager reset?
            // We can't replace the history_manager instance itself easily if it's borrowed.
            // We can clear it.
            // context.history_manager.clear(); // Need to implement clear() or similar
            // For now, let's assume we can just push the new state and maybe we live with the old history being accessible?
            // Actually MyApp::new_project creates a NEW HistoryManager.
            // If we can't do that here, we might need to expose clear method on HistoryManager.
            // Let's implement reset() on HistoryManager later?
            // Or just manually clear stacks.
            // For this refactor, maybe we delegate "NewProject" back to the caller (MyApp) via return value?
            // But let's try to do as much as possible here.

            context.history_manager.clear();
            if let Ok(proj_read) = context.project_service.get_project().read() {
                context
                    .history_manager
                    .push_project_state(proj_read.clone());
            }
        }
        CommandId::LoadProject => {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Project File", &["json"])
                .pick_file()
            {
                match fs::read_to_string(&path) {
                    Ok(s) => {
                        if let Err(e) = context.project_service.load_project(&s) {
                            error!("Failed to load project: {}", e);
                        } else {
                            // Reset history
                            // context.history_manager.reset(); // TODO
                            if let Ok(proj_read) = context.project_service.get_project().read() {
                                context
                                    .history_manager
                                    .push_project_state(proj_read.clone());
                            }
                            info!("Project loaded from {}", path.display());
                            context.editor_context.timeline.current_time = 0.0;
                        }
                    }
                    Err(e) => error!("Failed to read project file: {}", e),
                }
            }
        }
        CommandId::Save | CommandId::SaveAs => {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Project File", &["json"])
                .set_file_name("project.json")
                .save_file()
            {
                match context.project_service.save_project() {
                    Ok(json_str) => match fs::File::create(&path) {
                        Ok(mut file) => {
                            if let Err(e) = file.write_all(json_str.as_bytes()) {
                                error!("Failed to write project to file: {}", e);
                            } else {
                                info!("Project saved to {}", path.display());
                            }
                        }
                        Err(e) => error!("Failed to create file: {}", e),
                    },
                    Err(e) => error!("Failed to save project: {}", e),
                }
            }
        }
        CommandId::Quit => {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        CommandId::Undo => {
            if let Some(prev_state) = context.history_manager.undo() {
                context.project_service.set_project(prev_state);
            } else {
                warn!("Undo stack is empty (or at initial state).");
            }
        }
        CommandId::Redo => {
            if let Some(next_state) = context.history_manager.redo() {
                context.project_service.set_project(next_state);
            } else {
                warn!("Redo stack is empty.");
            }
        }
        CommandId::Delete => {
            if let Some(comp_id) = context.editor_context.selection.composition_id {
                if let Some(track_id) = context.editor_context.selection.track_id {
                    if let Some(entity_id) = context.editor_context.selection.entity_id {
                        if let Err(e) = context
                            .project_service
                            .remove_clip_from_track(comp_id, track_id, entity_id)
                        {
                            error!("Failed to remove entity: {:?}", e);
                        } else {
                            context.editor_context.selection.entity_id = None;
                            let current_state = context
                                .project_service
                                .get_project()
                                .read()
                                .unwrap()
                                .clone();
                            context.history_manager.push_project_state(current_state);
                        }
                    }
                }
            }
        }
        CommandId::ResetLayout => {
            *context.dock_state = crate::ui::tab_viewer::create_initial_dock_state();
        }
        CommandId::HandTool => {
            // HandTool is an interaction state command, handled by ViewportController.
            // No direct action needed here.
        }
        CommandId::TogglePlayback => {
            let is_playing = !context.editor_context.timeline.is_playing;
            context.editor_context.timeline.is_playing = is_playing;
            
            if is_playing {
                context.project_service.reset_audio_pump(context.editor_context.timeline.current_time as f64);
                if let Err(e) = context.project_service.audio_engine.play() {
                    log::error!("Failed to play audio: {}", e);
                }
            } else {
                 if let Err(e) = context.project_service.audio_engine.pause() {
                    log::error!("Failed to pause audio: {}", e);
                }
            }
        }
        CommandId::TogglePanel(tab) => {
            if let Some(index) = context.dock_state.find_tab(&tab) {
                context.dock_state.remove_tab(index);
            } else {
                context.dock_state.push_to_focused_leaf(tab);
            }
        }
    }
}
