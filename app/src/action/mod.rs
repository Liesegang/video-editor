pub mod command;
pub mod commands;

use anyhow::Result;

pub struct HistoryManager {
    undo_stack: Vec<Box<dyn command::Command + 'static>>,
    redo_stack: Vec<Box<dyn command::Command + 'static>>,
}

impl HistoryManager {
    pub fn new() -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    /// Pushes a new command onto the undo stack. Clears the redo stack.
    pub fn push(&mut self, command: Box<dyn command::Command + 'static>) {
        self.undo_stack.push(command);
        self.redo_stack.clear();
    }

    /// Executes the next command on the undo stack, moves it to the redo stack.
    pub fn undo(&mut self, service: &mut library::service::project_service::ProjectService) -> Result<()> {
        if let Some(mut command) = self.undo_stack.pop() {
            command.undo(service)?;
            self.redo_stack.push(command);
        }
        Ok(())
    }

    /// Executes the next command on the redo stack, moves it back to the undo stack.
    pub fn redo(&mut self, service: &mut library::service::project_service::ProjectService) -> Result<()> {
        if let Some(mut command) = self.redo_stack.pop() {
            command.redo(service)?;
            self.undo_stack.push(command);
        }
        Ok(())
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }
}