use library::model::project::project::Project;

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
    pub fn push_project_state(&mut self, project: Project) {
        self.undo_stack.push(project);
        self.redo_stack.clear();
    }

    /// Undoes the last action.
    /// Pops the current state (top of undo stack) and pushes it to the redo stack.
    /// Returns the *new* top of the undo stack (the state before the action), without popping it.
    /// If the undo stack has 1 or 0 elements, returns None (cannot undo initial state).
    pub fn undo(&mut self) -> Option<Project> {
        if self.undo_stack.len() <= 1 {
            return None;
        }

        if let Some(current_state) = self.undo_stack.pop() {
            self.redo_stack.push(current_state);
            // Return a clone of the new top (the previous state)
            self.undo_stack.last().cloned()
        } else {
            None
        }
    }

    /// Redoes the last undone action.
    /// Pops from redo stack, pushes to undo stack, and returns the new current state.
    pub fn redo(&mut self) -> Option<Project> {
        if let Some(next_state) = self.redo_stack.pop() {
            self.undo_stack.push(next_state.clone());
            Some(next_state)
        } else {
            None
        }
    }
}
