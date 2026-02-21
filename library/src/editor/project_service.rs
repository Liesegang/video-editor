use crate::core::color::ColorSpaceManager;
use crate::editor::handlers;
use crate::editor::handlers::clip_factory::ClipFactory;
use crate::error::LibraryError;
use crate::model::project::asset::Asset;
use crate::model::project::clip::TrackClip;
use crate::model::project::node::Node;
use crate::model::project::project::{Composition, Project};
use crate::model::project::property::PropertyValue;
use crate::model::project::property::{PropertyDefinition, PropertyUiType};
use crate::model::project::track::TrackData;
use crate::plugin::PluginManager;
use crate::plugin::entity_converter::measure_text_size;
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
        let (default_comp, root_track) =
            Composition::new("Main Composition", 1920, 1080, 30.0, 60.0);
        let new_comp_id = default_comp.id;
        new_project.add_node(Node::Track(root_track));
        new_project.add_composition(default_comp);

        let mut project_write = self.project.write().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project write lock: {}", e))
        })?;
        *project_write = new_project.clone();

        Ok((new_comp_id, new_project))
    }

    // --- Clip Factory Methods (delegated to ClipFactory) ---

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
        ClipFactory::create_audio_clip(
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
        ClipFactory::create_video_clip(
            &self.plugin_manager,
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
        ClipFactory::create_image_clip(
            &self.plugin_manager,
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
        ClipFactory::create_text_clip(
            &self.plugin_manager,
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
        ClipFactory::create_shape_clip(
            &self.plugin_manager,
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
        ClipFactory::create_sksl_clip(
            &self.plugin_manager,
            in_frame,
            out_frame,
            canvas_width,
            canvas_height,
            fps,
        )
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

    /// Remove all clips that reference the given entity from all tracks and nodes.
    fn remove_referencing_clips(project: &mut Project, entity_id: Uuid) {
        let clip_ids_to_remove: Vec<Uuid> = project
            .all_clips()
            .filter(|c| c.reference_id == Some(entity_id))
            .map(|c| c.id)
            .collect();

        for clip_id in &clip_ids_to_remove {
            for track in project.all_tracks().map(|t| t.id).collect::<Vec<_>>() {
                if let Some(t) = project.get_track_mut(track) {
                    t.remove_child(*clip_id);
                }
            }
            project.remove_node(*clip_id);
        }
    }

    pub fn remove_asset_fully(&self, asset_id: Uuid) -> Result<(), LibraryError> {
        let mut project_write = handlers::write_project(&self.project)?;
        Self::remove_referencing_clips(&mut project_write, asset_id);
        project_write.assets.retain(|a| a.id != asset_id);
        Ok(())
    }

    pub fn remove_composition_fully(&self, comp_id: Uuid) -> Result<(), LibraryError> {
        let mut project_write = handlers::write_project(&self.project)?;
        Self::remove_referencing_clips(&mut project_write, comp_id);
        project_write.compositions.retain(|c| c.id != comp_id);
        Ok(())
    }

    pub fn import_file(&self, path: &str) -> Result<Vec<Uuid>, LibraryError> {
        let path_obj = std::path::Path::new(path);
        let base_name = path_obj
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let mut assets_to_add = Vec::new();

        // 1. Try to get all streams
        if let Some(streams) = self.plugin_manager.get_available_streams(path) {
            for stream in streams {
                let suffix = if let Some(idx) = stream.stream_index {
                    format!(" [Stream {}: {:?}]", idx, stream.kind)
                } else {
                    "".to_string()
                };
                let name = format!("{}{}", base_name, suffix);

                let mut asset = crate::model::project::asset::Asset::new(&name, path, stream.kind);
                asset.duration = stream.duration;
                asset.fps = stream.fps;
                asset.width = stream.width;
                asset.height = stream.height;
                asset.stream_index = stream.stream_index;

                assets_to_add.push(asset);
            }
        }

        // 2. Fallback if no streams returned (or empty list)
        if assets_to_add.is_empty() {
            // 1. Get Metadata (Single call)
            let (mut kind, duration, fps, width, height) =
                if let Some(meta) = self.plugin_manager.get_metadata(path) {
                    (meta.kind, meta.duration, meta.fps, meta.width, meta.height)
                } else {
                    (
                        crate::model::project::asset::AssetKind::Other,
                        None,
                        None,
                        None,
                        None,
                    )
                };

            // 2. Fallback for Kind if Unknown
            if kind == crate::model::project::asset::AssetKind::Other {
                // Fallback to extension if plugin didn't detect it
                let ext = path_obj
                    .extension()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase();
                kind = match ext.as_str() {
                    "mp4" | "mov" | "avi" | "mkv" | "webm" => {
                        crate::model::project::asset::AssetKind::Video
                    }
                    "png" | "jpg" | "jpeg" | "bmp" | "webp" => {
                        crate::model::project::asset::AssetKind::Image
                    }
                    "mp3" | "wav" | "ogg" | "aac" | "flac" => {
                        crate::model::project::asset::AssetKind::Audio
                    }
                    "obj" | "gltf" | "glb" => crate::model::project::asset::AssetKind::Model3D,
                    _ => crate::model::project::asset::AssetKind::Other,
                };
            }

            // 3. Create Asset
            let mut asset = crate::model::project::asset::Asset::new(&base_name, path, kind);
            asset.duration = duration;
            asset.fps = fps;
            asset.width = width;
            asset.height = height;
            // stream_index remains None

            assets_to_add.push(asset);
        }

        let mut added_ids = Vec::new();
        for asset in assets_to_add {
            let id = self.add_asset(asset)?;
            added_ids.push(id);
        }

        Ok(added_ids)
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
        let mut track = TrackData::new(track_name);
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

    pub fn get_track(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
    ) -> Result<TrackData, LibraryError> {
        handlers::track_handler::TrackHandler::get_track(&self.project, composition_id, track_id)
    }

    pub fn remove_track(&self, composition_id: Uuid, track_id: Uuid) -> Result<(), LibraryError> {
        handlers::track_handler::TrackHandler::remove_track(&self.project, composition_id, track_id)
    }

    pub fn add_sub_track(
        &self,
        composition_id: Uuid,
        parent_track_id: Uuid,
        track_name: &str,
    ) -> Result<Uuid, LibraryError> {
        handlers::track_handler::TrackHandler::add_sub_track(
            &self.project,
            composition_id,
            parent_track_id,
            track_name,
        )
    }

    pub fn rename_track(&self, track_id: Uuid, new_name: &str) -> Result<(), LibraryError> {
        handlers::track_handler::TrackHandler::rename_track(&self.project, track_id, new_name)
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
        let clip_kind = clip.kind.clone();
        let clip_id = handlers::clip_handler::ClipHandler::add_clip_to_track(
            &self.project,
            composition_id,
            track_id,
            clip,
            in_frame,
            out_frame,
            insert_index,
        )?;

        // Auto-create default graph nodes (transform, style.fill) for the new clip
        if let Err(e) = handlers::clip_handler::ClipHandler::setup_clip_graph_nodes(
            &self.project,
            &self.plugin_manager,
            track_id,
            clip_id,
            &clip_kind,
        ) {
            log::warn!("Failed to setup graph nodes for clip {}: {}", clip_id, e);
        }

        Ok(clip_id)
    }

    pub fn remove_clip_from_track(
        &self,
        track_id: Uuid,
        clip_id: Uuid,
    ) -> Result<(), LibraryError> {
        handlers::clip_handler::ClipHandler::remove_clip_from_track(
            &self.project,
            track_id,
            clip_id,
        )
    }

    pub fn update_track_clip_property(
        &self,
        clip_id: Uuid,
        property_key: &str,
        value: PropertyValue,
    ) -> Result<(), LibraryError> {
        handlers::clip_handler::ClipHandler::update_target_property_or_keyframe(
            &self.project,
            clip_id,
            crate::model::project::property::PropertyTarget::Clip,
            property_key,
            0.0,
            value,
            None,
        )
    }

    pub fn update_property_or_keyframe(
        &self,
        clip_id: Uuid,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        handlers::clip_handler::ClipHandler::update_target_property_or_keyframe(
            &self.project,
            clip_id,
            crate::model::project::property::PropertyTarget::Clip,
            property_key,
            time,
            value,
            easing,
        )
    }

    pub fn add_target_keyframe(
        &self,
        clip_id: Uuid,
        target: crate::model::project::property::PropertyTarget,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        handlers::keyframe_handler::KeyframeHandler::add_keyframe(
            &self.project,
            clip_id,
            target,
            property_key,
            time,
            value,
            easing,
        )
    }

    pub fn update_target_keyframe_by_index(
        &self,
        clip_id: Uuid,
        target: crate::model::project::property::PropertyTarget,
        property_key: &str,
        keyframe_index: usize,
        time: Option<f64>,
        value: Option<PropertyValue>,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        handlers::keyframe_handler::KeyframeHandler::update_keyframe_by_index(
            &self.project,
            clip_id,
            target,
            property_key,
            keyframe_index,
            time,
            value,
            easing,
        )
    }

    pub fn remove_target_keyframe_by_index(
        &self,
        clip_id: Uuid,
        target: crate::model::project::property::PropertyTarget,
        property_key: &str,
        keyframe_index: usize,
    ) -> Result<(), LibraryError> {
        handlers::keyframe_handler::KeyframeHandler::remove_keyframe_by_index(
            &self.project,
            clip_id,
            target,
            property_key,
            keyframe_index,
        )
    }

    pub fn update_target_property_or_keyframe(
        &self,
        clip_id: Uuid,
        target: crate::model::project::property::PropertyTarget,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        handlers::clip_handler::ClipHandler::update_target_property_or_keyframe(
            &self.project,
            clip_id,
            target,
            property_key,
            time,
            value,
            easing,
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
        property: &crate::model::project::property::Property,
        context: &crate::model::project::property::PropertyMap,
        time: f64,
        fps: f64,
    ) -> PropertyValue {
        let eval_ctx = crate::plugin::EvaluationContext {
            property_map: context,
            fps,
        };
        self.plugin_manager
            .get_property_evaluators()
            .evaluate(property, time, &eval_ctx)
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

    pub fn move_clip_to_track_at_index(
        &self,
        composition_id: Uuid,
        source_track_id: Uuid,
        clip_id: Uuid,
        target_track_id: Uuid,
        new_in_frame: u64,
        target_index: Option<usize>,
    ) -> Result<(), LibraryError> {
        handlers::clip_handler::ClipHandler::move_clip_to_track_at_index(
            &self.project,
            composition_id,
            source_track_id,
            clip_id,
            target_track_id,
            new_in_frame,
            target_index,
        )
    }

    pub fn set_property_attribute(
        &self,
        clip_id: Uuid,
        target: crate::model::project::property::PropertyTarget,
        property_key: &str,
        attribute_key: &str,
        attribute_value: PropertyValue,
    ) -> Result<(), LibraryError> {
        handlers::clip_handler::ClipHandler::set_property_attribute(
            &self.project,
            clip_id,
            target,
            property_key,
            attribute_key,
            attribute_value,
        )
    }

    // --- Graph Node Operations ---

    pub fn add_graph_node(&self, container_id: Uuid, type_id: &str) -> Result<Uuid, LibraryError> {
        handlers::graph_handler::GraphHandler::add_graph_node(
            &self.project,
            &self.plugin_manager,
            container_id,
            type_id,
        )
    }

    pub fn remove_graph_node(&self, node_id: Uuid) -> Result<(), LibraryError> {
        handlers::graph_handler::GraphHandler::remove_graph_node(&self.project, node_id)
    }

    pub fn add_graph_connection(
        &self,
        from: crate::model::project::connection::PinId,
        to: crate::model::project::connection::PinId,
    ) -> Result<crate::model::project::connection::Connection, LibraryError> {
        handlers::graph_handler::GraphHandler::add_connection(&self.project, from, to)
    }

    pub fn remove_graph_connection(&self, connection_id: Uuid) -> Result<(), LibraryError> {
        handlers::graph_handler::GraphHandler::remove_connection(&self.project, connection_id)
    }

    pub fn update_graph_node_property(
        &self,
        node_id: Uuid,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        handlers::graph_handler::GraphHandler::update_graph_node_property(
            &self.project,
            node_id,
            property_key,
            time,
            value,
            easing,
        )
    }

    pub fn get_inspector_definitions(
        &self,
        comp_id: uuid::Uuid,
        _track_id: uuid::Uuid,
        clip_id: uuid::Uuid,
    ) -> Vec<PropertyDefinition> {
        let project = self.project.read().unwrap();

        let (clip, canvas_width, canvas_height) =
            if let Some(comp) = project.compositions.iter().find(|c| c.id == comp_id) {
                if let Some(c) = project.get_clip(clip_id) {
                    (c.clone(), comp.width, comp.height)
                } else {
                    return Vec::new();
                }
            } else {
                return Vec::new();
            };

        // Resolve clip dimensions
        let (clip_width, clip_height): (u64, u64) = match clip.kind {
            crate::model::project::clip::TrackClipKind::Video
            | crate::model::project::clip::TrackClipKind::Image => {
                if let Some(asset_id) = clip.reference_id {
                    if let Some(asset) = project.assets.iter().find(|a| a.id == asset_id) {
                        (
                            asset.width.unwrap_or(100) as u64,
                            asset.height.unwrap_or(100) as u64,
                        )
                    } else {
                        (100, 100)
                    }
                } else {
                    (100, 100)
                }
            }
            crate::model::project::clip::TrackClipKind::Shape => {
                // Try to get from properties, otherwise default 100
                let w = clip.properties.get_f64("width").unwrap_or(100.0) as u64;
                let h = clip.properties.get_f64("height").unwrap_or(100.0) as u64;
                (w, h)
            }
            crate::model::project::clip::TrackClipKind::Text => {
                let text = clip
                    .properties
                    .get_string("text")
                    .unwrap_or("Text".to_string());
                let font_name = clip
                    .properties
                    .get_string("font_family")
                    .unwrap_or("Arial".to_string());
                let size = clip.properties.get_f64("size").unwrap_or(100.0) as f32;

                let (w, h) = measure_text_size(&text, &font_name, size);
                (w.round() as u64, h.round() as u64)
            }
            crate::model::project::clip::TrackClipKind::SkSL => (canvas_width, canvas_height),
            _ => (100, 100),
        };

        let converter = self
            .plugin_manager
            .get_entity_converter(&clip.kind.to_string());

        let mut definitions = if let Some(converter) = converter {
            converter.get_property_definitions(canvas_width, canvas_height, clip_width, clip_height)
        } else {
            Vec::new()
        };

        if matches!(clip.kind, crate::model::project::clip::TrackClipKind::Video) {
            let colorspaces = ColorSpaceManager::get_available_colorspaces();
            if !colorspaces.is_empty() {
                definitions.push(PropertyDefinition::new(
                    "input_color_space",
                    PropertyUiType::Dropdown {
                        options: colorspaces.clone(),
                    },
                    "Input Color Space",
                    PropertyValue::String("".to_string()),
                ));
                definitions.push(PropertyDefinition::new(
                    "output_color_space",
                    PropertyUiType::Dropdown {
                        options: colorspaces,
                    },
                    "Output Color Space",
                    PropertyValue::String("".to_string()),
                ));
            }
        }

        // let mut definitions = clip.default_property_definitions(canvas_width, canvas_height, clip_width, clip_height); // Removed original line

        definitions
    }
}
