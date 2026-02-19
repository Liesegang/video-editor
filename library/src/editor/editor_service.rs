use crate::core::audio::engine::AudioEngine;
use crate::core::cache::CacheManager;
use crate::editor::audio_service::AudioService;
use crate::editor::project_service::ProjectManager;
use crate::error::LibraryError;
use crate::model::project::asset::Asset;
use crate::model::project::project::{Composition, Project};
use crate::model::project::property::PropertyValue;
use crate::model::project::{TrackClip, TrackData};
use crate::plugin::PluginManager;
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
    pub fn update_effector_property_or_keyframe(
        &self,
        clip_id: Uuid,
        effector_index: usize,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager.update_effector_property_or_keyframe(
            clip_id,
            effector_index,
            property_key,
            time,
            value,
            easing,
        )
    }

    pub fn update_decorator_property_or_keyframe(
        &self,
        clip_id: Uuid,
        decorator_index: usize,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager.update_decorator_property_or_keyframe(
            clip_id,
            decorator_index,
            property_key,
            time,
            value,
            easing,
        )
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

    /// Access the project immutably via a closure.
    /// Prefer this over `get_project()` to avoid exposing the lock.
    pub fn with_project<R>(&self, f: impl FnOnce(&Project) -> R) -> R {
        let project = self.project_manager.get_project();
        let guard = project.read().expect("Failed to acquire project read lock");
        f(&guard)
    }

    /// Access the project mutably via a closure.
    /// Prefer this over `get_project()` to avoid exposing the lock.
    pub fn with_project_mut<R>(&self, f: impl FnOnce(&mut Project) -> R) -> R {
        let project = self.project_manager.get_project();
        let mut guard = project
            .write()
            .expect("Failed to acquire project write lock");
        f(&mut guard)
    }

    #[deprecated(note = "Use with_project() or with_project_mut() instead")]
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

    pub fn import_file(&self, path: &str) -> Result<Vec<Uuid>, LibraryError> {
        let asset_ids = self.project_manager.import_file(path)?;

        // Orchestration: Check if Audio
        if let Ok(project) = self.project_manager.get_project().read() {
            for &asset_id in &asset_ids {
                if let Some(asset) = project.assets.iter().find(|a| a.id == asset_id) {
                    if asset.kind == crate::model::project::asset::AssetKind::Audio {
                        let path_clone = asset.path.clone();
                        // self.audio_service needs to trigger loading, but we are holding project read lock
                        // trigger_audio_loading doesn't seem to lock project?
                        // Actually it might.
                        // Ideally we collect need-to-load paths and do it after drop(project).
                        self.audio_service
                            .trigger_audio_loading(asset_id, path_clone);
                    }
                }
            }
        }

        Ok(asset_ids)
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

    // --- Clip Factory Methods ---

    pub fn create_audio_clip(
        &self,
        reference_id: Option<Uuid>,
        file_path: &str,
        in_frame: u64,
        out_frame: u64,
        source_begin_frame: i64,
        duration_frame: u64,
        fps: f64,
    ) -> TrackClip {
        self.project_manager.create_audio_clip(
            reference_id,
            file_path,
            in_frame,
            out_frame,
            source_begin_frame,
            duration_frame,
            fps,
        )
    }

    pub fn create_video_clip(
        &self,
        reference_id: Option<Uuid>,
        file_path: &str,
        in_frame: u64,
        out_frame: u64,
        source_begin_frame: i64,
        duration_frame: u64,
        fps: f64,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Result<TrackClip, LibraryError> {
        self.project_manager.create_video_clip(
            reference_id,
            file_path,
            in_frame,
            out_frame,
            source_begin_frame,
            duration_frame,
            fps,
            canvas_width,
            canvas_height,
        )
    }

    pub fn create_image_clip(
        &self,
        reference_id: Option<Uuid>,
        file_path: &str,
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
        fps: f64,
    ) -> Result<TrackClip, LibraryError> {
        self.project_manager.create_image_clip(
            reference_id,
            file_path,
            in_frame,
            out_frame,
            canvas_width,
            canvas_height,
            fps,
        )
    }

    pub fn create_text_clip(
        &self,
        text: &str,
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
        fps: f64,
    ) -> Result<TrackClip, LibraryError> {
        self.project_manager.create_text_clip(
            text,
            in_frame,
            out_frame,
            canvas_width,
            canvas_height,
            fps,
        )
    }

    pub fn create_shape_clip(
        &self,
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
        fps: f64,
    ) -> Result<TrackClip, LibraryError> {
        self.project_manager.create_shape_clip(
            in_frame,
            out_frame,
            canvas_width,
            canvas_height,
            fps,
        )
    }

    pub fn create_sksl_clip(
        &self,
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
        fps: f64,
    ) -> Result<TrackClip, LibraryError> {
        self.project_manager
            .create_sksl_clip(in_frame, out_frame, canvas_width, canvas_height, fps)
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

    pub fn get_track(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
    ) -> Result<TrackData, LibraryError> {
        self.project_manager.get_track(composition_id, track_id)
    }

    pub fn remove_track(&self, composition_id: Uuid, track_id: Uuid) -> Result<(), LibraryError> {
        self.project_manager.remove_track(composition_id, track_id)
    }

    pub fn add_sub_track(
        &self,
        composition_id: Uuid,
        parent_track_id: Uuid,
        track_name: &str,
    ) -> Result<Uuid, LibraryError> {
        self.project_manager
            .add_sub_track(composition_id, parent_track_id, track_name)
    }

    pub fn rename_track(&self, track_id: Uuid, new_name: &str) -> Result<(), LibraryError> {
        self.project_manager.rename_track(track_id, new_name)
    }

    pub fn add_clip_to_track(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        clip: TrackClip,
        in_frame: u64,
        out_frame: u64,
        insert_index: Option<usize>,
    ) -> Result<Uuid, LibraryError> {
        self.project_manager.add_clip_to_track(
            composition_id,
            track_id,
            clip,
            in_frame,
            out_frame,
            insert_index,
        )
    }

    pub fn remove_clip_from_track(
        &self,
        track_id: Uuid,
        clip_id: Uuid,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .remove_clip_from_track(track_id, clip_id)
    }

    pub fn update_clip_property(
        &self,
        clip_id: Uuid,
        property_key: &str,
        value: PropertyValue,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .update_track_clip_property(clip_id, property_key, value)
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

    pub fn move_clip_to_track_at_index(
        &self,
        composition_id: Uuid,
        source_track_id: Uuid,
        clip_id: Uuid,
        target_track_id: Uuid,
        new_in_frame: u64,
        target_index: Option<usize>,
    ) -> Result<(), LibraryError> {
        self.project_manager.move_clip_to_track_at_index(
            composition_id,
            source_track_id,
            clip_id,
            target_track_id,
            new_in_frame,
            target_index,
        )
    }

    pub fn add_effect_to_clip(&self, clip_id: Uuid, effect_id: &str) -> Result<(), LibraryError> {
        self.project_manager.add_effect_to_clip(clip_id, effect_id)
    }

    pub fn update_track_clip_effects(
        &self,
        clip_id: Uuid,
        effects: Vec<crate::model::project::EffectConfig>,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .update_track_clip_effects(clip_id, effects)
    }

    pub fn evaluate_property_value(
        &self,
        property: &crate::model::project::property::Property,
        context: &crate::model::project::property::PropertyMap,
        time: f64,
        fps: f64,
    ) -> PropertyValue {
        self.project_manager
            .evaluate_property_value(property, context, time, fps)
    }

    pub fn add_keyframe(
        &self,
        clip_id: Uuid,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .add_keyframe(clip_id, property_key, time, value, easing)
    }

    pub fn add_effect_keyframe(
        &self,
        clip_id: Uuid,
        effect_index: usize,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager.add_effect_keyframe(
            clip_id,
            effect_index,
            property_key,
            time,
            value,
            easing,
        )
    }

    pub fn add_effector_keyframe(
        &self,
        clip_id: Uuid,
        effector_index: usize,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager.add_effector_keyframe(
            clip_id,
            effector_index,
            property_key,
            time,
            value,
            easing,
        )
    }

    pub fn update_effector_keyframe_by_index(
        &self,
        clip_id: Uuid,
        effector_index: usize,
        property_key: &str,
        keyframe_index: usize,
        time: Option<f64>,
        value: Option<PropertyValue>,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager.update_effector_keyframe_by_index(
            clip_id,
            effector_index,
            property_key,
            keyframe_index,
            time,
            value,
            easing,
        )
    }

    pub fn remove_effector_keyframe_by_index(
        &self,
        clip_id: Uuid,
        effector_index: usize,
        property_key: &str,
        keyframe_index: usize,
    ) -> Result<(), LibraryError> {
        self.project_manager.remove_effector_keyframe_by_index(
            clip_id,
            effector_index,
            property_key,
            keyframe_index,
        )
    }

    pub fn add_decorator_keyframe(
        &self,
        clip_id: Uuid,
        decorator_index: usize,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager.add_decorator_keyframe(
            clip_id,
            decorator_index,
            property_key,
            time,
            value,
            easing,
        )
    }

    pub fn update_decorator_keyframe_by_index(
        &self,
        clip_id: Uuid,
        decorator_index: usize,
        property_key: &str,
        keyframe_index: usize,
        time: Option<f64>,
        value: Option<PropertyValue>,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager.update_decorator_keyframe_by_index(
            clip_id,
            decorator_index,
            property_key,
            keyframe_index,
            time,
            value,
            easing,
        )
    }

    pub fn remove_decorator_keyframe_by_index(
        &self,
        clip_id: Uuid,
        decorator_index: usize,
        property_key: &str,
        keyframe_index: usize,
    ) -> Result<(), LibraryError> {
        self.project_manager.remove_decorator_keyframe_by_index(
            clip_id,
            decorator_index,
            property_key,
            keyframe_index,
        )
    }

    // Aliases & Sequences

    pub fn update_clip_time(
        &self,
        clip_id: Uuid,
        in_frame: u64,
        out_frame: u64,
    ) -> Result<(), LibraryError> {
        self.update_clip_property(
            clip_id,
            "in_frame",
            PropertyValue::Number(ordered_float::OrderedFloat(in_frame as f64)),
        )?;
        self.update_clip_property(
            clip_id,
            "out_frame",
            PropertyValue::Number(ordered_float::OrderedFloat(out_frame as f64)),
        )?;
        Ok(())
    }

    pub fn update_clip_source_frames(&self, clip_id: Uuid, frame: i64) -> Result<(), LibraryError> {
        self.update_clip_property(
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
    ) -> Vec<crate::model::project::property::PropertyDefinition> {
        self.project_manager
            .get_inspector_definitions(comp_id, track_id, clip_id)
    }

    pub fn get_property_definitions(
        &self,
        comp_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
    ) -> Vec<crate::model::project::property::PropertyDefinition> {
        self.get_inspector_definitions(comp_id, track_id, clip_id)
    }

    pub fn update_property_or_keyframe(
        &self,
        clip_id: Uuid,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .update_property_or_keyframe(clip_id, property_key, time, value, easing)
    }

    pub fn update_keyframe(
        &self,
        clip_id: Uuid,
        property_key: &str,
        index: usize,
        new_time: Option<f64>,
        new_value: Option<PropertyValue>,
        new_easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager.update_keyframe(
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
        clip_id: Uuid,
        effect_index: usize,
        property_key: &str,
        keyframe_index: usize,
        new_time: Option<f64>,
        new_value: Option<PropertyValue>,
        new_easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager.update_effect_keyframe_by_index(
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
        clip_id: Uuid,
        effect_index: usize,
        property_key: &str,
        keyframe_index: usize,
    ) -> Result<(), LibraryError> {
        self.project_manager.remove_effect_keyframe_by_index(
            clip_id,
            effect_index,
            property_key,
            keyframe_index,
        )
    }

    pub fn remove_keyframe(
        &self,
        clip_id: Uuid,
        property_key: &str,
        index: usize,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .remove_keyframe(clip_id, property_key, index)
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
        clip_id: Uuid,
        styles: Vec<crate::model::project::style::StyleInstance>,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .update_track_clip_styles(clip_id, styles)
    }

    pub fn update_track_clip_effectors(
        &self,
        clip_id: Uuid,
        effectors: Vec<crate::model::project::ensemble::EffectorInstance>,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .update_track_clip_effectors(clip_id, effectors)
    }

    pub fn update_track_clip_decorators(
        &self,
        clip_id: Uuid,
        decorators: Vec<crate::model::project::ensemble::DecoratorInstance>,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .update_track_clip_decorators(clip_id, decorators)
    }

    pub fn update_track_clip_style_property(
        &self,
        clip_id: Uuid,
        style_index: usize,
        property_key: &str,
        value: PropertyValue,
    ) -> Result<(), LibraryError> {
        self.project_manager.update_track_clip_style_property(
            clip_id,
            style_index,
            property_key,
            value,
        )
    }

    pub fn add_style_keyframe(
        &self,
        clip_id: Uuid,
        style_index: usize,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager.add_style_keyframe(
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
        clip_id: Uuid,
        effect_index: usize,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager.update_effect_property_or_keyframe(
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
        clip_id: Uuid,
        style_index: usize,
        property_key: &str,
        keyframe_index: usize,
        new_time: Option<f64>,
        new_value: Option<PropertyValue>,
        new_easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager.update_style_keyframe_by_index(
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
        clip_id: Uuid,
        style_index: usize,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager.update_style_property_or_keyframe(
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
        clip_id: Uuid,
        style_index: usize,
        property_key: &str,
        keyframe_index: usize,
    ) -> Result<(), LibraryError> {
        self.project_manager.remove_style_keyframe(
            clip_id,
            style_index,
            property_key,
            keyframe_index,
        )
    }

    pub fn set_style_property_attribute(
        &self,
        clip_id: Uuid,
        style_index: usize,
        property_key: &str,
        attribute_key: &str,
        attribute_value: PropertyValue,
    ) -> Result<(), LibraryError> {
        self.project_manager.set_style_property_attribute(
            clip_id,
            style_index,
            property_key,
            attribute_key,
            attribute_value,
        )
    }

    pub fn set_clip_property_attribute(
        &self,
        clip_id: Uuid,
        property_key: &str,
        attribute_key: &str,
        attribute_value: PropertyValue,
    ) -> Result<(), LibraryError> {
        self.project_manager.set_clip_property_attribute(
            clip_id,
            property_key,
            attribute_key,
            attribute_value,
        )
    }

    pub fn set_effect_property_attribute(
        &self,
        clip_id: Uuid,
        effect_index: usize,
        property_key: &str,
        attribute_key: &str,
        attribute_value: PropertyValue,
    ) -> Result<(), LibraryError> {
        self.project_manager.set_effect_property_attribute(
            clip_id,
            effect_index,
            property_key,
            attribute_key,
            attribute_value,
        )
    }

    pub fn set_effector_property_attribute(
        &self,
        clip_id: Uuid,
        effector_index: usize,
        property_key: &str,
        attribute_key: &str,
        attribute_value: PropertyValue,
    ) -> Result<(), LibraryError> {
        self.project_manager.set_effector_property_attribute(
            clip_id,
            effector_index,
            property_key,
            attribute_key,
            attribute_value,
        )
    }

    pub fn set_decorator_property_attribute(
        &self,
        clip_id: Uuid,
        decorator_index: usize,
        property_key: &str,
        attribute_key: &str,
        attribute_value: PropertyValue,
    ) -> Result<(), LibraryError> {
        self.project_manager.set_decorator_property_attribute(
            clip_id,
            decorator_index,
            property_key,
            attribute_key,
            attribute_value,
        )
    }

    pub fn add_style(&self, clip_id: Uuid, style_type: &str) -> Result<(), LibraryError> {
        self.project_manager.add_style(clip_id, style_type)
    }

    pub fn add_effector(&self, clip_id: Uuid, effector_type: &str) -> Result<(), LibraryError> {
        self.project_manager.add_effector(clip_id, effector_type)
    }

    pub fn add_decorator(&self, clip_id: Uuid, decorator_type: &str) -> Result<(), LibraryError> {
        self.project_manager.add_decorator(clip_id, decorator_type)
    }
}
