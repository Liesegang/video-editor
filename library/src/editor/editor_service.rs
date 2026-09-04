use crate::core::audio::engine::AudioEngine;
use crate::core::cache::CacheManager;
use crate::editor::audio_service::AudioService;
use crate::editor::handlers::clip_handler::ClipBundle;
use crate::editor::handlers::keyframe_handler::KeyframeBatchUpdate;
use crate::editor::handlers::property_ops::PropertyOwner;
use crate::editor::project_service::{
    AssetSourceColorInspector, GeneratorNodeRequest, ProjectManager, SourceColorMetadataRefresh,
};
use crate::error::LibraryError;
use crate::model::asset::Asset;
use crate::model::frame::color::Color;
use crate::model::project::{Composition, NodeGraphBundle, Project};
use crate::model::property::{KeyframeId, KeyframeUpdate, Property, PropertyValue};
use crate::model::{Node, Track};
use crate::plugin::PluginManager;
use std::rc::Rc;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

mod semantic;

pub struct EditorService {
    project_manager: Arc<ProjectManager>,
    audio_service: Rc<AudioService>,
}

impl Clone for EditorService {
    fn clone(&self) -> Self {
        Self {
            project_manager: self.project_manager.clone(),
            audio_service: self.audio_service.clone(),
        }
    }
}

impl EditorService {}

impl EditorService {
    pub fn new(
        project: Arc<RwLock<Project>>,
        plugin_manager: Arc<PluginManager>,
        cache_manager: Arc<CacheManager>,
    ) -> Result<Self, LibraryError> {
        let audio_engine = Rc::new(AudioEngine::new().map_err(|error| {
            LibraryError::Runtime(format!("Failed to initialize audio engine: {error}"))
        })?);

        let project_manager =
            Arc::new(ProjectManager::new(project.clone(), plugin_manager.clone()));
        // AudioService also needs shared access to Project for mixing
        let audio_service = Rc::new(AudioService::new(
            project,
            audio_engine,
            cache_manager,
            plugin_manager,
        )?);

        Ok(Self {
            project_manager,
            audio_service,
        })
    }

    pub fn get_project(&self) -> Arc<RwLock<Project>> {
        self.project_manager.get_project()
    }

    pub fn set_project(&self, project: crate::model::project::Project) -> Result<(), LibraryError> {
        self.audio_service.invalidate_project();
        self.project_manager.set_project(project)
    }

    pub fn get_audio_service(&self) -> Rc<AudioService> {
        self.audio_service.clone()
    }

    pub fn get_cache_manager(&self) -> Arc<crate::cache::CacheManager> {
        self.audio_service.get_cache_manager()
    }

    pub fn get_plugin_manager(&self) -> Arc<PluginManager> {
        self.project_manager.get_plugin_manager()
    }

    pub fn get_audio_engine(&self) -> Rc<AudioEngine> {
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

    pub fn set_active_composition(&self, composition_id: Option<Uuid>, time: f64) -> bool {
        self.audio_service
            .set_active_composition(composition_id, time)
    }

    // --- Project Operations ---

    pub fn load_project(&self, json_str: &str) -> Result<(), LibraryError> {
        self.audio_service.invalidate_project();
        self.project_manager.load_project(json_str)?;
        Ok(())
    }

    pub fn create_new_project(&self) -> Result<Uuid, LibraryError> {
        self.audio_service.invalidate_project();
        let (new_comp_id, _) = self.project_manager.create_new_project()?;
        Ok(new_comp_id)
    }

    pub fn save_project(&self) -> Result<String, LibraryError> {
        self.project_manager.save_project()
    }

    pub fn import_file(&self, path: &str) -> Result<Vec<Uuid>, LibraryError> {
        let asset_ids = self.project_manager.import_file(path)?;

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

    pub fn asset_source_color_inspectors(
        &self,
        owner: crate::model::project::NodeContainer,
    ) -> Result<Vec<AssetSourceColorInspector>, LibraryError> {
        self.project_manager.asset_source_color_inspectors(owner)
    }

    pub fn assign_asset_source_color_space(
        &self,
        asset_id: Uuid,
        color_space: &str,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .assign_asset_source_color_space(asset_id, color_space)
    }

    pub fn clear_asset_source_color_space(&self, asset_id: Uuid) -> Result<(), LibraryError> {
        self.project_manager
            .clear_asset_source_color_space(asset_id)
    }

    pub fn use_detected_asset_source_color(&self, asset_id: Uuid) -> Result<(), LibraryError> {
        self.project_manager
            .use_detected_asset_source_color(asset_id)
    }

    pub fn refresh_asset_source_color_metadata(
        &self,
        asset_id: Uuid,
    ) -> Result<SourceColorMetadataRefresh, LibraryError> {
        self.project_manager
            .refresh_asset_source_color_metadata(asset_id)
    }

    pub fn clear_legacy_media_node_color_properties(
        &self,
        node_id: Uuid,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .clear_legacy_media_node_color_properties(node_id)
    }

    // --- Clip Factory Methods ---

    pub fn create_generator_node(
        &self,
        request: GeneratorNodeRequest,
        canvas_width: u64,
        canvas_height: u64,
        clip_width: u64,
        clip_height: u64,
    ) -> Result<Node, LibraryError> {
        self.project_manager.create_generator_node(
            request,
            canvas_width,
            canvas_height,
            clip_width,
            clip_height,
        )
    }

    pub fn create_text_node(
        &self,
        text: &str,
        font: &str,
        canvas_width: u64,
        canvas_height: u64,
    ) -> Result<Node, LibraryError> {
        self.project_manager
            .create_text_node(text, font, canvas_width, canvas_height)
    }

    pub fn create_text_graph(
        &self,
        text: &str,
        font: &str,
        canvas_width: u64,
        canvas_height: u64,
    ) -> Result<NodeGraphBundle, LibraryError> {
        self.project_manager
            .create_text_graph(text, font, canvas_width, canvas_height)
    }

    pub fn create_shape_node(
        &self,
        path: &str,
        canvas_width: u64,
        canvas_height: u64,
        shape_width: u64,
        shape_height: u64,
    ) -> Result<Node, LibraryError> {
        self.project_manager.create_shape_node(
            path,
            canvas_width,
            canvas_height,
            shape_width,
            shape_height,
        )
    }

    pub fn create_shape_graph(
        &self,
        path: &str,
        canvas_width: u64,
        canvas_height: u64,
        shape_width: u64,
        shape_height: u64,
    ) -> Result<NodeGraphBundle, LibraryError> {
        self.project_manager.create_shape_graph(
            path,
            canvas_width,
            canvas_height,
            shape_width,
            shape_height,
        )
    }

    pub fn create_sksl_node(
        &self,
        shader: &str,
        canvas_width: u64,
        canvas_height: u64,
    ) -> Result<Node, LibraryError> {
        self.project_manager
            .create_sksl_node(shader, canvas_width, canvas_height)
    }

    pub fn create_solid_node(
        &self,
        color: Color,
        canvas_width: u64,
        canvas_height: u64,
    ) -> Result<Node, LibraryError> {
        self.project_manager
            .create_solid_node(color, canvas_width, canvas_height)
    }

    pub fn create_audio_clip(
        &self,
        asset_id: Uuid,
        file_path: &str,
        start_time: f64,
        duration: f64,
        source_start_time: f64,
        speed: f64,
    ) -> Result<ClipBundle, LibraryError> {
        self.project_manager.create_audio_clip(
            asset_id,
            file_path,
            start_time,
            duration,
            source_start_time,
            speed,
        )
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "detached video Clip creation requires source timing and canvas dimensions atomically"
    )]
    pub fn create_video_clip(
        &self,
        asset_id: Uuid,
        file_path: &str,
        start_time: f64,
        duration: f64,
        source_start_time: f64,
        speed: f64,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Result<ClipBundle, LibraryError> {
        self.project_manager.create_video_clip(
            asset_id,
            file_path,
            start_time,
            duration,
            source_start_time,
            speed,
            canvas_width,
            canvas_height,
        )
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "the editor facade preserves image timing, canvas dimensions, and caller FPS as one operation"
    )]
    pub fn create_image_clip(
        &self,
        asset_id: Uuid,
        file_path: &str,
        start_time: f64,
        duration: f64,
        canvas_width: u32,
        canvas_height: u32,
        _fps: f64,
    ) -> Result<ClipBundle, LibraryError> {
        self.project_manager.create_image_clip(
            asset_id,
            file_path,
            start_time,
            duration,
            canvas_width,
            canvas_height,
        )
    }

    pub fn create_text_clip(
        &self,
        text: &str,
        start_time: f64,
        duration: f64,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Result<ClipBundle, LibraryError> {
        self.project_manager.create_text_clip(
            text,
            start_time,
            duration,
            canvas_width,
            canvas_height,
        )
    }

    pub fn create_shape_clip(
        &self,
        start_time: f64,
        duration: f64,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Result<ClipBundle, LibraryError> {
        self.project_manager
            .create_shape_clip(start_time, duration, canvas_width, canvas_height)
    }

    pub fn create_sksl_clip(
        &self,
        start_time: f64,
        duration: f64,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Result<ClipBundle, LibraryError> {
        self.project_manager
            .create_sksl_clip(start_time, duration, canvas_width, canvas_height)
    }

    pub fn create_composition_instance_clip(
        &self,
        composition_id: Uuid,
        start_time: f64,
        duration: f64,
    ) -> Result<ClipBundle, LibraryError> {
        self.project_manager
            .create_composition_instance_clip(composition_id, start_time, duration)
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

    pub fn rename_track(&self, track_id: Uuid, new_name: &str) -> Result<(), LibraryError> {
        self.project_manager.rename_track(track_id, new_name)
    }

    pub fn move_track_within_composition(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        destination_index: usize,
    ) -> Result<bool, LibraryError> {
        self.project_manager.move_track_within_composition(
            composition_id,
            track_id,
            destination_index,
        )
    }

    pub fn add_clip_to_track(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        bundle: ClipBundle,
        insert_index: Option<usize>,
    ) -> Result<Uuid, LibraryError> {
        self.project_manager
            .add_clip_to_track(composition_id, track_id, bundle, insert_index)
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
        self.project_manager.update_property_or_keyframe(
            PropertyOwner::Clip(clip_id),
            property_key,
            0.0,
            value,
            None,
        )
    }

    pub fn move_clip_to_track(
        &self,
        composition_id: Uuid,
        source_track_id: Uuid,
        clip_id: Uuid,
        target_track_id: Uuid,
        new_start_time: f64,
    ) -> Result<(), LibraryError> {
        self.project_manager.move_clip_to_track(
            composition_id,
            source_track_id,
            clip_id,
            target_track_id,
            new_start_time,
        )
    }

    pub fn move_clip_to_track_at_index(
        &self,
        composition_id: Uuid,
        source_track_id: Uuid,
        clip_id: Uuid,
        target_track_id: Uuid,
        new_start_time: f64,
        target_index: Option<usize>,
    ) -> Result<(), LibraryError> {
        self.project_manager.move_clip_to_track_at_index(
            composition_id,
            source_track_id,
            clip_id,
            target_track_id,
            new_start_time,
            target_index,
        )
    }

    pub fn evaluate_property_value(
        &self,
        property: &crate::model::property::Property,
        context: &crate::model::property::PropertyMap,
        time: f64,
        fps: f64,
        resolution: (u64, u64),
    ) -> Result<PropertyValue, crate::plugin::PropertyEvaluationError> {
        self.project_manager
            .evaluate_property_value(property, context, time, fps, resolution)
    }

    pub fn evaluate_property_with_diagnostics(
        &self,
        property: &crate::model::property::Property,
        context: &crate::model::property::PropertyMap,
        time: f64,
        fps: f64,
        resolution: (u64, u64),
    ) -> Result<crate::plugin::PropertyEvaluationOutcome, crate::plugin::PropertyEvaluationError>
    {
        self.project_manager
            .evaluate_property_with_diagnostics(property, context, time, fps, resolution)
    }

    pub fn add_keyframe(
        &self,
        owner: PropertyOwner,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .add_keyframe(owner, property_key, time, value, easing)
    }

    pub fn add_keyframe_with_id(
        &self,
        owner: PropertyOwner,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<KeyframeId, LibraryError> {
        self.project_manager
            .add_keyframe_with_id(owner, property_key, time, value, easing)
    }

    // Aliases & Sequences

    pub fn update_clip_timing(
        &self,
        clip_id: Uuid,
        start_time: f64,
        duration: f64,
        trim_in: f64,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .update_clip_timing(clip_id, start_time, duration, trim_in)
    }

    pub fn update_clip_source_start(&self, clip_id: Uuid, time: f64) -> Result<(), LibraryError> {
        self.update_clip_property(
            clip_id,
            "trim_in", // or source_start_time? ClipHandler used 'trim_in' for source trimming in previous logic
            PropertyValue::Number(ordered_float::OrderedFloat(time)),
        )
    }

    pub fn get_inspector_definitions(
        &self,
        comp_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
    ) -> Vec<crate::model::property::PropertyDefinition> {
        self.project_manager
            .get_inspector_definitions(comp_id, track_id, clip_id)
    }

    pub fn get_property_definitions(
        &self,
        comp_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
    ) -> Vec<crate::model::property::PropertyDefinition> {
        self.get_inspector_definitions(comp_id, track_id, clip_id)
    }

    pub fn update_property_or_keyframe(
        &self,
        owner: PropertyOwner,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .update_property_or_keyframe(owner, property_key, time, value, easing)
    }

    /// Applies a grouped property gesture as one model transaction.
    pub fn update_properties_or_keyframes(
        &self,
        owner: PropertyOwner,
        time: f64,
        values: &[(String, PropertyValue)],
    ) -> Result<(), LibraryError> {
        self.project_manager
            .update_properties_or_keyframes(owner, time, values)
    }

    pub fn replace_property(
        &self,
        owner: PropertyOwner,
        property_key: &str,
        property: Property,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .replace_property(owner, property_key, property)
    }

    pub fn set_expression_source(
        &self,
        owner: PropertyOwner,
        property_key: &str,
        source: String,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .set_expression_source(owner, property_key, source)
    }

    pub fn update_keyframe_by_id(
        &self,
        owner: PropertyOwner,
        property_key: &str,
        keyframe_id: KeyframeId,
        update: KeyframeUpdate,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .update_keyframe_by_id(owner, property_key, keyframe_id, update)
    }

    pub fn update_keyframes_batch(
        &self,
        updates: &[KeyframeBatchUpdate],
    ) -> Result<(), LibraryError> {
        self.project_manager.update_keyframes_batch(updates)
    }

    pub fn remove_keyframe_by_id(
        &self,
        owner: PropertyOwner,
        property_key: &str,
        keyframe_id: KeyframeId,
    ) -> Result<(), LibraryError> {
        self.project_manager
            .remove_keyframe_by_id(owner, property_key, keyframe_id)
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
    pub fn add_effector(&self, node_id: Uuid, effector_type: &str) -> Result<(), LibraryError> {
        self.project_manager.add_effector(node_id, effector_type)
    }

    pub fn add_decorator(&self, node_id: Uuid, decorator_type: &str) -> Result<(), LibraryError> {
        self.project_manager.add_decorator(node_id, decorator_type)
    }

    pub fn set_property_attribute(
        &self,
        owner: PropertyOwner,
        property_key: &str,
        attribute_key: &str,
        attribute_value: PropertyValue,
    ) -> Result<(), LibraryError> {
        self.project_manager.set_property_attribute(
            owner,
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
        self.project_manager.set_property_attribute(
            PropertyOwner::Clip(clip_id),
            property_key,
            attribute_key,
            attribute_value,
        )
    }
}
