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

    /// Pops a project state from the undo stack, pushes the current state to the redo stack, and returns the popped state.
    pub fn undo(&mut self, current_project: Project) -> Option<Project> {
        if let Some(project) = self.undo_stack.pop() {
            self.redo_stack.push(current_project);
            Some(project)
        } else {
            None
        }
    }

    /// Pops a project state from the redo stack, pushes the current state to the undo stack, and returns the popped state.
    pub fn redo(&mut self, current_project: Project) -> Option<Project> {
        if let Some(project) = self.redo_stack.pop() {
            self.undo_stack.push(current_project);
            Some(project)
        } else {
            None
        }
    }
}
