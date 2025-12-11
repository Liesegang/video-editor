use crate::error::LibraryError;
// use crate::model::project::entity::Entity; // Removed
use crate::model::project::asset::Asset;
use crate::model::project::project::{Composition, Project};
use crate::model::project::property::{Property, PropertyValue};
use crate::model::project::{Track, TrackClip, TrackClipKind};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::plugin::PluginManager;

pub struct ProjectService {
    project: Arc<RwLock<Project>>,
    plugin_manager: Arc<PluginManager>,
}

impl ProjectService {
    pub fn new(project: Arc<RwLock<Project>>, plugin_manager: Arc<PluginManager>) -> Self {
        ProjectService { 
            project, 
            plugin_manager 
        }
    }

    pub fn get_project(&self) -> Arc<RwLock<Project>> {
        Arc::clone(&self.project)
    }

    pub fn get_plugin_manager(&self) -> Arc<PluginManager> {
        Arc::clone(&self.plugin_manager)
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

    pub fn add_asset(&self, asset: Asset) -> Result<Uuid, LibraryError> {
        let mut project_write = self.project.write().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project write lock: {}", e))
        })?;
        let id = asset.id;
        project_write.assets.push(asset);
        Ok(id)
    }

    pub fn is_asset_used(&self, asset_id: Uuid) -> bool {
        let project_read = self.project.read().unwrap();
        for comp in &project_read.compositions {
            for track in &comp.tracks {
                for clip in &track.clips {
                    if clip.reference_id == Some(asset_id) {
                        return true;
                    }
                }
            }
        }
        false
    }

    pub fn remove_asset_fully(&self, asset_id: Uuid) -> Result<(), LibraryError> {
        let mut project_write = self.project.write().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project write lock: {}", e))
        })?;
        
        // Remove clips referencing the asset
        for comp in &mut project_write.compositions {
            for track in &mut comp.tracks {
                track.clips.retain(|clip| clip.reference_id != Some(asset_id));
            }
        }

        // Remove the asset itself
        project_write.assets.retain(|a| a.id != asset_id);
        
        Ok(())
    }

    fn remove_asset(&self, asset_id: Uuid) -> Result<(), LibraryError> {
        let mut project_write = self.project.write().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project write lock: {}", e))
        })?;
        project_write.assets.retain(|a| a.id != asset_id);
        Ok(())
    }

    pub fn import_file(&self, path: &str) -> Result<Uuid, LibraryError> {
        let path_buf = std::path::Path::new(path);
        let name = path_buf
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or("New Asset".to_string());
        
        let kind = self.plugin_manager.probe_asset_kind(path);
        
        // TODO: In the future, we can load metadata (duration, width, height) here using plugins
        let duration = None; 
        
        let mut asset = Asset::new(&name, path, kind);
        asset.duration = duration;
        
        self.add_asset(asset)
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
            .find(|c| c.id == id)
            .cloned()
            .ok_or(LibraryError::Project(format!("Composition not found: {}", id)))
    }

    pub fn is_composition_used(&self, comp_id: Uuid) -> bool {
        let project_read = self.project.read().unwrap();
        for comp in &project_read.compositions {
            // A composition can't contain itself directly (usually), but we check all comps
            // Ideally we check if *other* comps use this one.
            for track in &comp.tracks {
                for clip in &track.clips {
                    if clip.reference_id == Some(comp_id) {
                        return true;
                    }
                }
            }
        }
        false
    }

    pub fn remove_composition_fully(&self, comp_id: Uuid) -> Result<(), LibraryError> {
        let mut project_write = self.project.write().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project write lock: {}", e))
        })?;

        // Remove clips referencing the composition from ALL compositions
        for comp in &mut project_write.compositions {
            for track in &mut comp.tracks {
                track.clips.retain(|clip| clip.reference_id != Some(comp_id));
            }
        }

        // Remove the composition itself
        project_write
            .remove_composition(comp_id)
            .ok_or(LibraryError::Project(format!(
                "Failed to remove composition {}",
                comp_id
            )))?;

        Ok(())
    }

    fn remove_composition(&self, comp_id: Uuid) -> Result<(), LibraryError> {
        let mut project_write = self.project.write().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project write lock: {}", e))
        })?;
        project_write
            .remove_composition(comp_id)
            .ok_or(LibraryError::Project(format!(
                "Failed to remove composition {}",
                comp_id
            )))?;
        Ok(())
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

    pub fn remove_composition_old(&self, id: Uuid) -> Result<(), LibraryError> { // Renamed to avoid conflict
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
            let res = f(track);
            composition.rebuild_entity_cache();
            Ok(res)
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
        // Validation: Prevent circular references if adding a composition
        if clip.kind == TrackClipKind::Composition {
            if let Some(ref_id) = clip.reference_id {
                if !self.validate_recursion(ref_id, composition_id) {
                    return Err(LibraryError::Project(
                        "Cannot add composition: Circular reference detected".to_string(),
                    ));
                }
            }
        }

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

    fn validate_recursion(&self, child_id: Uuid, parent_id: Uuid) -> bool {
        // 1. Direct reflexive check
        if child_id == parent_id {
            return false;
        }

        // 2. Cycle check: Does 'child' (the comp being added) ALREADY contain 'parent'?
        // If so, adding child to parent creates Parent -> Child -> ... -> Parent (Cycle).
        if let Ok(project) = self.project.read() {
            // Find the child composition definition
            if let Some(child_comp) = project.compositions.iter().find(|c| c.id == child_id) {
                // BFS/DFS Traversal of child_comp's dependencies
                let mut stack = vec![child_comp];
                
                while let Some(current_comp) = stack.pop() {
                     for track in &current_comp.tracks {
                         for clip in &track.clips {
                             if clip.kind == TrackClipKind::Composition {
                                 if let Some(ref_id) = clip.reference_id {
                                     if ref_id == parent_id {
                                         // Found parent inside child's hierarchy -> Cycle!
                                         return false;
                                     }
                                     
                                     // Continue searching strictly deeper? 
                                     // Actually, we just need to traverse the graph of compositions.
                                     // We need to look up the comp definition for 'ref_id'.
                                     if let Some(next_comp) = project.compositions.iter().find(|c| c.id == ref_id) {
                                         // Prevent infinite loop in traversal if there's already a cycle elsewhere (safeguard)
                                         // But simple tree traversal is fine if we assume existing graph is DAG.
                                         // Just push to stack.
                                         // Use simple recursion check.
                                     }
                                     
                                     // Optimization: We actually need to traverse deeper.
                                     // But we can't easily push references to stack if we are iterating.
                                     // Let's implement a simple recursive helper or stack of IDs.
                                 }
                             }
                         }
                     }
                }
            }
        }
        
        // Re-implementing traversal cleanly using ID stack to avoid lifetime hell
        let project_read = match self.project.read() {
            Ok(p) => p,
            Err(_) => return false, // Lock failure
        };

        let mut stack = vec![child_id];
        // We should track visited to avoid infinite loops if graph is already cyclic (though it shouldn't be)
        let mut visited = std::collections::HashSet::new();

        while let Some(current_id) = stack.pop() {
            if !visited.insert(current_id) {
                continue;
            }

            if let Some(comp) = project_read.compositions.iter().find(|c| c.id == current_id) {
                for track in &comp.tracks {
                    for clip in &track.clips {
                        if clip.kind == TrackClipKind::Composition {
                             if let Some(ref_id) = clip.reference_id {
                                 if ref_id == parent_id {
                                     return false; // Found parent in child's descendants
                                 }
                                 stack.push(ref_id);
                             }
                        }
                    }
                }
            }
        }

        true
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
