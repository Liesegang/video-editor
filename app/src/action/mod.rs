use library::model::project::Project;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::state::context::EditorContext;
use crate::state::context_types::PreviewPrimaryGesture;
use crate::utils::lock::read_or_recover;

pub mod handler;

pub struct HistoryManager {
    undo_stack: Vec<Project>,
    redo_stack: Vec<Project>,
}

impl HistoryManager {
    pub fn new() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    /// Pushes a new project state onto the undo stack. Clears the redo stack.
    /// If the new state is identical to the current top of the stack, the push is ignored (heuristically deduplicated).
    pub fn push_project_state(&mut self, project: Project) {
        if let Some(last) = self.undo_stack.last() {
            if last == &project {
                return;
            }
        }
        self.undo_stack.push(project);
        self.redo_stack.clear();
    }

    /// Undoes the last action.
    /// Pops the current state (top of undo stack) and pushes it to the redo stack.
    /// Returns the *new* top of the undo stack (the state before the action), without popping it.
    /// If the undo stack has 1 or 0 elements, returns None (cannot undo initial state).
    pub fn undo(&mut self, current_state: &Project) -> Option<Project> {
        let recorded_current = self.undo_stack.last()? == current_state;

        // A mutation path should normally push its committed state. Preserve an
        // uncommitted current state as the redo target as a last line of
        // defence, so a history omission never makes the edit unrecoverable.
        if !recorded_current {
            let previous_state = self.undo_stack.last()?.clone();
            self.redo_stack.push(current_state.clone());
            return Some(previous_state);
        }

        if self.undo_stack.len() <= 1 {
            return None;
        }

        let current_state = self.undo_stack.pop()?;
        self.redo_stack.push(current_state);
        self.undo_stack.last().cloned()
    }

    /// Redoes the last undone action.
    /// Pops from redo stack, pushes to undo stack, and returns the new current state.
    pub fn redo(&mut self, current_state: &Project) -> Option<Project> {
        // A new unrecorded edit after Undo invalidates the redo branch just as
        // `push_project_state` does for a normally committed edit.
        if self.undo_stack.last() != Some(current_state) {
            self.redo_stack.clear();
            return None;
        }

        if let Some(next_state) = self.redo_stack.pop() {
            self.undo_stack.push(next_state.clone());
            Some(next_state)
        } else {
            None
        }
    }

    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }

    pub fn undo_depth(&self) -> usize {
        self.undo_stack.len()
    }

    pub fn redo_depth(&self) -> usize {
        self.redo_stack.len()
    }
}

fn has_live_project_edit(editor_context: &EditorContext) -> bool {
    editor_context
        .graph_editor
        .keyframe_drag
        .as_ref()
        .is_some_and(|drag| drag.changed)
        || editor_context.interaction.dragged_entity_has_moved
        || editor_context.interaction.is_resizing_entity
        || editor_context.interaction.is_moving_selected_entity
        || editor_context.interaction.body_drag_state.is_some()
        || editor_context.interaction.gizmo_state.is_some()
        || editor_context
            .interaction
            .vector_editor_state
            .as_ref()
            .is_some_and(|state| state.selected_handle.is_some())
        || editor_context.interaction.preview_viewport.primary_gesture
            == PreviewPrimaryGesture::Content
        || editor_context.interaction.timeline_track_reorder.is_some()
        || editor_context.node_editor_state.layout_changed_during_drag
        || editor_context.node_editor_state.node_reparent.is_some()
        || editor_context.node_editor_state.container_resize.is_some()
        || editor_context
            .node_editor_state
            .pending_continuous_edit
            .is_some()
}

/// Commit Project mutations already applied by an interrupted live UI gesture.
///
/// Timeline, Preview, Graph Editor, and Node Editor update the authoritative
/// Project incrementally, then normally push history on pointer release. A
/// Composition switch can invalidate their transient release state first, so
/// it must snapshot the current Project once before clearing those gestures.
/// HistoryManager deduplicates active gestures that have not changed Project.
pub fn commit_live_project_edits(
    editor_context: &mut EditorContext,
    history_manager: &mut HistoryManager,
    project: &Arc<RwLock<Project>>,
) -> bool {
    if !has_live_project_edit(editor_context) {
        return false;
    }
    history_manager.push_project_state(read_or_recover(project.as_ref()).clone());

    editor_context.graph_editor.keyframe_drag = None;
    editor_context.interaction.dragged_entity_original_track_id = None;
    editor_context.interaction.dragged_entity_hovered_track_id = None;
    editor_context.interaction.dragged_entity_has_moved = false;
    editor_context.interaction.timeline_track_reorder = None;
    editor_context.interaction.is_resizing_entity = false;
    editor_context.interaction.is_moving_selected_entity = false;
    editor_context.interaction.body_drag_state = None;
    editor_context.interaction.gizmo_state = None;
    editor_context.interaction.timeline_selection_drag_start = None;
    editor_context.interaction.preview_selection_drag_start = None;
    if let Some(state) = &mut editor_context.interaction.vector_editor_state {
        state.selected_handle = None;
    }
    editor_context.interaction.preview_viewport.primary_gesture = PreviewPrimaryGesture::Idle;
    editor_context.node_editor_state.layout_changed_during_drag = false;
    editor_context.node_editor_state.node_reparent = None;
    editor_context.node_editor_state.moved_node_ids.clear();
    editor_context.node_editor_state.container_resize = None;
    editor_context.node_editor_state.pending_continuous_edit = None;
    true
}

/// Navigate to a Composition without losing an in-flight authoritative edit.
pub fn activate_composition_with_history(
    editor_context: &mut EditorContext,
    composition_id: Option<Uuid>,
    history_manager: &mut HistoryManager,
    project: &Arc<RwLock<Project>>,
) -> bool {
    if editor_context.active_composition_id == composition_id {
        return false;
    }
    commit_live_project_edits(editor_context, history_manager, project);
    editor_context.activate_composition(composition_id)
}

#[cfg(test)]
mod tests {
    use super::{activate_composition_with_history, HistoryManager};
    use crate::state::context::EditorContext;
    use crate::state::context_types::{
        BodyDragState, GraphKeyframeDragState, NodeEditorPendingEdit, PreviewPrimaryGesture,
        SelectionTarget,
    };
    use library::model::project::{PortOwner, Project};
    use library::model::property::KeyframeId;
    use std::collections::HashMap;
    use std::sync::{Arc, RwLock};
    use uuid::Uuid;

    #[test]
    fn undo_redo_restores_committed_project_states() {
        let initial = Project::new("initial");
        let mut edited = initial.clone();
        edited.name = "edited".to_string();

        let mut history = HistoryManager::new();
        history.push_project_state(initial.clone());
        history.push_project_state(edited.clone());

        assert_eq!(history.undo(&edited), Some(initial.clone()));
        assert_eq!(history.redo(&initial), Some(edited));
    }

    #[test]
    fn undo_preserves_an_uncommitted_current_state_for_redo() {
        let initial = Project::new("initial");
        let mut uncommitted = initial.clone();
        uncommitted.name = "uncommitted".to_string();

        let mut history = HistoryManager::new();
        history.push_project_state(initial.clone());

        assert_eq!(history.undo(&uncommitted), Some(initial.clone()));
        assert_eq!(history.redo(&initial), Some(uncommitted));
    }

    #[test]
    fn an_uncommitted_edit_after_undo_invalidates_redo() {
        let initial = Project::new("initial");
        let mut edited = initial.clone();
        edited.name = "edited".to_string();
        let mut divergent = initial.clone();
        divergent.name = "divergent".to_string();

        let mut history = HistoryManager::new();
        history.push_project_state(initial.clone());
        history.push_project_state(edited.clone());
        assert_eq!(history.undo(&edited), Some(initial));
        assert_eq!(history.redo(&divergent), None);
    }

    #[test]
    fn composition_navigation_commits_all_interrupted_editors_once() {
        let first_composition = Uuid::new_v4();
        let second_composition = Uuid::new_v4();
        let node_id = Uuid::new_v4();
        let keyframe_id = KeyframeId::new();
        let original = Project::new("before live gestures");
        let project = Arc::new(RwLock::new(original.clone()));
        project.write().unwrap().name = "after live gestures".to_string();
        let edited = project.read().unwrap().clone();
        let mut context = EditorContext::new(first_composition);
        context.graph_editor.keyframe_drag = Some(GraphKeyframeDragState {
            target: SelectionTarget::Node(node_id),
            anchor: ("node:opacity".to_string(), keyframe_id),
            origins: Vec::new(),
            changed: true,
        });
        context.interaction.dragged_entity_has_moved = true;
        context.interaction.is_resizing_entity = true;
        context.interaction.is_moving_selected_entity = true;
        context.interaction.body_drag_state = Some(BodyDragState {
            start_mouse_pos: egui::Pos2::ZERO,
            original_positions: HashMap::new(),
            preview_targets: Vec::new(),
            has_changed: false,
        });
        context.interaction.preview_viewport.primary_gesture = PreviewPrimaryGesture::Content;
        context.node_editor_state.layout_changed_during_drag = true;
        context.node_editor_state.pending_continuous_edit = Some(NodeEditorPendingEdit {
            owner: PortOwner::Node(node_id),
            key: "opacity".to_string(),
        });
        let mut history = HistoryManager::new();
        history.push_project_state(original.clone());

        assert!(activate_composition_with_history(
            &mut context,
            Some(second_composition),
            &mut history,
            &project,
        ));

        assert_eq!(
            history.undo_depth(),
            2,
            "all dirty editors share one commit"
        );
        assert_eq!(history.undo(&edited), Some(original));
        assert!(context.graph_editor.keyframe_drag.is_none());
        assert!(!context.interaction.dragged_entity_has_moved);
        assert!(!context.interaction.is_resizing_entity);
        assert!(!context.interaction.is_moving_selected_entity);
        assert!(context.interaction.body_drag_state.is_none());
        assert_eq!(
            context.interaction.preview_viewport.primary_gesture,
            PreviewPrimaryGesture::Idle
        );
        assert!(!context.node_editor_state.layout_changed_during_drag);
        assert!(context.node_editor_state.pending_continuous_edit.is_none());
    }

    #[test]
    fn activating_current_composition_does_not_flush_or_clear_live_edit() {
        let composition_id = Uuid::new_v4();
        let node_id = Uuid::new_v4();
        let keyframe_id = KeyframeId::new();
        let original = Project::new("before same-target navigation");
        let project = Arc::new(RwLock::new(original.clone()));
        project.write().unwrap().name = "uncommitted".to_string();
        let mut context = EditorContext::new(composition_id);
        context.graph_editor.keyframe_drag = Some(GraphKeyframeDragState {
            target: SelectionTarget::Node(node_id),
            anchor: ("node:opacity".to_string(), keyframe_id),
            origins: Vec::new(),
            changed: true,
        });
        let mut history = HistoryManager::new();
        history.push_project_state(original);

        assert!(!activate_composition_with_history(
            &mut context,
            Some(composition_id),
            &mut history,
            &project,
        ));
        assert_eq!(history.undo_depth(), 1);
        assert!(context.graph_editor.keyframe_drag.is_some());
    }
}
