use crate::audio::engine::AudioEngine;
use crate::cache::CacheManager;
use crate::error::LibraryError;
use crate::model::project::asset::Asset;
use crate::model::project::project::{Composition, Project};
use crate::model::project::property::PropertyValue;
use crate::model::project::{Track, TrackClip};
use crate::plugin::PluginManager;
use crate::service::audio_service::AudioService;
use crate::service::project_manager::ProjectManager;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub struct EditorService {
    project_manager: Arc<ProjectManager>,
    audio_service: Arc<AudioService>,
}

impl Clone for EditorService {
    fn clone(&self) -> Self {
        Self {
            project_manager: self.project_manager.clone(),
            audio_service: self.audio_service.clone(),
        }
    }
}

impl EditorService {
    pub fn new(
        project: Arc<RwLock<Project>>,
        plugin_manager: Arc<PluginManager>,
        cache_manager: Arc<CacheManager>,
    ) -> Self {
        let audio_engine = Arc::new(AudioEngine::new().expect("Failed to initialize Audio Engine"));

        let project_manager = Arc::new(ProjectManager::new(project.clone(), plugin_manager));
        // AudioService also needs shared access to Project for mixing
        let audio_service = Arc::new(AudioService::new(project, audio_engine, cache_manager));

        Self {
            project_manager,
            audio_service,
        }
    }

    pub fn get_project(&self) -> Arc<RwLock<Project>> {
        self.project_manager.get_project()
    }

    pub fn set_project(&self, project: crate::model::project::project::Project) {
        let _ = self.project_manager.set_project(project);
    }

    pub fn get_audio_service(&self) -> Arc<AudioService> {
        self.audio_service.clone()
    }

    pub fn get_cache_manager(&self) -> Arc<crate::cache::CacheManager> {
        self.audio_service.get_cache_manager()
    }

    pub fn get_plugin_manager(&self) -> Arc<PluginManager> {
        self.project_manager.get_plugin_manager()
    }

    // For compatibility with App that might access audio_engine directly
    // Ideally App shouldn't, but `ProjectService` exposed it as pub
    pub fn get_audio_engine(&self) -> Arc<AudioEngine> {
        self.audio_service.get_audio_engine()
    }

    // Proxy for backward compatibility if needed, though we can change call sites to get accessor
    pub fn audio_engine(&self) -> Arc<AudioEngine> {
        self.audio_service.get_audio_engine()
    }

    // --- Audio Operations ---

    pub fn reset_audio_pump(&self, time: f64) {
        self.audio_service.reset_audio_pump(time);
    }

    pub fn pump_audio(&self) {
        self.audio_service.pump_audio();
    }

    pub fn render_audio(&self, start_time: f64, duration: f64) -> Vec<f32> {
        self.audio_service.render_audio(start_time, duration)
    }

    // --- Project Operations ---

    pub fn load_project(&self, json_str: &str) -> Result<(), LibraryError> {
        let new_project = self.project_manager.load_project(json_str)?;

        // Hydrate Audio Cache (Orchestration logic)
        for asset in &new_project.assets {
            if asset.kind == crate::model::project::asset::AssetKind::Audio {
                self.audio_service
                    .trigger_audio_loading(asset.id, asset.path.clone());
            }
        }

        // Note: ProjectManager::load_project already updated the shared Arc<RwLock<Project>>
        // so AudioService sees the new data automatically because it holds the same Arc.

        Ok(())
    }

    pub fn create_new_project(&self) -> Result<Uuid, LibraryError> {
        let (new_comp_id, _) = self.project_manager.create_new_project()?;
        // No audio to hydrate for new project
        Ok(new_comp_id)
    }

    pub fn save_project(&self) -> Result<String, LibraryError> {
        self.project_manager.save_project()
    }

    pub fn import_file(&self, path: &str) -> Result<Uuid, LibraryError> {
        let asset_id = self.project_manager.import_file(path)?;

        // Orchestration: Check if Audio
        if let Ok(project) = self.project_manager.get_project().read() {
            if let Some(asset) = project.assets.iter().find(|a| a.id == asset_id) {
                if asset.kind == crate::model::project::asset::AssetKind::Audio {
                    let path_clone = asset.path.clone();
                    drop(project);
                    self.audio_service
                        .trigger_audio_loading(asset_id, path_clone);
                    return Ok(asset_id);
                }
            }
        }

        Ok(asset_id)
    }

    // --- Delegated CRUD ---

    pub fn add_asset(&self, asset: Asset) -> Result<Uuid, LibraryError> {
        self.project_manager.add_asset(asset)
    }

    pub fn is_asset_used(&self, asset_id: Uuid) -> bool {
        self.project_manager.is_asset_used(asset_id)
    }

    pub fn remove_asset(&self, asset_id: Uuid) -> Result<(), LibraryError> {
        self.project_manager.remove_asset(asset_id)
    }

    pub fn remove_asset_fully(&self, asset_id: Uuid) -> Result<(), LibraryError> {
        self.project_manager.remove_asset_fully(asset_id)
    }

    pub fn add_composition(
        &self,
        name: &str,
        width: u32,
        height: u32,
        fps: f64,
        duration: f64,
    ) -> Result<Uuid, LibraryError> {
        self.project_manager
            .add_composition(name, width, height, fps, duration)
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
        self.project_manager
            .update_composition(id, name, width, height, fps, duration)
    }

    pub fn get_composition(&self, id: Uuid) -> Result<Composition, LibraryError> {
        self.project_manager.get_composition(id)
    }

    pub fn is_composition_used(&self, comp_id: Uuid) -> bool {
        self.project_manager.is_composition_used(comp_id)
    }

    pub fn add_track(&self, composition_id: Uuid, track_name: &str) -> Result<Uuid, LibraryError> {
        self.project_manager.add_track(composition_id, track_name)
    }

    pub fn add_track_with_id(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        track_name: &str,
    ) -> Result<Uuid, LibraryError> {
        self.project_manager
            .add_track_with_id(composition_id, track_id, track_name)
    }

    pub fn get_track(&self, composition_id: Uuid, track_id: Uuid) -> Result<Track, LibraryError> {
        self.project_manager.get_track(composition_id, track_id)
    }

    pub fn remove_track(&self, composition_id: Uuid, track_id: Uuid) -> Result<(), LibraryError> {
        self.project_manager.remove_track(composition_id, track_id)
    }

    pub fn add_clip_to_track(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip: TrackClip,
        in_frame: u64,
        out_frame: u64,
    ) -> Result<Uuid, LibraryError> {
        self.project_manager
            .add_clip_to_track(composition_id, track_id, clip, in_frame, out_frame)
    }

    pub fn remove_clip_from_track(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .remove_clip_from_track(composition_id, track_id, clip_id)
    }

    pub fn update_clip_property(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        property_key: &str,
        value: PropertyValue,
    ) -> Result<(), LibraryError> {
        self.project_manager.update_clip_property(
            composition_id,
            track_id,
            clip_id,
            property_key,
            value,
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
        self.project_manager.move_clip_to_track(
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
        self.project_manager
            .add_effect_to_clip(composition_id, track_id, clip_id, effect_id)
    }

    pub fn update_track_clip_effects(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        effects: Vec<crate::model::project::EffectConfig>,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .update_track_clip_effects(composition_id, track_id, clip_id, effects)
    }

    pub fn evaluate_property_value(
        &self,
        property: &crate::model::project::property::Property,
        context: &crate::model::project::property::PropertyMap,
        time: f64,
    ) -> PropertyValue {
        self.project_manager
            .evaluate_property_value(property, context, time)
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
        self.project_manager.add_keyframe(
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
        self.project_manager.add_effect_keyframe(
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

    // Aliases & Sequences



    pub fn update_clip_time(
        &self,
        comp_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        in_frame: u64,
        out_frame: u64,
    ) -> Result<(), LibraryError> {
        self.update_clip_property(
            comp_id,
            track_id,
            clip_id,
            "in_frame",
            PropertyValue::Number(ordered_float::OrderedFloat(in_frame as f64)),
        )?;
        self.update_clip_property(
            comp_id,
            track_id,
            clip_id,
            "out_frame",
            PropertyValue::Number(ordered_float::OrderedFloat(out_frame as f64)),
        )?;
        Ok(())
    }

    pub fn update_clip_source_frames(
        &self,
        comp_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        frame: u64,
    ) -> Result<(), LibraryError> {
        self.update_clip_property(
            comp_id,
            track_id,
            clip_id,
            "source_begin_frame",
            PropertyValue::Number(ordered_float::OrderedFloat(frame as f64)),
        )
    }

    pub fn get_inspector_definitions(
        &self,
        comp_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
    ) -> Vec<crate::plugin::PropertyDefinition> {
        self.project_manager.get_inspector_definitions(comp_id, track_id, clip_id)
    }

    pub fn get_property_definitions(
        &self,
        comp_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
    ) -> Vec<crate::plugin::PropertyDefinition> {
        self.get_inspector_definitions(comp_id, track_id, clip_id)
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
        self.project_manager.update_property_or_keyframe(
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
        new_time: Option<f64>,
        new_value: Option<PropertyValue>,
        new_easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager.update_keyframe(
            composition_id,
            track_id,
            clip_id,
            property_key,
            index,
            new_time,
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
        new_time: Option<f64>,
        new_value: Option<PropertyValue>,
        new_easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager.update_effect_keyframe_by_index(
            composition_id,
            track_id,
            clip_id,
            effect_index,
            property_key,
            keyframe_index,
            new_time,
            new_value,
            new_easing,
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
        self.project_manager.remove_effect_keyframe_by_index(
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
        self.project_manager
            .remove_keyframe(composition_id, track_id, clip_id, property_key, index)
    }

    pub fn has_asset_with_path(&self, path: &str) -> bool {
        self.project_manager.has_asset_with_path(path)
    }

    pub fn remove_composition_fully(&self, comp_id: Uuid) -> Result<(), LibraryError> {
        self.project_manager.remove_composition_fully(comp_id)
    }

    // File I/O for Frontend
    pub fn load_project_from_path(&self, path: &std::path::Path) -> Result<(), LibraryError> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| LibraryError::Runtime(format!("Failed to read project file: {}", e)))?;
        self.load_project(&content)
    }
    pub fn update_track_clip_styles(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        styles: Vec<crate::model::project::style::StyleInstance>,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .update_track_clip_styles(composition_id, track_id, clip_id, styles)
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
        self.project_manager.update_track_clip_style_property(
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
        self.project_manager.add_style_keyframe(
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
        self.project_manager.update_effect_property_or_keyframe(
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



    pub fn update_style_keyframe_by_index(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        style_index: usize,
        property_key: &str,
        keyframe_index: usize,
        new_time: Option<f64>,
        new_value: Option<PropertyValue>,
        new_easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager.update_style_keyframe_by_index(
            composition_id,
            track_id,
            clip_id,
            style_index,
            property_key,
            keyframe_index,
            new_time,
            new_value,
            new_easing,
        )
    }

    pub fn update_style_property_or_keyframe(
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
        self.project_manager.update_style_property_or_keyframe(
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
        self.project_manager.remove_style_keyframe(
            composition_id,
            track_id,
            clip_id,
            style_index,
            property_key,
            keyframe_index,
        )
    }

    pub fn set_style_property_attribute(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
        style_index: usize,
        property_key: &str,
        attribute_key: &str,
        attribute_value: PropertyValue,
    ) -> Result<(), LibraryError> {
        self.project_manager.set_style_property_attribute(
            composition_id,
            track_id,
            clip_id,
            style_index,
            property_key,
            attribute_key,
            attribute_value,
        )
    }
}

