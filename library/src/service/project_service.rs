use crate::error::LibraryError;
use crate::model::project::entity::Entity;
use crate::model::project::project::{Composition, Project};
use crate::model::project::property::{Property, PropertyValue};
use crate::model::project::{Track, TrackEntity};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub struct ProjectService {
    project: Arc<RwLock<Project>>,
}

impl ProjectService {
    pub fn new(project: Arc<RwLock<Project>>) -> Self {
        ProjectService { project }
    }

    pub fn get_project(&self) -> Arc<RwLock<Project>> {
        Arc::clone(&self.project)
    }

    pub fn set_project(&self, new_project: Project) {
        let mut project_write = self.project.write().unwrap();
        *project_write = new_project;
    }

    // --- Project Operations ---

    pub fn load_project(&self, json_str: &str) -> Result<(), LibraryError> {
        let new_project = Project::load(json_str)?;
        let mut project_write = self.project.write().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project write lock: {}", e))
        })?;
        *project_write = new_project;
        Ok(())
    }

    pub fn save_project(&self) -> Result<String, LibraryError> {
        let project_read = self.project.read().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project read lock: {}", e))
        })?;
        Ok(project_read.save()?)
    }

    // --- Composition Operations ---

    pub fn add_composition(
        &self,
        name: &str,
        width: u64,
        height: u64,
        fps: f64,
        duration: f64,
    ) -> Result<Uuid, LibraryError> {
        let mut project_write = self.project.write().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project write lock: {}", e))
        })?;
        let composition = Composition::new(name, width, height, fps, duration);
        let id = composition.id;
        project_write.add_composition(composition);
        Ok(id)
    }

    pub fn get_composition(&self, id: Uuid) -> Result<Composition, LibraryError> {
        let project_read = self.project.read().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project read lock: {}", e))
        })?;
        project_read
            .compositions
            .iter()
            .find(|&c| c.id == id)
            .cloned()
            .ok_or_else(|| LibraryError::Project(format!("Composition with ID {} not found", id)))
    }

    // New closure-based method for mutable access to Composition
    pub fn with_composition_mut<F, R>(&self, id: Uuid, f: F) -> Result<R, LibraryError>
    where
        F: FnOnce(&mut Composition) -> R,
    {
        let mut project_write = self.project.write().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project write lock: {}", e))
        })?;
        let composition = project_write.get_composition_mut(id).ok_or_else(|| {
            LibraryError::Project(format!("Composition with ID {} not found", id))
        })?;
        Ok(f(composition))
    }

    pub fn remove_composition(&self, id: Uuid) -> Result<(), LibraryError> {
        let mut project_write = self.project.write().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project write lock: {}", e))
        })?;
        if project_write.remove_composition(id).is_some() {
            Ok(())
        } else {
            Err(LibraryError::Project(format!(
                "Composition with ID {} not found",
                id
            )))
        }
    }

    pub fn update_composition(
        &self,
        id: Uuid,
        name: &str,
        width: u64,
        height: u64,
        fps: f64,
        duration: f64,
    ) -> Result<(), LibraryError> {
        self.with_composition_mut(id, |composition| {
            composition.name = name.to_string();
            composition.width = width;
            composition.height = height;
            composition.fps = fps;
            composition.duration = duration;
        })
    }

    // --- Track Operations ---

    pub fn add_track(&self, composition_id: Uuid, track_name: &str) -> Result<Uuid, LibraryError> {
        self.with_composition_mut(composition_id, |composition| {
            let track = Track::new(track_name);
            let id = track.id;
            composition.add_track(track);
            id
        })
    }

    pub fn add_track_with_id(
        &self,
        composition_id: Uuid,
        track: Track,
    ) -> Result<Uuid, LibraryError> {
        let track_id = track.id;
        self.with_composition_mut(composition_id, |composition| {
            composition.add_track(track);
            track_id
        })
    }

    pub fn get_track(&self, composition_id: Uuid, track_id: Uuid) -> Result<Track, LibraryError> {
        let project_read = self.project.read().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project read lock: {}", e))
        })?;
        let composition = project_read
            .compositions
            .iter()
            .find(|&c| c.id == composition_id)
            .ok_or_else(|| {
                LibraryError::Project(format!("Composition with ID {} not found", composition_id))
            })?;

        composition
            .tracks
            .iter()
            .find(|&t| t.id == track_id)
            .cloned()
            .ok_or_else(|| {
                LibraryError::Project(format!(
                    "Track with ID {} not found in Composition {}",
                    track_id, composition_id
                ))
            })
    }

    // New closure-based method for mutable access to Track
    pub fn with_track_mut<F, R>(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        f: F,
    ) -> Result<R, LibraryError>
    where
        F: FnOnce(&mut Track) -> R,
    {
        self.with_composition_mut(composition_id, |composition| {
            let track = composition.get_track_mut(track_id).ok_or_else(|| {
                LibraryError::Project(format!(
                    "Track with ID {} not found in Composition {}",
                    track_id, composition_id
                ))
            })?;
            Ok(f(track))
        })?
    }

    pub fn remove_track(&self, composition_id: Uuid, track_id: Uuid) -> Result<(), LibraryError> {
        self.with_composition_mut(composition_id, |composition| {
            if composition.remove_track(track_id).is_some() {
                Ok(())
            } else {
                Err(LibraryError::Project(format!(
                    "Track with ID {} not found in Composition {}",
                    track_id, composition_id
                )))
            }
        })?
    }

    // --- Entity Operations ---

    pub fn add_entity_to_track(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        entity_type: &str,
        start_time: f64,
        end_time: f64,
    ) -> Result<Uuid, LibraryError> {
        self.with_track_mut(composition_id, track_id, |track| {
            let entity = Entity::new(entity_type);
            let id = entity.id;
            track.entities.push(TrackEntity::new(
                entity.id,
                entity.entity_type,
                start_time,
                end_time,
                entity.fps,
                entity.properties,
                entity.effects,
            ));
            Ok(id)
        })?
    }

    pub fn remove_entity_from_track(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        entity_id: Uuid,
    ) -> Result<(), LibraryError> {
        self.with_track_mut(composition_id, track_id, |track| {
            let initial_len = track.entities.len();
            track.entities.retain(|e| e.id != entity_id);
            if track.entities.len() < initial_len {
                Ok(())
            } else {
                Err(LibraryError::Project(format!(
                    "Entity with ID {} not found in Track {}",
                    entity_id, track_id
                )))
            }
        })?
    }

    pub fn update_entity_property(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        entity_id: Uuid,
        key: &str,
        value: PropertyValue,
    ) -> Result<(), LibraryError> {
        self.with_track_mut(composition_id, track_id, |track| {
            let track_entity = track
                .entities
                .iter_mut()
                .find(|e| e.id == entity_id)
                .ok_or_else(|| {
                    LibraryError::Project(format!(
                        "Entity with ID {} not found in Track {}",
                        entity_id, track_id
                    ))
                })?;

            track_entity
                .properties
                .set(key.to_string(), Property::constant(value));
            Ok(())
        })?
    }

    pub fn update_entity_time(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        entity_id: Uuid,
        new_start_time: f64,
        new_end_time: f64,
    ) -> Result<(), LibraryError> {
        self.with_track_mut(composition_id, track_id, |track| {
            let track_entity = track
                .entities
                .iter_mut()
                .find(|e| e.id == entity_id)
                .ok_or_else(|| {
                    LibraryError::Project(format!(
                        "Entity with ID {} not found in Track {}",
                        entity_id, track_id
                    ))
                })?;

            track_entity.start_time = new_start_time;
            track_entity.end_time = new_end_time;
            Ok(())
        })?
    }
}
