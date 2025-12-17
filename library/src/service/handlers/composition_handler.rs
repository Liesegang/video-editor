use crate::error::LibraryError;
use crate::model::project::project::{Composition, Project};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub struct CompositionHandler;

impl CompositionHandler {
    pub fn add_composition(
        project: &Arc<RwLock<Project>>,
        name: &str,
        width: u64,
        height: u64,
        fps: f64,
        duration: f64,
    ) -> Result<Uuid, LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let composition = Composition::new(name, width, height, fps, duration);
        let id = composition.id;
        proj.add_composition(composition);
        Ok(id)
    }

    pub fn get_composition(
        project: &Arc<RwLock<Project>>,
        id: Uuid,
    ) -> Result<Composition, LibraryError> {
        let proj = project
            .read()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        proj.compositions
            .iter()
            .find(|c| c.id == id)
            .cloned()
            .ok_or(LibraryError::Project(format!(
                "Composition not found: {}",
                id
            )))
    }

    pub fn is_composition_used(project: &Arc<RwLock<Project>>, comp_id: Uuid) -> bool {
        if let Ok(proj) = project.read() {
            for comp in &proj.compositions {
                for track in &comp.tracks {
                    for clip in &track.clips {
                        if clip.reference_id == Some(comp_id) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }
}
