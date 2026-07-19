use library::model::project::Project;

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

#[cfg(test)]
mod tests {
    use super::HistoryManager;
    use library::model::project::Project;

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
}
