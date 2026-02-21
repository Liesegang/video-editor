use crate::error::LibraryError;
use crate::project::project::{Composition, Project};
use crate::timing::measure_info;
use std::fs;
use std::sync::Arc;

#[derive(Clone)]
pub struct ProjectModel {
    project: Arc<Project>,
    composition_index: usize,
}

impl ProjectModel {
    pub fn from_project_path(
        project_path: &str,
        composition_index: usize,
    ) -> Result<Self, LibraryError> {
        let project = measure_info(
            format!("Load project {}", project_path),
            || -> Result<Project, LibraryError> {
                let json = fs::read_to_string(project_path)?;
                let project = Project::load(&json)?;
                Ok(project)
            },
        )?;
        Self::new(Arc::new(project), composition_index)
    }

    pub fn new(project: Arc<Project>, composition_index: usize) -> Result<Self, LibraryError> {
        if project.compositions.get(composition_index).is_none() {
            return Err(LibraryError::project(format!(
                "Invalid composition index {}",
                composition_index
            )));
        }

        Ok(Self {
            project,
            composition_index,
        })
    }

    pub fn project(&self) -> &Arc<Project> {
        &self.project
    }

    pub fn composition_index(&self) -> usize {
        self.composition_index
    }

    pub fn composition(&self) -> &Composition {
        &self.project.compositions[self.composition_index]
    }
}
