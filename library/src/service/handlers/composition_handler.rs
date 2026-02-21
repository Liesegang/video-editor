use crate::error::LibraryError;
use crate::project::node::Node;
use crate::project::project::{Composition, Project};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub struct CompositionHandler;

impl CompositionHandler {
    pub fn update_composition(
        project: &Arc<RwLock<Project>>,
        id: Uuid,
        name: &str,
        width: u32,
        height: u32,
        fps: f64,
        duration: f64,
    ) -> Result<(), LibraryError> {
        let mut proj = super::write_project(project)?;
        let comp =
            proj.compositions
                .iter_mut()
                .find(|c| c.id == id)
                .ok_or(LibraryError::project(format!(
                    "Composition not found: {}",
                    id
                )))?;

        comp.name = name.to_string();
        comp.width = width as u64;
        comp.height = height as u64;
        comp.fps = fps;
        comp.duration = duration;

        Ok(())
    }

    pub fn add_composition(
        project: &Arc<RwLock<Project>>,
        name: &str,
        width: u64,
        height: u64,
        fps: f64,
        duration: f64,
    ) -> Result<Uuid, LibraryError> {
        let mut proj = super::write_project(project)?;

        // Composition::new returns (Composition, TrackData)
        let (composition, root_track) = Composition::new(name, width, height, fps, duration);
        let id = composition.id;

        // Add root track to nodes registry
        proj.add_node(Node::Track(root_track));
        proj.add_composition(composition);

        Ok(id)
    }

    #[allow(dead_code)]
    pub fn remove_composition(
        project: &Arc<RwLock<Project>>,
        id: Uuid,
    ) -> Result<Option<Composition>, LibraryError> {
        let mut proj = super::write_project(project)?;
        Ok(proj.remove_composition(id))
    }

    pub fn get_composition(
        project: &Arc<RwLock<Project>>,
        id: Uuid,
    ) -> Result<Composition, LibraryError> {
        let proj = super::read_project(project)?;
        proj.compositions
            .iter()
            .find(|c| c.id == id)
            .cloned()
            .ok_or(LibraryError::project(format!(
                "Composition not found: {}",
                id
            )))
    }

    pub fn is_composition_used(project: &Arc<RwLock<Project>>, comp_id: Uuid) -> bool {
        if let Ok(proj) = super::read_project(project) {
            // Check all clips in the nodes registry
            for clip in proj.all_clips() {
                if clip.reference_id == Some(comp_id) {
                    return true;
                }
            }
        }
        false
    }
}
