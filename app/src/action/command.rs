use anyhow::Result;
use library::service::project_service::ProjectService;

/// Trait for an executable and reversible command.
pub trait Command: std::fmt::Debug {
    /// Executes the command, applying changes to the ProjectService.
    fn execute(&mut self, service: &mut ProjectService) -> Result<()>;

    /// Undoes the command, reverting changes made by execute.
    fn undo(&mut self, service: &mut ProjectService) -> Result<()>;

    /// Redoes the command, reapplying changes after an undo.
    fn redo(&mut self, service: &mut ProjectService) -> Result<()>;

    /// Returns a human-readable name for the command.
    fn name(&self) -> String {
        format!("{:?}", self)
    }
}