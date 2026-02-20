pub mod asset_handler;
pub mod clip_handler;
pub mod composition_handler;
pub mod keyframe_handler;
pub mod track_handler;

use crate::error::LibraryError;
use crate::model::project::project::Project;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Acquire a write lock on the project, converting poison errors to LibraryError.
pub fn write_project(
    project: &Arc<RwLock<Project>>,
) -> Result<RwLockWriteGuard<'_, Project>, LibraryError> {
    project
        .write()
        .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))
}

/// Acquire a read lock on the project, converting poison errors to LibraryError.
pub fn read_project(
    project: &Arc<RwLock<Project>>,
) -> Result<RwLockReadGuard<'_, Project>, LibraryError> {
    project
        .read()
        .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))
}
