use crate::model::project::project::{Composition, Project};
use crate::util::timing::measure_info;
use std::error::Error;
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
    ) -> Result<Self, Box<dyn Error>> {
        let project = measure_info(format!("Load project {}", project_path), || -> Result<Project, Box<dyn Error>> {
            let json = fs::read_to_string(project_path)?;
            let project = Project::load(&json)?;
            Ok(project)
        })?;
        Self::new(Arc::new(project), composition_index)
    }

    pub fn new(project: Arc<Project>, composition_index: usize) -> Result<Self, Box<dyn Error>> {
        if project.compositions.get(composition_index).is_none() {
            return Err(format!("Invalid composition index {}", composition_index).into());
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