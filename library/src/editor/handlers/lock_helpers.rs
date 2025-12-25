//! Helper functions for acquiring project locks with consistent error handling.

use crate::error::LibraryError;
use crate::model::project::project::Project;
use std::sync::{Arc, RwLock};

/// Execute a function with a write lock on the project.
/// Provides consistent error handling for lock acquisition.
pub fn with_project_write<F, R>(project: &Arc<RwLock<Project>>, f: F) -> Result<R, LibraryError>
where
    F: FnOnce(&mut Project) -> Result<R, LibraryError>,
{
    let mut proj = project
        .write()
        .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
    f(&mut proj)
}

/// Execute a function with a read lock on the project.
/// Provides consistent error handling for lock acquisition.
pub fn with_project_read<F, R>(project: &Arc<RwLock<Project>>, f: F) -> Result<R, LibraryError>
where
    F: FnOnce(&Project) -> Result<R, LibraryError>,
{
    let proj = project
        .read()
        .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
    f(&proj)
}
