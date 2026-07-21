//! Authoritative Project adoption, persistence, and shared service state.

use crate::error::LibraryError;
use crate::model::project::{Composition, Project};
use crate::plugin::PluginManager;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// History-free commands over the single authoritative Project instance.
///
/// Higher-level editor services own undo history; this type owns only shared
/// Project access and plugin-backed model mutations.
pub struct ProjectManager {
    pub(super) project: Arc<RwLock<Project>>,
    pub(super) plugin_manager: Arc<PluginManager>,
}

impl ProjectManager {
    pub fn new(project: Arc<RwLock<Project>>, plugin_manager: Arc<PluginManager>) -> Self {
        Self {
            project,
            plugin_manager,
        }
    }

    pub fn get_project(&self) -> Arc<RwLock<Project>> {
        Arc::clone(&self.project)
    }

    pub fn get_plugin_manager(&self) -> Arc<PluginManager> {
        Arc::clone(&self.plugin_manager)
    }

    pub fn set_project(&self, new_project: Project) -> Result<(), LibraryError> {
        Self::validate_project_for_adoption(&new_project)?;
        let mut project_write = self.project.write().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project write lock: {}", e))
        })?;
        *project_write = new_project;
        Ok(())
    }

    pub fn load_project(&self, json_str: &str) -> Result<Project, LibraryError> {
        let new_project = Project::load(json_str)?;
        Self::validate_project_for_adoption(&new_project)?;
        let mut project_write = self.project.write().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project write lock: {}", e))
        })?;
        *project_write = new_project.clone();
        Ok(new_project)
    }

    fn validate_project_for_adoption(project: &Project) -> Result<(), LibraryError> {
        let errors = project.validation_issues();
        if errors.is_empty() {
            return Ok(());
        }
        Err(LibraryError::Validation(
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; "),
        ))
    }

    pub fn create_new_project(&self) -> Result<(Uuid, Project), LibraryError> {
        let mut new_project = Project::new("New Project");
        let (default_comp, root_track) =
            Composition::new("Main Composition", 1920, 1080, 30.0, 60.0);
        let new_comp_id = default_comp.id;
        new_project
            .add_track(root_track)
            .and_then(|()| new_project.add_composition(default_comp))
            .map_err(|error| LibraryError::Project(error.to_string()))?;

        let mut project_write = self.project.write().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project write lock: {}", e))
        })?;
        *project_write = new_project.clone();

        Ok((new_comp_id, new_project))
    }

    pub fn save_project(&self) -> Result<String, LibraryError> {
        let project_read = self.project.read().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project read lock: {}", e))
        })?;
        Ok(project_read.save()?)
    }
}
