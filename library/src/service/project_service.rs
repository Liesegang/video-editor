use crate::error::LibraryError;
// use crate::model::project::entity::Entity; // Removed
use crate::model::project::asset::Asset;
use crate::model::project::project::{Composition, Project};
use crate::model::project::property::{Property, PropertyValue};
use crate::model::project::{Track, TrackClip, TrackClipKind};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use crate::plugin::PluginManager;

use crate::audio::engine::AudioEngine;

use crate::cache::CacheManager;
use serde_json;

pub struct ProjectService {
    project: Arc<RwLock<Project>>,
    plugin_manager: Arc<PluginManager>,
    pub audio_engine: Arc<AudioEngine>,
    cache_manager: Arc<CacheManager>,
    next_write_sample: std::sync::atomic::AtomicU64,
}

impl Clone for ProjectService {
    fn clone(&self) -> Self {
        Self {
            project: self.project.clone(),
            plugin_manager: self.plugin_manager.clone(),
            audio_engine: self.audio_engine.clone(),
            cache_manager: self.cache_manager.clone(),
            next_write_sample: std::sync::atomic::AtomicU64::new(
                self.next_write_sample
                    .load(std::sync::atomic::Ordering::Relaxed),
            ),
        }
    }
}

impl ProjectService {
    pub fn new(
        project: Arc<RwLock<Project>>,
        plugin_manager: Arc<PluginManager>,
        cache_manager: Arc<CacheManager>,
    ) -> Self {
        // Initialize Audio Engine
        let audio_engine = Arc::new(AudioEngine::new().expect("Failed to initialize Audio Engine"));

        ProjectService {
            project,
            plugin_manager,
            audio_engine,
            cache_manager,
            next_write_sample: std::sync::atomic::AtomicU64::new(0),
        }
    }

    pub fn reset_audio_pump(&self, time: f64) {
        self.audio_engine.set_time(time);

        let sample_rate = self.audio_engine.get_sample_rate();
        let channels = self.audio_engine.get_channels();
        let sample_pos = (time * sample_rate as f64).round() as u64;

        // Zapping: Generate 50ms preview
        let preview_duration = 0.05;
        let frames = (preview_duration * sample_rate as f64) as usize;
        let scrub_samples = if let Ok(project) = self.project.read() {
            if let Some(comp) = project.compositions.first() {
                crate::audio::mixer::mix_samples(
                    &project.assets,
                    comp,
                    &self.cache_manager,
                    sample_pos,
                    frames,
                    sample_rate,
                    channels as u32,
                )
            } else {
                vec![0.0; frames * channels as usize]
            }
        } else {
            vec![0.0; frames * channels as usize]
        };

        // Push and advance
        self.audio_engine.push_samples(&scrub_samples);
        self.next_write_sample.store(
            sample_pos + frames as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    pub fn pump_audio(&self) {
        let available = self.audio_engine.available_slots();
        if available == 0 {
            return;
        }

        // Limit chunk size to avoid stalling the UI thread with massive mixing
        let chunk_size = available.min(4096); // ~85ms at 48kHz

        let sample_rate = self.audio_engine.get_sample_rate();
        let channels = self.audio_engine.get_channels();

        let start_sample = self
            .next_write_sample
            .load(std::sync::atomic::Ordering::Relaxed);

        // Safety check
        if chunk_size < (channels as usize) {
            return;
        }

        let frames_to_write = chunk_size / (channels as usize);

        // Use shared mixing logic
        let mix_buffer = if let Ok(project) = self.project.read() {
            if let Some(comp) = project.compositions.first() {
                crate::audio::mixer::mix_samples(
                    &project.assets,
                    comp,
                    &self.cache_manager,
                    start_sample,
                    frames_to_write,
                    sample_rate,
                    channels as u32,
                )
            } else {
                vec![0.0; frames_to_write * channels as usize]
            }
        } else {
            vec![0.0; frames_to_write * channels as usize]
        };

        // Push to Engine
        self.audio_engine.push_samples(&mix_buffer);

        // Advance cursor
        self.next_write_sample
            .fetch_add(frames_to_write as u64, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn get_project(&self) -> Arc<RwLock<Project>> {
        Arc::clone(&self.project)
    }

    pub fn get_plugin_manager(&self) -> Arc<PluginManager> {
        Arc::clone(&self.plugin_manager)
    }

    pub fn get_cache_manager(&self) -> Arc<CacheManager> {
        Arc::clone(&self.cache_manager)
    }

    pub fn set_project(&self, new_project: Project) {
        let mut project_write = self.project.write().unwrap();
        *project_write = new_project;
    }

    // --- Project Operations ---

    pub fn load_project(&self, json_str: &str) -> Result<(), LibraryError> {
        let new_project = Project::load(json_str)?;

        // Hydrate Audio Cache
        for asset in &new_project.assets {
            if asset.kind == crate::model::project::asset::AssetKind::Audio {
                self.trigger_audio_loading(asset.id, asset.path.clone());
            }
        }

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
                track
                    .clips
                    .retain(|clip| clip.reference_id != Some(asset_id));
            }
        }

        // Remove the asset itself
        project_write.assets.retain(|a| a.id != asset_id);

        Ok(())
    }

    #[allow(dead_code)]
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
        let mut duration = self.plugin_manager.get_duration(path);

        if kind == crate::model::project::asset::AssetKind::Audio {
            if let Ok(d) = crate::audio::loader::AudioLoader::get_duration(path) {
                duration = Some(d);
            }
        }
        let dimensions = self.plugin_manager.get_dimensions(path);
        let fps = self.plugin_manager.get_fps(path);

        let mut asset = Asset::new(&name, path, kind.clone());
        asset.duration = duration;
        asset.fps = fps;
        if let Some((w, h)) = dimensions {
            asset.width = Some(w);
            asset.height = Some(h);
        }

        let asset_id = asset.id;
        self.add_asset(asset)?;

        // Background load audio for pure Audio assets
        if kind == crate::model::project::asset::AssetKind::Audio {
            self.trigger_audio_loading(asset_id, path.to_string());
        }

        // Check for separate Audio stream in Video files
        if kind == crate::model::project::asset::AssetKind::Video {
            if crate::audio::loader::AudioLoader::has_audio(path) {
                // Create separate Audio asset
                let audio_name = format!("{} (Audio)", name);
                let mut audio_asset = Asset::new(
                    &audio_name,
                    path,
                    crate::model::project::asset::AssetKind::Audio,
                );
                audio_asset.duration = duration; // Assume sync with video for now

                let audio_id = audio_asset.id;
                self.add_asset(audio_asset)?;
                self.trigger_audio_loading(audio_id, path.to_string());
            }
        }

        Ok(asset_id)
    }

    fn trigger_audio_loading(&self, asset_id: Uuid, path: String) {
        let cache_manager = self.cache_manager.clone();
        let target_sample_rate = self.audio_engine.get_sample_rate();

        std::thread::spawn(move || {
            use crate::audio::loader::AudioLoader;
            // Log start?
            match AudioLoader::load_entire_file(&path, target_sample_rate) {
                Ok(data) => {
                    cache_manager.put_audio(asset_id, data);
                    log::info!("Loaded audio for asset {}", asset_id);
                }
                Err(e) => {
                    log::error!("Failed to load audio for asset {}: {}", asset_id, e);
                }
            }
        });
    }

    pub fn render_audio(&self, start_time: f64, duration: f64) -> Vec<f32> {
        let sample_rate = self.audio_engine.get_sample_rate();
        let channels = self.audio_engine.get_channels();

        // Calculate samples
        let start_sample = (start_time * sample_rate as f64).round() as u64;
        let frames = (duration * sample_rate as f64).round() as usize;

        if let Ok(project) = self.project.read() {
            if let Some(comp) = project.compositions.first() {
                crate::audio::mixer::mix_samples(
                    &project.assets,
                    comp,
                    &self.cache_manager,
                    start_sample,
                    frames,
                    sample_rate,
                    channels as u32,
                )
            } else {
                vec![0.0; frames * channels as usize]
            }
        } else {
            vec![0.0; frames * channels as usize]
        }
    }

    // mix_samples removed (logic moved to mixer.rs)

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
            .ok_or(LibraryError::Project(format!(
                "Composition not found: {}",
                id
            )))
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
                track
                    .clips
                    .retain(|clip| clip.reference_id != Some(comp_id));
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

    #[allow(dead_code)]
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

    pub fn remove_composition_old(&self, id: Uuid) -> Result<(), LibraryError> {
        // Renamed to avoid conflict
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
        in_frame: u64,   // Timeline start frame
        out_frame: u64,  // Timeline end frame
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
                                    if let Some(_next_comp) =
                                        project.compositions.iter().find(|c| c.id == ref_id)
                                    {
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

            if let Some(comp) = project_read
                .compositions
                .iter()
                .find(|c| c.id == current_id)
            {
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
                // Sync struct fields with property updates
                match key {
                    "in_frame" => {
                        if let PropertyValue::Number(n) = &value {
                            clip.in_frame = n.into_inner().round() as u64;
                        }
                    }
                    "out_frame" => {
                        if let PropertyValue::Number(n) = &value {
                            clip.out_frame = n.into_inner().round() as u64;
                        }
                    }
                    "source_begin_frame" => {
                        if let PropertyValue::Number(n) = &value {
                            clip.source_begin_frame = n.into_inner().round() as u64;
                        }
                    }
                    _ => {}
                }

                clip.properties
                    .set(key.to_string(), Property::constant(value));
                Ok(())
            } else {
                Err(LibraryError::Project(format!(
                    "Clip with ID {} not found",
                    clip_id
                )))
            }
        })?
    }

    pub fn add_keyframe(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.with_track_mut(composition_id, track_id, |track| {
            if let Some(clip) = track.clips.iter_mut().find(|e| e.id == clip_id) {
                // Get or create property
                if let Some(prop) = clip.properties.get_mut(property_key) {
                    use crate::model::project::property::{Keyframe, Property};
                    use ordered_float::OrderedFloat;

                    // Check logic: if currently "constant", convert to "keyframe"
                    if prop.evaluator == "constant" {
                        // Current value becomes a keyframe at time 0
                        let initial_val = prop
                            .properties
                            .get("value")
                            .cloned()
                            .unwrap_or(PropertyValue::Number(OrderedFloat(0.0)));
                        let kf0 = Keyframe {
                            time: OrderedFloat(0.0),
                            value: initial_val,
                            easing: crate::animation::EasingFunction::Linear,
                        };

                        // New keyframe
                        let kf_new = Keyframe {
                            time: OrderedFloat(time),
                            value: value.clone(),
                            easing: easing.unwrap_or(crate::animation::EasingFunction::Linear),
                        };

                        let keyframes = vec![kf0, kf_new];
                        // Replace property with new Keyframe property
                        *prop = Property::keyframe(keyframes);
                    } else if prop.evaluator == "keyframe" {
                        let mut current_keyframes = prop.keyframes();

                        // Check for collision to preserve easing
                        let mut preserved_easing = crate::animation::EasingFunction::Linear;
                        if let Some(idx) = current_keyframes
                            .iter()
                            .position(|k| (k.time.into_inner() - time).abs() < 0.001)
                        {
                            preserved_easing = current_keyframes[idx].easing.clone();
                            current_keyframes.remove(idx);
                        }

                        let final_easing = easing.unwrap_or(preserved_easing);

                        current_keyframes.push(Keyframe {
                            time: OrderedFloat(time),
                            value,
                            easing: final_easing,
                        });

                        current_keyframes.sort_by(|a, b| a.time.cmp(&b.time));

                        *prop = Property::keyframe(current_keyframes);
                    }
                    Ok(())
                } else {
                    Err(LibraryError::Project(format!(
                        "Property {} not found",
                        property_key
                    )))
                }
            } else {
                Err(LibraryError::Project(format!("Clip {} not found", clip_id)))
            }
        })?
    }

    pub fn remove_keyframe(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        property_key: &str,
        index: usize,
    ) -> Result<(), LibraryError> {
        self.with_track_mut(composition_id, track_id, |track| {
            if let Some(clip) = track.clips.iter_mut().find(|e| e.id == clip_id) {
                if let Some(prop) = clip.properties.get_mut(property_key) {
                    if prop.evaluator == "keyframe" {
                        use crate::model::project::property::Property;
                        let mut current_keyframes = prop.keyframes();
                        if index < current_keyframes.len() {
                            current_keyframes.remove(index);
                            *prop = Property::keyframe(current_keyframes);
                        }
                    }
                    Ok(())
                } else {
                    Err(LibraryError::Project(format!(
                        "Property {} not found",
                        property_key
                    )))
                }
            } else {
                Err(LibraryError::Project(format!("Clip {} not found", clip_id)))
            }
        })?
    }

    pub fn update_property_or_keyframe(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.with_track_mut(composition_id, track_id, |track| {
            if let Some(clip) = track.clips.iter_mut().find(|e| e.id == clip_id) {
                // Get or create property
                // We must handle property creation if it doesn't exist
                let (evaluator, _is_new) = if let Some(prop) = clip.properties.get(property_key) {
                    (prop.evaluator.clone(), false)
                } else {
                    ("constant".to_string(), true)
                };

                if evaluator == "keyframe" {
                    use crate::model::project::property::{Keyframe, Property};
                    use ordered_float::OrderedFloat;

                    // Safely get mutable prop - we know it exists because evaluator check passed (which required get)
                    // But we dropped reference to unpack evaluator.
                    if let Some(prop) = clip.properties.get_mut(property_key) {
                        let mut current_keyframes = prop.keyframes();

                        // Check for collision to preserve easing
                        let mut preserved_easing = crate::animation::EasingFunction::Linear;
                        if let Some(idx) = current_keyframes
                            .iter()
                            .position(|k| (k.time.into_inner() - time).abs() < 0.001)
                        {
                            preserved_easing = current_keyframes[idx].easing.clone();
                            current_keyframes.remove(idx);
                        }

                        let final_easing = easing.unwrap_or(preserved_easing);

                        current_keyframes.push(Keyframe {
                            time: OrderedFloat(time),
                            value,
                            easing: final_easing,
                        });

                        current_keyframes.sort_by(|a, b| a.time.cmp(&b.time));

                        *prop = Property::keyframe(current_keyframes);
                    }
                } else {
                    // Constant mode
                    use crate::model::project::property::Property;
                    // Simply overwrite or create as constant
                    clip.properties
                        .set(property_key.to_string(), Property::constant(value));
                }
                Ok(())
            } else {
                Err(LibraryError::Project(format!("Clip {} not found", clip_id)))
            }
        })?
    }

    pub fn update_keyframe(
        &mut self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        property_key: &str,
        keyframe_index: usize,
        new_time: Option<f64>,
        new_value: Option<PropertyValue>,
        new_easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        // use crate::animation::EasingFunction; // Removed unused import
        use crate::model::project::property::{Keyframe, PropertyValue};
        use ordered_float::OrderedFloat;

        let mut project = self.project.write().unwrap();
        let composition = project
            .compositions
            .iter_mut()
            .find(|c| c.id == composition_id)
            .ok_or(LibraryError::Project(format!(
                "Composition {} not found",
                composition_id
            )))?;
        let track = composition
            .tracks
            .iter_mut()
            .find(|t| t.id == track_id)
            .ok_or(LibraryError::Project(format!(
                "Track {} not found",
                track_id
            )))?;
        let clip = track
            .clips
            .iter_mut()
            .find(|c| c.id == clip_id)
            .ok_or(LibraryError::Project(format!("Clip {} not found", clip_id)))?;

        let property = clip
            .properties
            .get_mut(property_key)
            .ok_or(LibraryError::Project(format!(
                "Property {} not found",
                property_key
            )))?;

        if let Some(PropertyValue::Array(promoted_array)) = property.properties.get_mut("keyframes")
        {
            let mut keyframes: Vec<Keyframe> = promoted_array
                .iter()
                .filter_map(|v| serde_json::from_value(serde_json::Value::from(v)).ok())
                .collect();

            if let Some(kf) = keyframes.get_mut(keyframe_index) {
                if let Some(t) = new_time {
                    kf.time = OrderedFloat(t);
                }
                if let Some(val) = new_value {
                    kf.value = val;
                }
                if let Some(easing) = new_easing {
                    kf.easing = easing;
                }
            } else {
                return Err(LibraryError::Project(
                    "Keyframe index out of bounds".to_string(),
                ));
            }

            // Resort
            keyframes.sort_by(|a, b| a.time.cmp(&b.time));

            let new_array: Vec<PropertyValue> = keyframes
                .into_iter()
                .filter_map(|kf| serde_json::to_value(kf).ok())
                .map(PropertyValue::from)
                .collect();

            promoted_array.clear();
            promoted_array.extend(new_array);
        }

        Ok(())
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

    pub fn add_effect_to_clip(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        effect_id: &str,
    ) -> Result<(), LibraryError> {
        self.with_track_mut(composition_id, track_id, |track| {
            if let Some(clip) = track.clips.iter_mut().find(|e| e.id == clip_id) {
                // Initialize defaults
                let defs = self.plugin_manager.get_effect_properties(effect_id);
                let mut props = crate::model::project::property::PropertyMap::new();
                for def in defs {
                    props.set(
                        def.name,
                        crate::model::project::property::Property::constant(def.default_value),
                    );
                }

                let config = crate::model::project::EffectConfig {
                    id: Uuid::new_v4(),
                    effect_type: effect_id.to_string(),
                    properties: props,
                };
                clip.effects.push(config);
                Ok(())
            } else {
                Err(LibraryError::Project(format!("Clip {} not found", clip_id)))
            }
        })?
    }

    pub fn update_track_clip_effects(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        effects: Vec<crate::model::project::EffectConfig>,
    ) -> Result<(), LibraryError> {
        self.with_track_mut(composition_id, track_id, |track| {
            if let Some(clip) = track.clips.iter_mut().find(|e| e.id == clip_id) {
                clip.effects = effects;
                Ok(())
            } else {
                Err(LibraryError::Project(format!("Clip {} not found", clip_id)))
            }
        })?
    }

    pub fn remove_effect_from_clip(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        effect_index: usize,
    ) -> Result<(), LibraryError> {
        self.with_track_mut(composition_id, track_id, |track| {
            if let Some(clip) = track.clips.iter_mut().find(|e| e.id == clip_id) {
                if effect_index < clip.effects.len() {
                    clip.effects.remove(effect_index);
                    Ok(())
                } else {
                    Err(LibraryError::Project(
                        "Effect index out of bounds".to_string(),
                    ))
                }
            } else {
                Err(LibraryError::Project(format!("Clip {} not found", clip_id)))
            }
        })?
    }

    pub fn update_effect_property_or_keyframe(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        effect_index: usize,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.with_track_mut(composition_id, track_id, |track| {
            if let Some(clip) = track.clips.iter_mut().find(|e| e.id == clip_id) {
                if let Some(effect) = clip.effects.get_mut(effect_index) {
                    // Logic similar to update_property_or_keyframe
                    let (evaluator, _is_new) =
                        if let Some(prop) = effect.properties.get(property_key) {
                            (prop.evaluator.clone(), false)
                        } else {
                            ("constant".to_string(), true)
                        };

                    if evaluator == "keyframe" {
                        use crate::model::project::property::{Keyframe, Property};
                        use ordered_float::OrderedFloat;

                        if let Some(prop) = effect.properties.get_mut(property_key) {
                            let mut current_keyframes = prop.keyframes();

                            // Check for collision to preserve easing
                            let mut preserved_easing = crate::animation::EasingFunction::Linear;
                            if let Some(idx) = current_keyframes
                                .iter()
                                .position(|k| (k.time.into_inner() - time).abs() < 0.001)
                            {
                                preserved_easing = current_keyframes[idx].easing.clone();
                                current_keyframes.remove(idx);
                            }

                            let final_easing = easing.unwrap_or(preserved_easing);

                            current_keyframes.push(Keyframe {
                                time: OrderedFloat(time),
                                value,
                                easing: final_easing,
                            });
                            current_keyframes.sort_by(|a, b| a.time.cmp(&b.time));
                            *prop = Property::keyframe(current_keyframes);
                        }
                    } else {
                        use crate::model::project::property::Property;
                        effect
                            .properties
                            .set(property_key.to_string(), Property::constant(value));
                    }
                    Ok(())
                } else {
                    Err(LibraryError::Project(
                        "Effect index out of bounds".to_string(),
                    ))
                }
            } else {
                Err(LibraryError::Project(format!("Clip {} not found", clip_id)))
            }
        })?
    }

    pub fn update_effect_keyframe_by_index(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        effect_index: usize,
        property_key: &str,
        keyframe_index: usize,
        new_time: Option<f64>,
        new_value: Option<PropertyValue>,
        new_easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.with_track_mut(composition_id, track_id, |track| {
            if let Some(clip) = track.clips.iter_mut().find(|e| e.id == clip_id) {
                if let Some(effect) = clip.effects.get_mut(effect_index) {
                    if let Some(prop) = effect.properties.get_mut(property_key) {
                        let mut keyframes = prop.keyframes();
                        if let Some(kf) = keyframes.get_mut(keyframe_index) {
                            if let Some(t) = new_time {
                                kf.time = ordered_float::OrderedFloat(t);
                            }
                            if let Some(v) = new_value {
                                kf.value = v;
                            }
                            if let Some(e) = new_easing {
                                kf.easing = e;
                            }
                        }
                        // Re-sort after potential time change
                        keyframes.sort_by(|a, b| a.time.cmp(&b.time));
                        use crate::model::project::property::Property;
                        *prop = Property::keyframe(keyframes);
                        Ok(())
                    } else {
                        Err(LibraryError::Project(format!(
                            "Property {} not found",
                            property_key
                        )))
                    }
                } else {
                    Err(LibraryError::Project(
                        "Effect index out of bounds".to_string(),
                    ))
                }
            } else {
                Err(LibraryError::Project(format!("Clip {} not found", clip_id)))
            }
        })?
    }

    pub fn remove_effect_keyframe_by_index(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        effect_index: usize,
        property_key: &str,
        keyframe_index: usize,
    ) -> Result<(), LibraryError> {
        self.with_track_mut(composition_id, track_id, |track| {
            if let Some(clip) = track.clips.iter_mut().find(|e| e.id == clip_id) {
                if let Some(effect) = clip.effects.get_mut(effect_index) {
                    if let Some(prop) = effect.properties.get_mut(property_key) {
                        let mut keyframes = prop.keyframes();
                        if keyframe_index < keyframes.len() {
                            keyframes.remove(keyframe_index);
                            use crate::model::project::property::Property;
                            *prop = Property::keyframe(keyframes);
                            Ok(())
                        } else {
                            Err(LibraryError::Project(
                                "Keyframe index out of bounds".to_string(),
                            ))
                        }
                    } else {
                        Err(LibraryError::Project(format!(
                            "Property {} not found",
                            property_key
                        )))
                    }
                } else {
                    Err(LibraryError::Project(
                        "Effect index out of bounds".to_string(),
                    ))
                }
            } else {
                Err(LibraryError::Project(format!("Clip {} not found", clip_id)))
            }
        })?
    }
    pub fn add_effect_keyframe(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        effect_index: usize,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.with_track_mut(composition_id, track_id, |track| {
            if let Some(clip) = track.clips.iter_mut().find(|e| e.id == clip_id) {
                if let Some(effect) = clip.effects.get_mut(effect_index) {
                    if let Some(prop) = effect.properties.get_mut(property_key) {
                        use crate::model::project::property::{Keyframe, Property};
                        use ordered_float::OrderedFloat;

                        if prop.evaluator == "constant" {
                            let initial_val = prop
                                .properties
                                .get("value")
                                .cloned()
                                .unwrap_or(PropertyValue::Number(OrderedFloat(0.0)));

                            let kf0 = Keyframe {
                                time: OrderedFloat(0.0),
                                value: initial_val,
                                easing: crate::animation::EasingFunction::Linear,
                            };

                            let kf_new = Keyframe {
                                time: OrderedFloat(time),
                                value: value.clone(),
                                easing: easing.unwrap_or(crate::animation::EasingFunction::Linear),
                            };

                            let keyframes = vec![kf0, kf_new];
                            *prop = Property::keyframe(keyframes);
                        } else if prop.evaluator == "keyframe" {
                            let mut current_keyframes = prop.keyframes();

                            // Check for collision
                            if let Some(idx) = current_keyframes
                                .iter()
                                .position(|k| (k.time.into_inner() - time).abs() < 0.001)
                            {
                                current_keyframes.remove(idx);
                            }

                            current_keyframes.push(Keyframe {
                                time: OrderedFloat(time),
                                value: value.clone(),
                                easing: easing.unwrap_or(crate::animation::EasingFunction::Linear),
                            });

                            current_keyframes.sort_by(|a, b| a.time.cmp(&b.time));
                            *prop = Property::keyframe(current_keyframes);
                        }
                        Ok(())
                    } else {
                        Err(LibraryError::Project(format!(
                            "Property {} not found",
                            property_key
                        )))
                    }
                } else {
                    Err(LibraryError::Project(
                        "Effect index out of bounds".to_string(),
                    ))
                }
            } else {
                Err(LibraryError::Project(format!("Clip {} not found", clip_id)))
            }
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
            LibraryError::Runtime(
                "Unexpected error: Clip not found after position check".to_string(),
            )
        })?;

        // 2. Add clip to target track
        self.with_track_mut(composition_id, target_track_id, |track| {
            track.clips.push(moved_clip);
            Ok(())
        })?
    }

    pub fn get_property_definitions(
        &self,
        kind: crate::model::project::TrackClipKind,
    ) -> Vec<crate::plugin::PropertyDefinition> {
        let mut definitions = Vec::new();

        use crate::model::project::property::PropertyValue;
        use crate::plugin::{PropertyDefinition, PropertyUiType};
        use ordered_float::OrderedFloat;

        // 1. Default Transform Properties (Available for almost all visual clips)
        if kind != crate::model::project::TrackClipKind::Audio {
            let defaults = vec![
                PropertyDefinition {
                    name: "position_x".to_string(),
                    label: "Position X".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: -10000.0,
                        max: 10000.0,
                        step: 1.0,
                        suffix: "px".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(960.0)),
                    category: "Transform".to_string(),
                },
                PropertyDefinition {
                    name: "position_y".to_string(),
                    label: "Position Y".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: -10000.0,
                        max: 10000.0,
                        step: 1.0,
                        suffix: "px".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(540.0)),
                    category: "Transform".to_string(),
                },
                PropertyDefinition {
                    name: "scale_x".to_string(),
                    label: "Scale X".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: 0.0,
                        max: 1000.0,
                        step: 1.0,
                        suffix: "%".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(100.0)),
                    category: "Transform".to_string(),
                },
                PropertyDefinition {
                    name: "scale_y".to_string(),
                    label: "Scale Y".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: 0.0,
                        max: 1000.0,
                        step: 1.0,
                        suffix: "%".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(100.0)),
                    category: "Transform".to_string(),
                },
                PropertyDefinition {
                    name: "anchor_x".to_string(),
                    label: "Anchor X".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: -10000.0,
                        max: 10000.0,
                        step: 1.0,
                        suffix: "px".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(0.0)),
                    category: "Transform".to_string(),
                },
                PropertyDefinition {
                    name: "anchor_y".to_string(),
                    label: "Anchor Y".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: -10000.0,
                        max: 10000.0,
                        step: 1.0,
                        suffix: "px".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(0.0)),
                    category: "Transform".to_string(),
                },
                PropertyDefinition {
                    name: "rotation".to_string(),
                    label: "Rotation".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: -360.0,
                        max: 360.0,
                        step: 1.0,
                        suffix: "°".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(0.0)),
                    category: "Transform".to_string(),
                },
                PropertyDefinition {
                    name: "opacity".to_string(),
                    label: "Opacity".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: 0.0,
                        max: 100.0,
                        step: 1.0,
                        suffix: "%".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(100.0)),
                    category: "Transform".to_string(),
                },
            ];
            definitions.extend(defaults);
        }

        // 2a. Text Properties
        if kind == crate::model::project::TrackClipKind::Text {
            let text_defs = vec![
                PropertyDefinition {
                    name: "text".to_string(),
                    label: "Content".to_string(),
                    ui_type: PropertyUiType::Text,
                    default_value: PropertyValue::String("New Text".to_string()),
                    category: "Text".to_string(),
                },
                PropertyDefinition {
                    name: "font_family".to_string(), // Corrected name matching TrackClip::create_text
                    label: "Font".to_string(),
                    ui_type: PropertyUiType::Font, // New Font type
                    default_value: PropertyValue::String("Arial".to_string()),
                    category: "Text".to_string(),
                },
                PropertyDefinition {
                    name: "size".to_string(),
                    label: "Font Size".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: 1.0,
                        max: 1000.0,
                        step: 1.0,
                        suffix: "px".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(100.0)),
                    category: "Text".to_string(),
                },
                PropertyDefinition {
                    name: "styles".to_string(),
                    label: "Styles".to_string(),
                    ui_type: PropertyUiType::Styles,
                    default_value: {
                        let style = crate::model::frame::draw_type::DrawStyle::Fill {
                            color: crate::model::frame::color::Color {
                                r: 255,
                                g: 255,
                                b: 255,
                                a: 255,
                            },
                            offset: 0.0,
                        };
                        let config = crate::model::frame::entity::StyleConfig {
                            id: Uuid::new_v4(),
                            style,
                        };
                        let style_json = serde_json::to_value(config).unwrap();
                        PropertyValue::Array(vec![PropertyValue::from(style_json)])
                    },
                    category: "Styles".to_string(),
                },
            ];
            definitions.extend(text_defs);
        }

        if kind == crate::model::project::TrackClipKind::Shape {
            let shape_defs = vec![
                PropertyDefinition {
                    name: "width".to_string(),
                    label: "Width".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: 0.0,
                        max: 10000.0,
                        step: 1.0,
                        suffix: "px".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(100.0)),
                    category: "Shape".to_string(),
                },
                PropertyDefinition {
                    name: "height".to_string(),
                    label: "Height".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: 0.0,
                        max: 10000.0,
                        step: 1.0,
                        suffix: "px".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(100.0)),
                    category: "Shape".to_string(),
                },
                PropertyDefinition {
                    name: "styles".to_string(),
                    label: "Styles".to_string(),
                    ui_type: PropertyUiType::Styles,
                    default_value: {
        // Styles: Fill Red (#FF0000), Stroke White (#FFFFFF, width 2.0)
        let styles = vec![
            crate::model::frame::draw_type::DrawStyle::Fill {
                color: crate::model::frame::color::Color {
                    r: 255,
                    g: 0,
                    b: 0,
                    a: 255,
                },
                offset: 0.0,
            },
            crate::model::frame::draw_type::DrawStyle::Stroke {
                color: crate::model::frame::color::Color {
                    r: 255,
                    g: 255,
                    b: 255,
                    a: 255,
                },
                width: 5.0,
                cap: Default::default(),
                join: Default::default(),
                miter: 4.0,
                dash_array: Vec::new(),
                dash_offset: 0.0,
                offset: 0.0,
            },
        ];
        let style_configs: Vec<crate::model::frame::entity::StyleConfig> = styles
            .into_iter()
            .map(|style| crate::model::frame::entity::StyleConfig {
                id: Uuid::new_v4(),
                style,
            })
            .collect();

        let style_json_array: Vec<serde_json::Value> = style_configs
            .into_iter()
            .map(|config| serde_json::to_value(config).unwrap())
            .collect();

        PropertyValue::Array(
            style_json_array
                .into_iter()
                .map(PropertyValue::from)
                .collect(),
        )
                    },
                    category: "Styles".to_string(),
                },
            ];
            definitions.extend(shape_defs);
        }

        if kind == crate::model::project::TrackClipKind::SkSL {
            let shader_defs = vec![PropertyDefinition {
                name: "shader".to_string(),
                label: "Shader Code".to_string(),
                ui_type: PropertyUiType::MultilineText,
                default_value: PropertyValue::String("".to_string()),
                category: "Shader".to_string(),
            }];
            definitions.extend(shader_defs);
        }

        // 2. Plugin Properties
        let plugin_defs = self.plugin_manager.get_inspector_definitions(&kind);
        definitions.extend(plugin_defs);

        definitions
    }
}
