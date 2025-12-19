use crate::error::LibraryError;
use crate::core::model::asset::Asset;
use crate::core::model::project::{Composition, Project};
use crate::core::model::property::PropertyValue;
use crate::core::model::{Track, TrackClip};
use crate::extensions::manager::PluginManager;
use crate::app::handlers;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub struct ProjectManager {
    project: Arc<RwLock<Project>>,
    plugin_manager: Arc<PluginManager>,
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
        let mut project_write = self.project.write().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project write lock: {}", e))
        })?;
        *project_write = new_project;
        Ok(())
    }

    pub fn load_project(&self, json_str: &str) -> Result<Project, LibraryError> {
        let new_project = Project::load(json_str)?;
        let mut project_write = self.project.write().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project write lock: {}", e))
        })?;
        *project_write = new_project.clone();
        Ok(new_project)
    }

    pub fn create_new_project(&self) -> Result<(Uuid, Project), LibraryError> {
        let mut new_project = Project::new("New Project");
        let default_comp = Composition::new("Main Composition", 1920, 1080, 30.0, 60.0);
        let new_comp_id = default_comp.id;
        new_project.add_composition(default_comp);

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

    // --- Delegated Handlers ---

    pub fn add_asset(&self, asset: Asset) -> Result<Uuid, LibraryError> {
        handlers::asset_handler::AssetHandler::add_asset(&self.project, asset)
    }

    pub fn is_asset_used(&self, asset_id: Uuid) -> bool {
        handlers::asset_handler::AssetHandler::is_asset_used(&self.project, asset_id)
    }

    pub fn remove_asset(&self, asset_id: Uuid) -> Result<(), LibraryError> {
        handlers::asset_handler::AssetHandler::remove_asset(&self.project, asset_id)
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

    pub fn remove_composition_fully(&self, comp_id: Uuid) -> Result<(), LibraryError> {
        let mut project_write = self.project.write().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project write lock: {}", e))
        })?;

        // Remove clips referencing the composition (Nested Comps)
        for c in &mut project_write.compositions {
            // Skip self (though we remove it later)
            if c.id == comp_id {
                continue;
            }

            for track in &mut c.tracks {
                track
                    .clips
                    .retain(|clip| clip.reference_id != Some(comp_id));
            }
        }

        // Remove the composition itself
        project_write.compositions.retain(|c| c.id != comp_id);
        Ok(())
    }

    pub fn import_file(&self, path: &str) -> Result<Uuid, LibraryError> {
        let path_obj = std::path::Path::new(path);
        let name = path_obj
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        // 1. Get Metadata (Single call)
        let (mut kind, duration, fps, width, height) =
            if let Some(meta) = self.plugin_manager.get_metadata(path) {
                (meta.kind, meta.duration, meta.fps, meta.width, meta.height)
            } else {
                (
                    crate::core::model::asset::AssetKind::Other,
                    None,
                    None,
                    None,
                    None,
                )
            };

        // 2. Fallback for Kind if Unknown
        if kind == crate::core::model::asset::AssetKind::Other {
            // Fallback to extension if plugin didn't detect it
            let ext = path_obj
                .extension()
                .unwrap_or_default()
                .to_string_lossy()
                .to_lowercase();
            kind = match ext.as_str() {
                "mp4" | "mov" | "avi" | "mkv" | "webm" => {
                    crate::core::model::asset::AssetKind::Video
                }
                "png" | "jpg" | "jpeg" | "bmp" | "webp" => {
                    crate::core::model::asset::AssetKind::Image
                }
                "mp3" | "wav" | "ogg" | "aac" | "flac" => {
                    crate::core::model::asset::AssetKind::Audio
                }
                "obj" | "gltf" | "glb" => crate::core::model::asset::AssetKind::Model3D,
                _ => crate::core::model::asset::AssetKind::Other,
            };
        }

        // 3. Create Asset
        let mut asset = crate::core::model::asset::Asset::new(&name, path, kind);
        asset.duration = duration;
        asset.fps = fps;
        asset.width = match width {
            Some(w) => Some(w),
            None => None,
        };
        asset.height = match height {
            Some(h) => Some(h),
            None => None,
        };

        // 4. Clean up path (optional, maybe check absolute vs relative)
        // For now, keep as is.
        self.add_asset(asset)
    }

    pub fn add_composition(
        &self,
        name: &str,
        width: u32,
        height: u32,
        fps: f64,
        duration: f64,
    ) -> Result<Uuid, LibraryError> {
        handlers::composition_handler::CompositionHandler::add_composition(
            &self.project,
            name,
            width.into(),
            height.into(),
            fps,
            duration,
        )
    }

    pub fn get_composition(&self, id: Uuid) -> Result<Composition, LibraryError> {
        handlers::composition_handler::CompositionHandler::get_composition(&self.project, id)
    }

    pub fn update_composition(
        &self,
        id: Uuid,
        name: &str,
        width: u32,
        height: u32,
        fps: f64,
        duration: f64,
    ) -> Result<(), LibraryError> {
        handlers::composition_handler::CompositionHandler::update_composition(
            &self.project,
            id,
            name,
            width,
            height,
            fps,
            duration,
        )
    }

    pub fn is_composition_used(&self, comp_id: Uuid) -> bool {
        handlers::composition_handler::CompositionHandler::is_composition_used(
            &self.project,
            comp_id,
        )
    }

    pub fn add_track(&self, composition_id: Uuid, track_name: &str) -> Result<Uuid, LibraryError> {
        handlers::track_handler::TrackHandler::add_track(&self.project, composition_id, track_name)
    }

    pub fn add_track_with_id(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        track_name: &str,
    ) -> Result<Uuid, LibraryError> {
        let mut track = Track::new(track_name);
        track.id = track_id;
        handlers::track_handler::TrackHandler::add_track_with_id(
            &self.project,
            composition_id,
            track,
        )
    }

    // Add closure-based accessors (omitted for brevity, can restore if needed)
    // Actually, ProjectService had `mutate_track` etc. which are useful helpers.
    // I will include get_track and remove_track first.

    pub fn get_track(&self, composition_id: Uuid, track_id: Uuid) -> Result<Track, LibraryError> {
        handlers::track_handler::TrackHandler::get_track(&self.project, composition_id, track_id)
    }

    pub fn remove_track(&self, composition_id: Uuid, track_id: Uuid) -> Result<(), LibraryError> {
        handlers::track_handler::TrackHandler::remove_track(&self.project, composition_id, track_id)
    }

    pub fn add_clip_to_track(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip: TrackClip,
        in_frame: u64,
        out_frame: u64,
    ) -> Result<Uuid, LibraryError> {
        handlers::clip_handler::ClipHandler::add_clip_to_track(
            &self.project,
            composition_id,
            track_id,
            clip,
            in_frame,  // Pass u64
            out_frame, // Pass u64
        )
    }

    pub fn remove_clip_from_track(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
    ) -> Result<(), LibraryError> {
        handlers::clip_handler::ClipHandler::remove_clip_from_track(
            &self.project,
            composition_id,
            track_id,
            clip_id,
        )
    }

    pub fn update_clip_property(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        property_key: &str,
        value: PropertyValue,
    ) -> Result<(), LibraryError> {
        handlers::clip_handler::ClipHandler::update_clip_property(
            &self.project,
            composition_id,
            track_id,
            clip_id,
            property_key,
            value,
        )
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
        handlers::clip_handler::ClipHandler::update_property_or_keyframe(
            &self.project,
            composition_id,
            track_id,
            clip_id,
            property_key,
            time,
            value,
            easing,
        )
    }

    pub fn update_keyframe(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        property_key: &str,
        index: usize,
        time: Option<f64>,
        new_value: Option<PropertyValue>,
        new_easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        handlers::keyframe_handler::KeyframeHandler::update_keyframe(
            &self.project,
            composition_id,
            track_id,
            clip_id,
            property_key,
            index,
            time,
            new_value,
            new_easing,
        )
    }

    pub fn update_effect_keyframe_by_index(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        effect_index: usize,
        property_key: &str,
        keyframe_index: usize,
        time: Option<f64>,
        value: Option<PropertyValue>,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        handlers::keyframe_handler::KeyframeHandler::update_effect_keyframe_by_index(
            &self.project,
            composition_id,
            track_id,
            clip_id,
            effect_index,
            property_key,
            keyframe_index,
            time,
            value,
            easing,
        )
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
        handlers::keyframe_handler::KeyframeHandler::remove_effect_keyframe_by_index(
            &self.project,
            composition_id,
            track_id,
            clip_id,
            effect_index,
            property_key,
            keyframe_index,
        )
    }

    pub fn remove_keyframe(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        property_key: &str,
        index: usize,
    ) -> Result<(), LibraryError> {
        handlers::clip_handler::ClipHandler::remove_keyframe(
            &self.project,
            composition_id,
            track_id,
            clip_id,
            property_key,
            index,
        )
    }

    pub fn has_asset_with_path(&self, path: &str) -> bool {
        if let Ok(project) = self.project.read() {
            let path_norm = std::path::Path::new(path).to_string_lossy().to_string();
            project.assets.iter().any(|asset| {
                let asset_norm = std::path::Path::new(&asset.path)
                    .to_string_lossy()
                    .to_string();
                asset_norm == path_norm
            })
        } else {
            false
        }
    }

    // Copied metadata logic from ProjectService
    pub fn evaluate_property_value(
        &self,
        property: &crate::core::model::property::Property,
        context: &crate::core::model::property::PropertyMap,
        time: f64,
    ) -> PropertyValue {
        let eval_ctx = crate::extensions::traits::EvaluationContext {
            property_map: context,
        };
        self.plugin_manager
            .get_property_evaluators()
            .evaluate(property, time, &eval_ctx)
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
        handlers::keyframe_handler::KeyframeHandler::add_keyframe(
            &self.project,
            composition_id,
            track_id,
            clip_id,
            property_key,
            time,
            value,
            easing,
        )
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
        handlers::keyframe_handler::KeyframeHandler::add_effect_keyframe(
            &self.project,
            composition_id,
            track_id,
            clip_id,
            effect_index,
            property_key,
            time,
            value,
            easing,
        )
    }

    pub fn move_clip_to_track(
        &self,
        composition_id: Uuid,
        source_track_id: Uuid,
        clip_id: Uuid,
        target_track_id: Uuid,
        new_in_frame: u64,
    ) -> Result<(), LibraryError> {
        handlers::clip_handler::ClipHandler::move_clip_to_track(
            &self.project,
            composition_id,
            source_track_id,
            clip_id,
            target_track_id,
            new_in_frame,
        )
    }

    pub fn add_effect_to_clip(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        effect_id: &str,
    ) -> Result<(), LibraryError> {
        let effect_config = self
            .plugin_manager
            .get_default_effect_config(effect_id)
            .ok_or_else(|| {
                LibraryError::Project(format!(
                    "Default config for effect '{}' not found",
                    effect_id
                ))
            })?;

        handlers::clip_handler::ClipHandler::add_effect(
            &self.project,
            composition_id,
            track_id,
            clip_id,
            effect_config,
        )
    }

    pub fn update_track_clip_effects(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        effects: Vec<crate::core::model::EffectConfig>,
    ) -> Result<(), LibraryError> {
        handlers::clip_handler::ClipHandler::update_effects(
            &self.project,
            composition_id,
            track_id,
            clip_id,
            effects,
        )
    }

    pub fn update_track_clip_styles(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        styles: Vec<crate::core::model::style::StyleInstance>,
    ) -> Result<(), LibraryError> {
        handlers::clip_handler::ClipHandler::update_styles(
            &self.project,
            composition_id,
            track_id,
            clip_id,
            styles,
        )
    }

    pub fn update_track_clip_style_property(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        style_index: usize,
        property_key: &str,
        value: PropertyValue,
    ) -> Result<(), LibraryError> {
        handlers::clip_handler::ClipHandler::update_style_property(
            &self.project,
            composition_id,
            track_id,
            clip_id,
            style_index,
            property_key,
            value,
        )
    }

    pub fn add_style_keyframe(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        style_index: usize,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        handlers::keyframe_handler::KeyframeHandler::add_style_keyframe(
            &self.project,
            composition_id,
            track_id,
            clip_id,
            style_index,
            property_key,
            time,
            value,
            easing,
        )
    }

    pub fn remove_style_keyframe(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        style_index: usize,
        property_key: &str,
        keyframe_index: usize,
    ) -> Result<(), LibraryError> {
        handlers::keyframe_handler::KeyframeHandler::remove_style_keyframe(
            &self.project,
            composition_id,
            track_id,
            clip_id,
            style_index,
            property_key,
            keyframe_index,
        )
    }

    pub fn update_style_keyframe_by_index(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        style_index: usize,
        property_key: &str,
        keyframe_index: usize,
        time: Option<f64>,
        value: Option<PropertyValue>,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        handlers::keyframe_handler::KeyframeHandler::update_style_keyframe_by_index(
            &self.project,
            composition_id,
            track_id,
            clip_id,
            style_index,
            property_key,
            keyframe_index,
            time,
            value,
            easing,
        )
    }

    pub fn get_inspector_definitions(
        &self,
        comp_id: uuid::Uuid,
        track_id: uuid::Uuid,
        clip_id: uuid::Uuid,
    ) -> Vec<crate::extensions::traits::PropertyDefinition> {
        let project = self.project.read().unwrap();
        
        let (clip, canvas_width, canvas_height) = if let Some(comp) = project.compositions.iter().find(|c| c.id == comp_id) {
            if let Some(track) = comp.tracks.iter().find(|t| t.id == track_id) {
                if let Some(clip) = track.clips.iter().find(|c| c.id == clip_id) {
                     (clip.clone(), comp.width, comp.height)
                } else {
                    return Vec::new();
                }
            } else {
                return Vec::new();
            }
        } else {
            return Vec::new();
        };
        
        // Resolve clip dimensions
        let (clip_width, clip_height): (u64, u64) = match clip.kind {
            crate::core::model::TrackClipKind::Video | crate::core::model::TrackClipKind::Image => {
                if let Some(asset_id) = clip.reference_id {
                    if let Some(asset) = project.assets.iter().find(|a| a.id == asset_id) {
                        (asset.width.unwrap_or(100) as u64, asset.height.unwrap_or(100) as u64)
                    } else {
                         (100, 100)
                    }
                } else {
                    (100, 100)
                }
            },
            crate::core::model::TrackClipKind::Shape => {
                // Try to get from properties, otherwise default 100
                let w = clip.properties.get_f64("width").unwrap_or(100.0) as u64;
                let h = clip.properties.get_f64("height").unwrap_or(100.0) as u64;
                (w, h)
            },
            crate::core::model::TrackClipKind::Text => {
                 // Text size is hard to know without measuring. 
                 // For default anchor purposes, maybe use 0 or some heuristic?
                 // Or we could fallback to the "measured" size if we had it stored?
                 // Current text creation sets anchor based on measurement at creation.
                 // Let's assume 0,0 for now or maybe 100,100?
                 // If default anchor is center, and we define center as w/2, h/2...
                 // If we pass 0,0, anchor default is 0,0.
                 (0, 0) 
            },
            _ => (100, 100)
        };
        
        let mut definitions = clip.default_property_definitions(canvas_width as u32, canvas_height as u32, clip_width as u32, clip_height as u32);

        let plugin_defs = self.plugin_manager.get_inspector_definitions(&clip.kind);
        definitions.extend(plugin_defs);

        definitions
    }
}
