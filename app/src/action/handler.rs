use eframe::egui;
use egui_dock::DockState;
use log::{error, info, warn};
use std::fs;
use std::io::Write;

use library::EditorService;

use crate::action::{
    HistoryManager, activate_composition_with_history, commit_live_project_edits,
    request_node_layout_command,
};
use crate::command::CommandId;
use crate::model::ui_types::Tab;
use crate::state::context::EditorContext;
use crate::state::context_types::SelectionTarget;
use crate::utils::lock::read_or_recover;

pub struct ActionContext<'a> {
    pub editor_context: &'a mut EditorContext,
    pub project_service: &'a mut EditorService,
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
        // File / Project Operations
        CommandId::NewProject
        | CommandId::LoadProject
        | CommandId::Save
        | CommandId::SaveAs
        | CommandId::Export => {
            handle_file_command(ctx, action, context);
        }

        // Edit Operations
        CommandId::Undo | CommandId::Redo | CommandId::Delete => {
            handle_edit_command(action, context);
        }

        // View / UI Operations
        CommandId::ResetLayout
        | CommandId::TogglePlayback
        | CommandId::TogglePanel(_)
        | CommandId::HandTool => {
            handle_view_command(action, context);
        }

        // Global / Misc Operations
        CommandId::Settings => {
            *trigger_settings = true;
        }
        CommandId::ShowCommandPalette => {
            // Handled in MyApp::update explicitly to open dialog
        }
        CommandId::NodeEditorCleanLayout
        | CommandId::NodeEditorCleanLayoutSelection
        | CommandId::NodeEditorCleanLayoutContainer
        | CommandId::NodeEditorCleanLayoutAll => {
            if !ctx.input(|input| input.pointer.primary_down()) {
                request_node_layout_command(&mut context.editor_context.node_editor_state, action);
            }
        }
        CommandId::Quit => {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn handle_file_command(_ctx: &egui::Context, action: CommandId, context: ActionContext) {
    match action {
        CommandId::NewProject => {
            let previous_project = context.project_service.get_project();
            // New Project deliberately discards the old undo branch. Close
            // any in-flight gesture against the old Project first so its
            // transient state is never reinterpreted against the replacement.
            commit_live_project_edits(
                context.editor_context,
                context.history_manager,
                &previous_project,
            );
            match context.project_service.create_new_project() {
                Ok(new_comp_id) => {
                    context.history_manager.clear();
                    let project = context.project_service.get_project();
                    activate_composition_with_history(
                        context.editor_context,
                        Some(new_comp_id),
                        context.history_manager,
                        &project,
                    );
                    context.editor_context.timeline.seek_to(0.0);
                    if let Ok(proj_read) = project.read() {
                        context
                            .editor_context
                            .reconcile_project_replacement(&proj_read);
                        context
                            .history_manager
                            .push_project_state(proj_read.clone());
                    };
                }
                Err(e) => error!("Failed to create new project: {}", e),
            }
        }
        CommandId::LoadProject => {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Project File", &["json"])
                .pick_file()
            {
                if let Err(e) = context.project_service.load_project_from_path(&path) {
                    error!("Failed to load project: {}", e);
                } else {
                    context.history_manager.clear();
                    let project = context.project_service.get_project();
                    if let Ok(proj_read) = project.read() {
                        context
                            .editor_context
                            .reconcile_project_replacement(&proj_read);
                        context
                            .editor_context
                            .interaction
                            .preview_viewport
                            .request_fit();
                        context
                            .history_manager
                            .push_project_state(proj_read.clone());
                    };
                    info!("Project loaded from {}", path.display());
                    context.editor_context.timeline.seek_to(0.0);
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
        CommandId::Export => {
            // Handled elsewhere or placeholder
        }
        _ => {}
    }
}

fn handle_edit_command(action: CommandId, context: ActionContext) {
    match action {
        CommandId::Undo => {
            let project = context.project_service.get_project();
            let current_state = project.read().ok().map(|project| project.clone());
            if let Some(prev_state) = current_state
                .as_ref()
                .and_then(|current| context.history_manager.undo(current))
            {
                if let Err(error) = context.project_service.set_project(prev_state) {
                    error!("Failed to restore Undo state: {error}");
                    return;
                }
                if let Ok(project) = project.read() {
                    context
                        .editor_context
                        .reconcile_project_replacement(&project);
                }
                context
                    .project_service
                    .reset_audio_pump(context.editor_context.timeline.current_time as f64);
            } else {
                warn!("Undo stack is empty (or at initial state).");
            }
        }
        CommandId::Redo => {
            let project = context.project_service.get_project();
            let current_state = project.read().ok().map(|project| project.clone());
            if let Some(next_state) = current_state
                .as_ref()
                .and_then(|current| context.history_manager.redo(current))
            {
                if let Err(error) = context.project_service.set_project(next_state) {
                    error!("Failed to restore Redo state: {error}");
                    return;
                }
                if let Ok(project) = project.read() {
                    context
                        .editor_context
                        .reconcile_project_replacement(&project);
                }
                context
                    .project_service
                    .reset_audio_pump(context.editor_context.timeline.current_time as f64);
            } else {
                warn!("Redo stack is empty.");
            }
        }
        CommandId::Delete => {
            let Some(target) = context.editor_context.selection.primary() else {
                return;
            };
            let removed = match target {
                SelectionTarget::Clip(clip_id) => {
                    let project = context.project_service.get_project();
                    let track_id = project
                        .read()
                        .ok()
                        .and_then(|project| project.find_track_for_clip(clip_id));
                    track_id.is_some_and(|track_id| {
                        context
                            .project_service
                            .remove_clip_from_track(track_id, clip_id)
                            .inspect_err(|error| {
                                error!("Failed to remove Clip {clip_id}: {error:?}");
                            })
                            .is_ok()
                    })
                }
                SelectionTarget::Node(node_id) => {
                    let project = context.project_service.get_project();
                    project.write().is_ok_and(|mut project| {
                        let removed = match project.remove_node(node_id) {
                            Ok(Some(_)) => true,
                            Ok(None) => {
                                error!("Failed to remove Node {node_id}: Node was not found");
                                false
                            }
                            Err(error) => {
                                error!("Failed to remove Node {node_id}: {error}");
                                false
                            }
                        };
                        if !removed {
                            context.editor_context.interaction.active_modal_error = Some(
                                "Cannot remove Node: structural Merge nodes belong to their Timeline container"
                                    .to_string(),
                            );
                        }
                        removed
                    })
                }
                SelectionTarget::Track(track_id) => {
                    let project = context.project_service.get_project();
                    let composition_id = project
                        .read()
                        .ok()
                        .and_then(|project| project.find_composition_for_track(track_id));
                    composition_id.is_some_and(|composition_id| {
                        context
                            .project_service
                            .remove_track(composition_id, track_id)
                            .inspect_err(|error| {
                                error!("Failed to remove Track {track_id}: {error:?}");
                            })
                            .is_ok()
                    })
                }
                SelectionTarget::Composition(composition_id) => {
                    let mut dialog = crate::ui::dialogs::confirmation::ConfirmationDialog::new();
                    dialog.open(
                        "Delete Composition",
                        "Are you sure you want to delete this composition?",
                        crate::ui::dialogs::confirmation::ConfirmationAction::DeleteComposition(
                            composition_id,
                        ),
                    );
                    context.editor_context.interaction.active_confirmation = Some(dialog);
                    false
                }
                // Timeline-first items are owned by TimelineEditorService and
                // are never deleted through the legacy Project command path.
                SelectionTarget::TimelineItem(_) => false,
            };
            if removed {
                let project = context.project_service.get_project();
                let current_state = read_or_recover(project.as_ref()).clone();
                context.editor_context.reconcile_selection(&current_state);
                context.history_manager.push_project_state(current_state);
            }
        }
        _ => {}
    }
}

fn handle_view_command(action: CommandId, context: ActionContext) {
    match action {
        CommandId::ResetLayout => {
            *context.dock_state = crate::ui::tab_viewer::create_initial_dock_state();
        }
        CommandId::TogglePlayback => {
            let is_playing = !context.editor_context.timeline.is_playing;
            context.editor_context.timeline.is_playing = is_playing;

            if is_playing {
                context
                    .project_service
                    .reset_audio_pump(context.editor_context.timeline.current_time as f64);
                if let Err(e) = context.project_service.get_audio_engine().play() {
                    log::error!("Failed to play audio: {}", e);
                }
            } else {
                // The frame-level AudioService playing -> paused transition
                // clears queued audio exactly once. Do not turn pause into a
                // seek/scrub request here.
                if let Err(e) = context.project_service.get_audio_engine().pause() {
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
        CommandId::HandTool => {
            // Handled by ViewportController logic elsewhere usually
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{ActionContext, handle_edit_command, handle_file_command};
    use crate::action::HistoryManager;
    use crate::command::CommandId;
    use crate::state::context::EditorContext;
    use crate::state::context_types::{GraphKeyframeDragState, SelectionTarget};
    use crate::ui::tab_viewer::create_initial_dock_state;
    use library::EditorService;
    use library::cache::CacheManager;
    use library::model::project::{NodeContainer, Project};
    use library::model::property::KeyframeId;
    use library::model::{Clip, Composition, Node};
    use library::plugin::PluginManager;
    use std::sync::{Arc, RwLock};
    use uuid::Uuid;

    #[test]
    fn delete_dispatches_same_uuid_node_and_clip_by_target_kind()
    -> Result<(), Box<dyn std::error::Error>> {
        let shared_id = Uuid::new_v4();
        let mut project_model = Project::new("typed delete");
        let (composition, track) = Composition::new("main", 320, 180, 30.0, 2.0);
        let composition_id = composition.id;
        let track_id = track.id;
        let mut clip = Clip::new("same UUID Clip", 0.0, 1.0);
        clip.id = shared_id;
        let mut node = Node::new_merge("same UUID Node");
        node.id = shared_id;
        assert!(
            project_model.add_track(track).is_ok(),
            "container structural Merge insertion must succeed"
        );
        assert!(
            project_model.add_composition(composition).is_ok(),
            "container structural Merge insertion must succeed"
        );
        project_model.add_clip(clip);
        project_model.add_node(node);
        project_model.attach_clip_to_track(track_id, shared_id)?;
        project_model
            .attach_node_to_container(NodeContainer::Composition(composition_id), shared_id)?;

        let project = Arc::new(RwLock::new(project_model));
        let mut service = EditorService::new(
            Arc::clone(&project),
            Arc::new(PluginManager::default()),
            Arc::new(CacheManager::new()),
        )?;
        let mut editor_context = EditorContext::new(composition_id);
        let mut history = HistoryManager::new();
        history.push_project_state(
            project
                .read()
                .map_err(|error| std::io::Error::other(error.to_string()))?
                .clone(),
        );
        let mut dock_state = create_initial_dock_state();

        editor_context.replace_selection(
            [
                SelectionTarget::Clip(shared_id),
                SelectionTarget::Node(shared_id),
            ],
            Some(SelectionTarget::Node(shared_id)),
        );
        handle_edit_command(
            CommandId::Delete,
            ActionContext {
                editor_context: &mut editor_context,
                project_service: &mut service,
                history_manager: &mut history,
                dock_state: &mut dock_state,
            },
        );
        {
            let project = project
                .read()
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            assert!(project.get_node(shared_id).is_none());
            assert!(project.get_clip(shared_id).is_some());
        }
        assert_eq!(
            editor_context.selection.targets(),
            &[SelectionTarget::Clip(shared_id)]
        );

        {
            let mut project = project
                .write()
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            let mut node = Node::new_merge("restored same UUID Node");
            node.id = shared_id;
            project.add_node(node);
            project
                .attach_node_to_container(NodeContainer::Composition(composition_id), shared_id)?;
        }
        editor_context.replace_selection(
            [
                SelectionTarget::Node(shared_id),
                SelectionTarget::Clip(shared_id),
            ],
            Some(SelectionTarget::Clip(shared_id)),
        );
        handle_edit_command(
            CommandId::Delete,
            ActionContext {
                editor_context: &mut editor_context,
                project_service: &mut service,
                history_manager: &mut history,
                dock_state: &mut dock_state,
            },
        );
        let project = project
            .read()
            .map_err(|error| std::io::Error::other(error.to_string()))?;
        assert!(project.get_clip(shared_id).is_none());
        assert!(project.get_node(shared_id).is_some());
        assert_eq!(
            editor_context.selection.targets(),
            &[SelectionTarget::Node(shared_id)]
        );
        Ok(())
    }

    #[test]
    fn new_project_keeps_only_one_replacement_baseline_after_interrupted_edit()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut initial = Project::new("old project");
        let (composition, track) = Composition::new("old", 320, 180, 30.0, 2.0);
        let old_composition_id = composition.id;
        assert!(
            initial.add_track(track).is_ok(),
            "container structural Merge insertion must succeed"
        );
        assert!(
            initial.add_composition(composition).is_ok(),
            "container structural Merge insertion must succeed"
        );
        let project = Arc::new(RwLock::new(initial.clone()));
        let mut service = EditorService::new(
            Arc::clone(&project),
            Arc::new(PluginManager::default()),
            Arc::new(CacheManager::new()),
        )?;
        let mut editor_context = EditorContext::new(old_composition_id);
        editor_context.graph_editor.keyframe_drag = Some(GraphKeyframeDragState {
            target: SelectionTarget::Node(Uuid::new_v4()),
            anchor: ("node:opacity".to_string(), KeyframeId::new()),
            origins: Vec::new(),
            changed: true,
        });
        project
            .write()
            .map_err(|error| std::io::Error::other(error.to_string()))?
            .name = "uncommitted old edit".to_string();
        let mut history = HistoryManager::new();
        history.push_project_state(initial);
        let mut dock_state = create_initial_dock_state();

        handle_file_command(
            &egui::Context::default(),
            CommandId::NewProject,
            ActionContext {
                editor_context: &mut editor_context,
                project_service: &mut service,
                history_manager: &mut history,
                dock_state: &mut dock_state,
            },
        );

        let current = project
            .read()
            .map_err(|error| std::io::Error::other(error.to_string()))?
            .clone();
        assert_eq!(history.undo_depth(), 1);
        assert_eq!(history.undo(&current), None);
        assert!(editor_context.graph_editor.keyframe_drag.is_none());
        assert!(
            editor_context
                .active_composition_id
                .is_some_and(|id| current.get_composition(id).is_some())
        );
        Ok(())
    }
}
