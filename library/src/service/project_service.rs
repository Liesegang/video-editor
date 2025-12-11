use crate::error::LibraryError;
// use crate::model::project::entity::Entity; // Removed
use crate::model::project::project::{Composition, Project};
use crate::model::project::property::{Property, PropertyValue};
use crate::model::project::{Track, TrackClip};
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

    pub fn add_clip_to_track(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip: TrackClip, // Pass a fully formed TrackClip object
        in_frame: u64,  // Timeline start frame
        out_frame: u64, // Timeline end frame
    ) -> Result<Uuid, LibraryError> {
        self.with_track_mut(composition_id, track_id, |track| {
            let id = clip.id;
            // Ensure the clip's timing matches the requested timing
            let mut final_clip = clip;
            final_clip.in_frame = in_frame;
            final_clip.out_frame = out_frame;

            track.clips.push(final_clip);
            Ok(id)
        })?
    }

    pub fn remove_clip_from_track(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
    ) -> Result<(), LibraryError> {
        self.with_track_mut(composition_id, track_id, |track| {
            if let Some(index) = track.clips.iter().position(|e| e.id == clip_id) {
                track.clips.remove(index);
                Ok(())
            } else {
                Err(LibraryError::Project(format!(
                    "Clip with ID {} not found in track {}",
                    clip_id, track_id
                )))
            }
        })?
    }

    pub fn update_clip_property(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        key: &str,
        value: PropertyValue,
    ) -> Result<(), LibraryError> {
        self.with_track_mut(composition_id, track_id, |track| {
            if let Some(clip) = track.clips.iter_mut().find(|e| e.id == clip_id) {
                clip.properties.set(key.to_string(), Property::constant(value));
                Ok(())
            } else {
                Err(LibraryError::Project(format!(
                    "Clip with ID {} not found",
                    clip_id
                )))
            }
        })?
    }

    pub fn update_clip_time(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        new_in_frame: u64,
        new_out_frame: u64,
    ) -> Result<(), LibraryError> {
        self.with_track_mut(composition_id, track_id, |track| {
            let track_clip = track
                .clips
                .iter_mut()
                .find(|e| e.id == clip_id)
                .ok_or_else(|| {
                    LibraryError::Project(format!(
                        "Clip with ID {} not found in Track {}",
                        clip_id, track_id
                    ))
                })?;

            track_clip.in_frame = new_in_frame;
            track_clip.out_frame = new_out_frame;
            Ok(())
        })?
    }

    pub fn update_clip_source_frames(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        new_source_begin_frame: u64,
        new_duration_frame: Option<u64>,
    ) -> Result<(), LibraryError> {
        self.with_track_mut(composition_id, track_id, |track| {
            let track_clip = track
                .clips
                .iter_mut()
                .find(|e| e.id == clip_id)
                .ok_or_else(|| {
                    LibraryError::Project(format!(
                        "Clip with ID {} not found in Track {}",
                        clip_id, track_id
                    ))
                })?;

            track_clip.source_begin_frame = new_source_begin_frame;
            track_clip.duration_frame = new_duration_frame;
            Ok(())
        })?
    }

    pub fn move_clip_to_track(
        &self,
        composition_id: Uuid,
        source_track_id: Uuid,
        target_track_id: Uuid,
        clip_id: Uuid,
    ) -> Result<(), LibraryError> {
        // 1. Remove clip from source track
        let mut clip_to_move = None;
        self.with_track_mut(composition_id, source_track_id, |track| {
            if let Some(pos) = track.clips.iter().position(|e| e.id == clip_id) {
                clip_to_move = Some(track.clips.remove(pos));
                Ok(())
            } else {
                 Err(LibraryError::Project(format!(
                    "Clip with ID {} not found in source Track {}",
                    clip_id, source_track_id
                )))
            }
        })??; // Double question mark because with_track_mut returns Result, and closure returns Result

        let moved_clip = clip_to_move.ok_or_else(|| {
             LibraryError::Runtime("Unexpected error: Clip not found after position check".to_string())
        })?;

        // 2. Add clip to target track
        self.with_track_mut(composition_id, target_track_id, |track| {
            track.clips.push(moved_clip);
            Ok(())
        })?
    }
}
