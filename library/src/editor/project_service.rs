use crate::editor::handlers;
use crate::editor::handlers::clip_handler::ClipBundle;
use crate::editor::handlers::property_ops::PropertyOwner;
use crate::error::LibraryError;
use crate::model::asset::Asset;
use crate::model::frame::color::Color;
use crate::model::project::{
    Composition, DECORATOR_OUTPUT_PORT, DECORATORS_INPUT_PORT, EFFECTOR_OUTPUT_PORT,
    EFFECTORS_INPUT_PORT, NodeGraphBundle, PortAddress, PortOwner, Project, ProjectConnection,
    STYLE_OUTPUT_PORT, STYLES_INPUT_PORT,
};
use crate::model::property::{
    KeyframeId, KeyframeUpdate, Property, PropertyDefinition, PropertyMap, PropertyUiType,
    PropertyValue,
};
use crate::model::{
    Clip, EffectConfig, GeneratorContent, MediaContent, Node, NodeContent, ReferenceContent, Track,
};
use crate::plugin::PluginManager;
use crate::plugin::entity_converter::measure_text_size;
use ordered_float::OrderedFloat;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub const DEFAULT_TEXT_FONT: &str = "Arial";
pub const DEFAULT_SHAPE_PATH: &str =
    "M 50,30 A 20,20 0,0,1 90,30 C 90,55 50,85 50,85 C 50,85 10,55 10,30 A 20,20 0,0,1 50,30 Z";
pub const DEFAULT_SKSL_SHADER: &str = r#"
half4 main(float2 fragCoord) {
    float2 uv = fragCoord / iResolution.xy;
    float3 col = 0.5 + 0.5*cos(iTime+uv.xyx+float3(0,2,4));
    return half4(col,1.0);
}
"#;

/// Creation-only values for a generator Node. Authored values are materialized
/// into `Node::properties`; `GeneratorContent` stores only the generator kind.
#[derive(Clone, Debug, PartialEq)]
pub enum GeneratorNodeRequest {
    Text { text: String, font: String },
    Shape { path: String },
    Solid { color: Color },
    SkSL { shader: String },
}

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
        Self::validate_project_for_adoption(&new_project)?;
        let mut project_write = self.project.write().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project write lock: {}", e))
        })?;
        *project_write = new_project;
        Ok(())
    }

    pub fn load_project(&self, json_str: &str) -> Result<Project, LibraryError> {
        let new_project = Project::load(json_str)?;
        Self::validate_project_for_adoption(&new_project)?;
        let mut project_write = self.project.write().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project write lock: {}", e))
        })?;
        *project_write = new_project.clone();
        Ok(new_project)
    }

    fn validate_project_for_adoption(project: &Project) -> Result<(), LibraryError> {
        let errors = project.validation_issues();
        if errors.is_empty() {
            return Ok(());
        }
        Err(LibraryError::Validation(
            errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; "),
        ))
    }

    pub fn create_new_project(&self) -> Result<(Uuid, Project), LibraryError> {
        let mut new_project = Project::new("New Project");
        let (default_comp, root_track) =
            Composition::new("Main Composition", 1920, 1080, 30.0, 60.0);
        let new_comp_id = default_comp.id;
        new_project.add_track(root_track);
        new_project.add_composition(default_comp);

        let mut project_write = self.project.write().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project write lock: {}", e))
        })?;
        *project_write = new_project.clone();

        Ok((new_comp_id, new_project))
    }

    // --- Clip Factory Methods ---

    /// Builds a detached generator Node from the same converter definitions
    /// used by timeline Clip factories. Every definition is materialized in
    /// the authoritative property map before content-specific values replace
    /// their defaults.
    pub fn create_generator_node(
        &self,
        request: GeneratorNodeRequest,
        canvas_width: u64,
        canvas_height: u64,
        clip_width: u64,
        clip_height: u64,
    ) -> Result<Node, LibraryError> {
        self.create_generator_node_with_style_mode(
            request,
            canvas_width,
            canvas_height,
            clip_width,
            clip_height,
            true,
        )
    }

    fn create_generator_node_with_style_mode(
        &self,
        request: GeneratorNodeRequest,
        canvas_width: u64,
        canvas_height: u64,
        clip_width: u64,
        clip_height: u64,
        include_legacy_styles: bool,
    ) -> Result<Node, LibraryError> {
        let (name, converter_kind, style_types): (&str, &str, &[&str]) = match &request {
            GeneratorNodeRequest::Text { .. } => ("Text", "text", &["fill"]),
            GeneratorNodeRequest::Shape { .. } => ("Shape", "shape", &["fill", "stroke"]),
            GeneratorNodeRequest::Solid { .. } => ("Solid", "solid", &[]),
            GeneratorNodeRequest::SkSL { .. } => ("SkSL", "sksl", &[]),
        };
        let converter = self
            .plugin_manager
            .get_entity_converter(converter_kind)
            .ok_or_else(|| {
                LibraryError::Plugin(format!(
                    "{name} converter plugin not found ({converter_kind})"
                ))
            })?;
        let definitions = converter.get_property_definitions(
            canvas_width,
            canvas_height,
            clip_width,
            clip_height,
        );
        let mut properties = PropertyMap::from_definitions(&definitions);

        let content = match &request {
            GeneratorNodeRequest::Text { text, font } => {
                properties.set(
                    "text".to_string(),
                    Property::constant(PropertyValue::String(text.clone())),
                );
                properties.set(
                    "font_family".to_string(),
                    Property::constant(PropertyValue::String(font.clone())),
                );
                GeneratorContent::Text
            }
            GeneratorNodeRequest::Shape { path } => {
                properties.set(
                    "path".to_string(),
                    Property::constant(PropertyValue::String(path.clone())),
                );
                GeneratorContent::Shape
            }
            GeneratorNodeRequest::Solid { color } => {
                properties.set(
                    "color".to_string(),
                    Property::constant(PropertyValue::Color(color.clone())),
                );
                GeneratorContent::Solid
            }
            GeneratorNodeRequest::SkSL { shader } => {
                properties.set(
                    "shader".to_string(),
                    Property::constant(PropertyValue::String(shader.clone())),
                );
                GeneratorContent::SkSL
            }
        };

        let styles = if include_legacy_styles {
            style_types
                .iter()
                .map(|style_type| self.plugin_manager.create_style_instance(style_type))
                .collect::<Result<Vec<_>, LibraryError>>()?
        } else {
            Vec::new()
        };

        let mut node = Node::new(name, NodeContent::Generator(content));
        node.properties = properties;
        node.styles = styles;
        Ok(node)
    }

    /// Builds a detached Fill -> Text graph. The consumer has no embedded
    /// styles; graph wiring is the only Style authority for this factory.
    pub fn create_text_graph(
        &self,
        text: &str,
        font: &str,
        canvas_width: u64,
        canvas_height: u64,
    ) -> Result<NodeGraphBundle, LibraryError> {
        let (text_width, text_height) = measure_text_size(text, font, 100.0);
        let mut text_node = self.create_generator_node_with_style_mode(
            GeneratorNodeRequest::Text {
                text: text.to_string(),
                font: font.to_string(),
            },
            canvas_width,
            canvas_height,
            text_width as u64,
            text_height as u64,
            false,
        )?;
        let mut fill_node = self.plugin_manager.create_style_operation_node("fill")?;
        fill_node.ui_position = [0.0, 0.0];
        text_node.ui_position = [360.0, 0.0];
        let output_node_id = text_node.id;
        let connection = ProjectConnection::new(
            PortAddress::new(PortOwner::Node(fill_node.id), STYLE_OUTPUT_PORT),
            PortAddress::new(PortOwner::Node(text_node.id), STYLES_INPUT_PORT),
            0,
        );
        Ok(NodeGraphBundle::new(
            vec![fill_node, text_node],
            vec![connection],
            Some(output_node_id),
        ))
    }

    /// Builds detached Fill/Stroke -> Shape graph wiring. ProjectConnection
    /// order, not vector or UI pin order, defines Style application order.
    pub fn create_shape_graph(
        &self,
        path: &str,
        canvas_width: u64,
        canvas_height: u64,
        shape_width: u64,
        shape_height: u64,
    ) -> Result<NodeGraphBundle, LibraryError> {
        let mut shape_node = self.create_generator_node_with_style_mode(
            GeneratorNodeRequest::Shape {
                path: path.to_string(),
            },
            canvas_width,
            canvas_height,
            shape_width,
            shape_height,
            false,
        )?;
        let mut fill_node = self.plugin_manager.create_style_operation_node("fill")?;
        let mut stroke_node = self.plugin_manager.create_style_operation_node("stroke")?;
        fill_node.ui_position = [0.0, 0.0];
        stroke_node.ui_position = [0.0, 220.0];
        shape_node.ui_position = [360.0, 110.0];
        let output_node_id = shape_node.id;
        let connections = [(&fill_node, 0), (&stroke_node, 1)]
            .into_iter()
            .map(|(style, order)| {
                ProjectConnection::new(
                    PortAddress::new(PortOwner::Node(style.id), STYLE_OUTPUT_PORT),
                    PortAddress::new(PortOwner::Node(shape_node.id), STYLES_INPUT_PORT),
                    order,
                )
            })
            .collect();
        Ok(NodeGraphBundle::new(
            vec![fill_node, stroke_node, shape_node],
            connections,
            Some(output_node_id),
        ))
    }

    pub fn create_text_node(
        &self,
        text: &str,
        font: &str,
        canvas_width: u64,
        canvas_height: u64,
    ) -> Result<Node, LibraryError> {
        let (text_width, text_height) = measure_text_size(text, font, 100.0);
        self.create_generator_node(
            GeneratorNodeRequest::Text {
                text: text.to_string(),
                font: font.to_string(),
            },
            canvas_width,
            canvas_height,
            text_width as u64,
            text_height as u64,
        )
    }

    pub fn create_shape_node(
        &self,
        path: &str,
        canvas_width: u64,
        canvas_height: u64,
        shape_width: u64,
        shape_height: u64,
    ) -> Result<Node, LibraryError> {
        self.create_generator_node(
            GeneratorNodeRequest::Shape {
                path: path.to_string(),
            },
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
        self.create_generator_node(
            GeneratorNodeRequest::SkSL {
                shader: shader.to_string(),
            },
            canvas_width,
            canvas_height,
            canvas_width,
            canvas_height,
        )
    }

    pub fn create_solid_node(
        &self,
        color: Color,
        canvas_width: u64,
        canvas_height: u64,
    ) -> Result<Node, LibraryError> {
        self.create_generator_node(
            GeneratorNodeRequest::Solid { color },
            canvas_width,
            canvas_height,
            canvas_width,
            canvas_height,
        )
    }

    pub fn create_audio_clip(
        &self,
        reference_id: Uuid,
        file_path: &str,
        start_time: f64,
        duration: f64,
        source_start_time: f64,
        speed: f64,
    ) -> Result<ClipBundle, LibraryError> {
        let mut clip = Clip::new("Audio Clip", start_time, duration);
        clip.update_timing_property(
            crate::model::node::CLIP_START_TIME_PROPERTY,
            PropertyValue::Number(OrderedFloat(start_time)),
        )
        .map_err(LibraryError::Project)?;
        clip.update_timing_property(
            crate::model::node::CLIP_DURATION_PROPERTY,
            PropertyValue::Number(OrderedFloat(duration)),
        )
        .map_err(LibraryError::Project)?;
        clip.update_timing_property(
            crate::model::node::CLIP_TRIM_IN_PROPERTY,
            PropertyValue::Number(OrderedFloat(source_start_time)),
        )
        .map_err(LibraryError::Project)?;
        clip.update_timing_property(
            crate::model::node::CLIP_TIME_STRETCH_PROPERTY,
            PropertyValue::Number(OrderedFloat(speed)),
        )
        .map_err(LibraryError::Project)?;
        let mut node = Node::new(
            "Audio",
            NodeContent::Media(MediaContent {
                asset_id: reference_id,
                stream_index: None,
                audio_stream_index: None,
            }),
        );

        // Properties
        let defs = self
            .plugin_manager
            .get_entity_converter("audio")
            .map(|p| p.get_property_definitions(0, 0, 0, 0)) // Dimensions irrelevant for audio
            .unwrap_or_default();
        node.properties = crate::model::property::PropertyMap::from_definitions(&defs);
        node.properties.set(
            "file_path".to_string(),
            crate::model::property::Property::constant(PropertyValue::String(
                file_path.to_string(),
            )),
        );

        Ok(ClipBundle::with_primary_node(clip, node))
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "detached video Clip creation requires source timing and canvas dimensions atomically"
    )]
    pub fn create_video_clip(
        &self,
        reference_id: Uuid, // Required for Media
        file_path: &str,
        start_time: f64,
        duration: f64,
        source_start_time: f64,
        speed: f64,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Result<ClipBundle, LibraryError> {
        let plugin = self
            .plugin_manager
            .get_entity_converter("video")
            .ok_or_else(|| LibraryError::Plugin("Video converter plugin not found".to_string()))?;

        // Calculate media dimensions (placeholder or fetch from asset if available via reference_id? For now just use defaults or props)
        // Ideally we fetch asset metadata, but avoiding async or lock here if possible. ProjectService usually has asset info.
        let media_width = canvas_width as u64; // Fallback
        let media_height = canvas_height as u64;

        let defs = plugin.get_property_definitions(
            canvas_width as u64,
            canvas_height as u64,
            media_width,
            media_height,
        );
        let mut props = crate::model::property::PropertyMap::from_definitions(&defs);

        props.set(
            "file_path".to_string(),
            crate::model::property::Property::constant(PropertyValue::String(
                file_path.to_string(),
            )),
        );

        let mut clip = Clip::new("Video Clip", start_time, duration);
        clip.update_timing_property(
            crate::model::node::CLIP_START_TIME_PROPERTY,
            PropertyValue::Number(OrderedFloat(start_time)),
        )
        .map_err(LibraryError::Project)?;
        clip.update_timing_property(
            crate::model::node::CLIP_DURATION_PROPERTY,
            PropertyValue::Number(OrderedFloat(duration)),
        )
        .map_err(LibraryError::Project)?;
        clip.update_timing_property(
            crate::model::node::CLIP_TRIM_IN_PROPERTY,
            PropertyValue::Number(OrderedFloat(source_start_time)),
        )
        .map_err(LibraryError::Project)?;
        clip.update_timing_property(
            crate::model::node::CLIP_TIME_STRETCH_PROPERTY,
            PropertyValue::Number(OrderedFloat(speed)),
        )
        .map_err(LibraryError::Project)?;
        let mut node = Node::new(
            "Video",
            NodeContent::Media(MediaContent {
                asset_id: reference_id,
                stream_index: None,
                audio_stream_index: None,
            }),
        );
        node.properties = props;

        Ok(ClipBundle::with_primary_node(clip, node))
    }

    pub fn create_image_clip(
        &self,
        reference_id: Uuid,
        file_path: &str,
        start_time: f64,
        duration: f64,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Result<ClipBundle, LibraryError> {
        let plugin = self
            .plugin_manager
            .get_entity_converter("image")
            .ok_or_else(|| LibraryError::Plugin("Image converter plugin not found".to_string()))?;

        let defs = plugin.get_property_definitions(
            canvas_width as u64,
            canvas_height as u64,
            canvas_width as u64,
            canvas_height as u64,
        );
        let mut props = crate::model::property::PropertyMap::from_definitions(&defs);

        props.set(
            "file_path".to_string(),
            crate::model::property::Property::constant(PropertyValue::String(
                file_path.to_string(),
            )),
        );

        let mut node = Node::new(
            "Image",
            NodeContent::Media(MediaContent {
                asset_id: reference_id,
                stream_index: None,
                audio_stream_index: None,
            }),
        );
        node.properties = props;

        Ok(ClipBundle::with_primary_node(
            Clip::new("Image Clip", start_time, duration),
            node,
        ))
    }

    pub fn create_text_clip(
        &self,
        text: &str,
        start_time: f64,
        duration: f64,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Result<ClipBundle, LibraryError> {
        let graph = self.create_text_graph(
            text,
            DEFAULT_TEXT_FONT,
            u64::from(canvas_width),
            u64::from(canvas_height),
        )?;

        Ok(ClipBundle {
            clip: Clip::new("Text Clip", start_time, duration),
            graph,
        })
    }

    pub fn create_shape_clip(
        &self,
        start_time: f64,
        duration: f64,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Result<ClipBundle, LibraryError> {
        let graph = self.create_shape_graph(
            DEFAULT_SHAPE_PATH,
            u64::from(canvas_width),
            u64::from(canvas_height),
            100,
            100,
        )?;

        Ok(ClipBundle {
            clip: Clip::new("Shape Clip", start_time, duration),
            graph,
        })
    }

    pub fn create_sksl_clip(
        &self,
        start_time: f64,
        duration: f64,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Result<ClipBundle, LibraryError> {
        let node = self.create_sksl_node(
            DEFAULT_SKSL_SHADER,
            u64::from(canvas_width),
            u64::from(canvas_height),
        )?;

        Ok(ClipBundle::with_primary_node(
            Clip::new("SkSL Clip", start_time, duration),
            node,
        ))
    }

    pub fn create_reference_clip(
        &self,
        target_node_id: Uuid,
        start_time: f64,
        duration: f64,
    ) -> Result<ClipBundle, LibraryError> {
        let node = Node::new(
            "Reference",
            NodeContent::Reference(ReferenceContent {
                target_id: target_node_id,
                sync_global_time: false,
            }),
        );
        Ok(ClipBundle::with_primary_node(
            Clip::new("Reference Clip", start_time, duration),
            node,
        ))
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

        let media_node_ids: Vec<Uuid> = project_write
            .nodes
            .values()
            .filter_map(|node| match &node.content {
                NodeContent::Media(media) if media.asset_id == asset_id => Some(node.id),
                _ => None,
            })
            .collect();
        let clip_ids_to_remove: std::collections::HashSet<_> = media_node_ids
            .iter()
            .filter_map(|node_id| project_write.find_parent_clip(*node_id))
            .collect();
        for clip_id in clip_ids_to_remove {
            project_write.remove_clip(clip_id);
        }
        for node_id in media_node_ids {
            project_write.remove_node(node_id);
        }

        // Remove the asset itself
        project_write.assets.retain(|a| a.id != asset_id);
        Ok(())
    }

    pub fn remove_composition_fully(&self, comp_id: Uuid) -> Result<(), LibraryError> {
        let mut project_write = self.project.write().map_err(|e| {
            LibraryError::Runtime(format!("Failed to acquire project write lock: {}", e))
        })?;

        project_write
            .remove_composition(comp_id)
            .map(|_| ())
            .ok_or_else(|| {
                LibraryError::Project(format!("Composition with ID {} not found", comp_id))
            })
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

                let mut asset = crate::model::asset::Asset::new(&name, path, stream.kind);
                asset.duration = stream.duration;
                asset.fps = stream.fps;
                asset.width = stream.width;
                asset.height = stream.height;
                asset.stream_index = stream.stream_index;
                if asset.kind == crate::model::asset::AssetKind::Video {
                    asset.frame_count = stream.frame_count;
                }

                assets_to_add.push(asset);
            }
        }

        // 2. Fallback if no streams returned (or empty list)
        if assets_to_add.is_empty() {
            // 1. Get Metadata (Single call)
            let (mut kind, duration, fps, width, height, frame_count) =
                if let Some(meta) = self.plugin_manager.get_metadata(path) {
                    (
                        meta.kind,
                        meta.duration,
                        meta.fps,
                        meta.width,
                        meta.height,
                        meta.frame_count,
                    )
                } else {
                    (
                        crate::model::asset::AssetKind::Other,
                        None,
                        None,
                        None,
                        None,
                        None,
                    )
                };

            // 2. Fallback for Kind if Unknown
            if kind == crate::model::asset::AssetKind::Other {
                // Fallback to extension if plugin didn't detect it
                let ext = path_obj
                    .extension()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase();
                kind = match ext.as_str() {
                    "mp4" | "mov" | "avi" | "mkv" | "webm" => crate::model::asset::AssetKind::Video,
                    "png" | "jpg" | "jpeg" | "bmp" | "webp" => {
                        crate::model::asset::AssetKind::Image
                    }
                    "mp3" | "wav" | "ogg" | "aac" | "flac" => crate::model::asset::AssetKind::Audio,
                    "obj" | "gltf" | "glb" => crate::model::asset::AssetKind::Model3D,
                    _ => crate::model::asset::AssetKind::Other,
                };
            }

            // 3. Create Asset
            let mut asset = crate::model::asset::Asset::new(&base_name, path, kind);
            asset.duration = duration;
            asset.fps = fps;
            asset.width = width;
            asset.height = height;
            if asset.kind == crate::model::asset::AssetKind::Video {
                asset.frame_count = frame_count;
            }
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

    pub fn rename_track(&self, track_id: Uuid, new_name: &str) -> Result<(), LibraryError> {
        handlers::track_handler::TrackHandler::rename_track(&self.project, track_id, new_name)
    }

    pub fn move_track_within_composition(
        &self,
        composition_id: Uuid,
        track_id: Uuid,
        destination_index: usize,
    ) -> Result<bool, LibraryError> {
        handlers::track_handler::TrackHandler::move_track_within_composition(
            &self.project,
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
        handlers::clip_handler::ClipHandler::add_clip_to_track(
            &self.project,
            composition_id,
            track_id,
            bundle,
            insert_index,
        )
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

    pub fn update_property_or_keyframe(
        &self,
        owner: PropertyOwner,
        target: crate::model::property::PropertyTarget,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        handlers::clip_handler::ClipHandler::update_target_property_or_keyframe(
            &self.project,
            owner,
            target,
            property_key,
            time,
            value,
            easing,
        )
    }

    pub fn update_keyframe_by_id(
        &self,
        owner: PropertyOwner,
        target: crate::model::property::PropertyTarget,
        property_key: &str,
        keyframe_id: KeyframeId,
        update: KeyframeUpdate,
    ) -> Result<(), LibraryError> {
        handlers::keyframe_handler::KeyframeHandler::update_keyframe_by_id(
            &self.project,
            owner,
            target,
            property_key,
            keyframe_id,
            update,
        )
    }

    pub fn update_keyframes_batch(
        &self,
        updates: &[handlers::keyframe_handler::KeyframeBatchUpdate],
    ) -> Result<(), LibraryError> {
        handlers::keyframe_handler::KeyframeHandler::update_keyframes_batch(&self.project, updates)
    }

    pub fn remove_keyframe_by_id(
        &self,
        owner: PropertyOwner,
        target: crate::model::property::PropertyTarget,
        property_key: &str,
        keyframe_id: KeyframeId,
    ) -> Result<(), LibraryError> {
        handlers::keyframe_handler::KeyframeHandler::remove_keyframe_by_id(
            &self.project,
            owner,
            target,
            property_key,
            keyframe_id,
        )
    }

    pub fn update_clip_timing(
        &self,
        clip_id: Uuid,
        start_time: f64,
        duration: f64,
        trim_in: f64,
    ) -> Result<(), LibraryError> {
        handlers::clip_handler::ClipHandler::update_clip_timing(
            &self.project,
            clip_id,
            start_time,
            duration,
            trim_in,
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
        property: &crate::model::property::Property,
        context: &crate::model::property::PropertyMap,
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

    pub fn add_keyframe(
        &self,
        owner: PropertyOwner,
        target: crate::model::property::PropertyTarget,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), LibraryError> {
        handlers::keyframe_handler::KeyframeHandler::add_keyframe(
            &self.project,
            owner,
            target,
            property_key,
            time,
            value,
            easing,
        )
    }

    pub fn add_keyframe_with_id(
        &self,
        owner: PropertyOwner,
        target: crate::model::property::PropertyTarget,
        property_key: &str,
        time: f64,
        value: PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<KeyframeId, LibraryError> {
        handlers::keyframe_handler::KeyframeHandler::add_keyframe_with_id(
            &self.project,
            owner,
            target,
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
        new_start_time: f64,
    ) -> Result<(), LibraryError> {
        handlers::clip_handler::ClipHandler::move_clip_to_track(
            &self.project,
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
        handlers::clip_handler::ClipHandler::move_clip_to_track_at_index(
            &self.project,
            composition_id,
            source_track_id,
            clip_id,
            target_track_id,
            new_start_time,
            target_index,
        )
    }

    pub fn add_effect(&self, owner: PropertyOwner, effect_id: &str) -> Result<(), LibraryError> {
        let effect_config = self
            .plugin_manager
            .get_default_effect_config(effect_id)
            .ok_or_else(|| {
                LibraryError::Project(format!(
                    "Default config for effect '{}' not found",
                    effect_id
                ))
            })?;

        handlers::clip_handler::ClipHandler::add_effect(&self.project, owner, effect_config)
    }

    pub fn update_effects(
        &self,
        owner: PropertyOwner,
        effects: Vec<EffectConfig>,
    ) -> Result<(), LibraryError> {
        handlers::clip_handler::ClipHandler::update_effects(&self.project, owner, effects)
    }

    pub fn update_node_styles(
        &self,
        node_id: Uuid,
        styles: Vec<crate::model::style::StyleInstance>,
    ) -> Result<(), LibraryError> {
        handlers::clip_handler::ClipHandler::update_node_styles(&self.project, node_id, styles)
    }

    pub fn add_effector(&self, node_id: Uuid, effector_type: &str) -> Result<(), LibraryError> {
        let mut effector = self
            .plugin_manager
            .create_effector_operation_node(effector_type)?;
        let mut project = self
            .project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let consumer = project
            .get_node(node_id)
            .ok_or_else(|| LibraryError::Project(format!("Node {node_id} not found")))?;
        effector.ui_position = [consumer.ui_position[0] - 360.0, consumer.ui_position[1]];
        let container = project.find_node_container(node_id).ok_or_else(|| {
            LibraryError::Project(format!("Node {node_id} has no containing graph"))
        })?;
        let target = PortAddress::new(PortOwner::Node(node_id), EFFECTORS_INPUT_PORT);
        let order = project
            .connections
            .iter()
            .filter(|connection| connection.to == target)
            .map(|connection| connection.order)
            .max()
            .unwrap_or(-1)
            + 1;
        let connection = ProjectConnection::new(
            PortAddress::new(PortOwner::Node(effector.id), EFFECTOR_OUTPUT_PORT),
            target,
            order,
        );
        project
            .insert_node_graph(
                container,
                NodeGraphBundle::new(vec![effector], vec![connection], None),
            )
            .map_err(|error| LibraryError::Project(error.to_string()))
    }

    pub fn update_node_effectors(
        &self,
        node_id: Uuid,
        mut effectors: Vec<crate::model::ensemble::EffectorInstance>,
    ) -> Result<(), LibraryError> {
        for effector in &mut effectors {
            let _known_plugin = self.plugin_manager.complete_effector_instance(effector);
        }
        handlers::clip_handler::ClipHandler::update_node_effectors(
            &self.project,
            node_id,
            effectors,
        )
    }

    pub fn add_decorator(&self, node_id: Uuid, decorator_type: &str) -> Result<(), LibraryError> {
        let mut decorator = self
            .plugin_manager
            .create_decorator_operation_node(decorator_type)?;
        let mut project = self
            .project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let consumer = project
            .get_node(node_id)
            .ok_or_else(|| LibraryError::Project(format!("Node {node_id} not found")))?;
        decorator.ui_position = [consumer.ui_position[0] - 360.0, consumer.ui_position[1]];
        let container = project.find_node_container(node_id).ok_or_else(|| {
            LibraryError::Project(format!("Node {node_id} has no containing graph"))
        })?;
        let target = PortAddress::new(PortOwner::Node(node_id), DECORATORS_INPUT_PORT);
        let order = project
            .connections
            .iter()
            .filter(|connection| connection.to == target)
            .map(|connection| connection.order)
            .max()
            .unwrap_or(-1)
            + 1;
        let connection = ProjectConnection::new(
            PortAddress::new(PortOwner::Node(decorator.id), DECORATOR_OUTPUT_PORT),
            target,
            order,
        );
        project
            .insert_node_graph(
                container,
                NodeGraphBundle::new(vec![decorator], vec![connection], None),
            )
            .map_err(|error| LibraryError::Project(error.to_string()))
    }

    pub fn update_node_decorators(
        &self,
        node_id: Uuid,
        mut decorators: Vec<crate::model::ensemble::DecoratorInstance>,
    ) -> Result<(), LibraryError> {
        for decorator in &mut decorators {
            let _known_plugin = self.plugin_manager.complete_decorator_instance(decorator);
        }
        handlers::clip_handler::ClipHandler::update_node_decorators(
            &self.project,
            node_id,
            decorators,
        )
    }

    pub fn set_property_attribute(
        &self,
        owner: PropertyOwner,
        target: crate::model::property::PropertyTarget,
        property_key: &str,
        attribute_key: &str,
        attribute_value: PropertyValue,
    ) -> Result<(), LibraryError> {
        handlers::clip_handler::ClipHandler::set_property_attribute(
            &self.project,
            owner,
            target,
            property_key,
            attribute_key,
            attribute_value,
        )
    }

    pub fn add_style_to_node(&self, node_id: Uuid, style_type: &str) -> Result<(), LibraryError> {
        let plugin = self
            .plugin_manager
            .get_style_plugin(style_type)
            .ok_or_else(|| {
                LibraryError::Plugin(format!("Style plugin '{}' not found", style_type))
            })?;

        let properties =
            crate::model::property::PropertyMap::from_definitions(&plugin.properties());

        let instance = crate::model::style::StyleInstance::new(style_type, properties);
        let mut project = self
            .project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        project
            .get_node_mut(node_id)
            .ok_or_else(|| LibraryError::Validation(format!("Node {node_id} not found")))?
            .styles
            .push(instance);
        Ok(())
    }

    pub fn get_inspector_definitions(
        &self,
        comp_id: uuid::Uuid,
        _track_id: uuid::Uuid,
        node_id: uuid::Uuid,
    ) -> Vec<PropertyDefinition> {
        let project = self
            .project
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        let (node, canvas_width, canvas_height) =
            if let Some(comp) = project.compositions.iter().find(|c| c.id == comp_id) {
                if let Some(node) = project.get_node(node_id) {
                    (node.clone(), comp.width, comp.height)
                } else {
                    return Vec::new();
                }
            } else {
                return Vec::new();
            };

        // Resolve clip dimensions
        let (clip_width, clip_height): (u64, u64) = match &node.content {
            NodeContent::Media(m) => {
                // If asset is loaded, get dimensions
                if let Some(asset) = project.assets.iter().find(|a| a.id == m.asset_id) {
                    (
                        asset.width.unwrap_or(100) as u64,
                        asset.height.unwrap_or(100) as u64,
                    )
                } else {
                    (100, 100)
                }
            }
            NodeContent::Generator(GeneratorContent::Shape) => {
                let w = node.properties.get_f64("width").unwrap_or(100.0) as u64;
                let h = node.properties.get_f64("height").unwrap_or(100.0) as u64;
                (w, h)
            }
            NodeContent::Generator(GeneratorContent::Text) => {
                let size = node.properties.get_f64("size").unwrap_or(100.0) as f32;
                let text = node.properties.get_string("text").unwrap_or_default();
                let font = node
                    .properties
                    .get_string("font_family")
                    .unwrap_or_else(|| DEFAULT_TEXT_FONT.to_string());
                let (w, h) = measure_text_size(&text, &font, size);
                (w.round() as u64, h.round() as u64)
            }
            NodeContent::Generator(GeneratorContent::SkSL) => (canvas_width, canvas_height),
            _ => (100, 100),
        };

        // Key for entity converter: "video", "image", "text", "shape", "sksl"
        // In Trinity, LayerContent doesn't store "Kind" string.
        // We infer key from content.
        let kind_key = match &node.content {
            NodeContent::Media(m) => {
                if let Some(asset) = project.assets.iter().find(|a| a.id == m.asset_id) {
                    match asset.kind {
                        crate::model::asset::AssetKind::Video => "video",
                        crate::model::asset::AssetKind::Image => "image",
                        crate::model::asset::AssetKind::Audio => "audio",
                        _ => "unknown",
                    }
                } else {
                    "video" // default fallback?
                }
            }
            NodeContent::Generator(GeneratorContent::Shape) => "shape",
            NodeContent::Generator(GeneratorContent::Text) => "text",
            NodeContent::Generator(GeneratorContent::SkSL) => "sksl",
            NodeContent::Generator(GeneratorContent::Solid) => "solid",
            _ => "unknown",
        };

        let converter = self.plugin_manager.get_entity_converter(kind_key);

        let mut definitions = if let Some(converter) = converter {
            converter.get_property_definitions(canvas_width, canvas_height, clip_width, clip_height)
        } else {
            Vec::new()
        };

        if kind_key == "video" {
            let colorspaces =
                crate::editor::color_service::ColorSpaceManager::get_available_colorspaces();
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

        definitions
    }
}

#[cfg(test)]
mod keyframe_tests {
    use super::*;
    use crate::editor::handlers::property_ops::PropertyOwner;
    use crate::model::property::{Property, PropertyMap, PropertyTarget, PropertyValue};
    use crate::model::style::StyleInstance;
    use crate::model::{DecoratorInstance, EffectorInstance};

    fn labelled_properties(label: &str) -> PropertyMap {
        let mut properties = PropertyMap::new();
        properties.set(
            "amount".to_string(),
            Property::constant(PropertyValue::String(label.to_string())),
        );
        properties
    }

    fn effect(label: &str) -> EffectConfig {
        EffectConfig {
            id: Uuid::new_v4(),
            effect_type: label.to_string(),
            properties: labelled_properties(label),
        }
    }

    fn assert_converter_properties(
        manager: &ProjectManager,
        node: &Node,
        converter_kind: &str,
        dimensions: (u64, u64, u64, u64),
    ) {
        let Some(converter) = manager.plugin_manager.get_entity_converter(converter_kind) else {
            panic!("{converter_kind} converter should be registered");
        };
        let definitions = converter.get_property_definitions(
            dimensions.0,
            dimensions.1,
            dimensions.2,
            dimensions.3,
        );
        for definition in definitions {
            assert!(
                node.properties.get(definition.name()).is_some(),
                "{} factory omitted converter property {}",
                converter_kind,
                definition.name()
            );
        }
    }

    fn assert_property_value(node: &Node, key: &str, expected: PropertyValue) {
        assert_eq!(
            node.properties.get(key).and_then(Property::value).cloned(),
            Some(expected),
            "authoritative property {key} must match GeneratorContent"
        );
    }

    fn assert_default_styles(manager: &ProjectManager, node: &Node, expected_types: &[&str]) {
        assert_eq!(node.styles.len(), expected_types.len());
        for (style, expected_type) in node.styles.iter().zip(expected_types) {
            assert_eq!(&style.style_type, expected_type);
            let Some(plugin) = manager.plugin_manager.get_style_plugin(expected_type) else {
                panic!("{expected_type} style should be registered");
            };
            assert_eq!(
                style.properties,
                PropertyMap::from_definitions(&plugin.properties())
            );
        }
    }

    #[test]
    fn project_manager_exposes_identity_based_keyframe_edits() {
        let mut project = Project::new("keyframe service");
        let node = Node::new("solid", NodeContent::Generator(GeneratorContent::Solid));
        let node_id = node.id;
        project.add_node(node);
        let shared = Arc::new(RwLock::new(project));
        let manager = ProjectManager::new(Arc::clone(&shared), Arc::new(PluginManager::default()));
        let owner = PropertyOwner::Node(node_id);

        let id = manager
            .add_keyframe_with_id(
                owner,
                PropertyTarget::Direct,
                "opacity",
                1.0,
                PropertyValue::Number(OrderedFloat(100.0)),
                Some(crate::animation::EasingFunction::EaseOutQuad),
            )
            .expect("service should promote a missing property");
        manager
            .update_keyframe_by_id(
                owner,
                PropertyTarget::Direct,
                "opacity",
                id,
                KeyframeUpdate {
                    time: Some(2.0),
                    value: Some(PropertyValue::Number(OrderedFloat(75.0))),
                    ..Default::default()
                },
            )
            .expect("service should update the identified key");

        {
            let read = shared.read().expect("project should remain readable");
            let keyframe = read
                .get_node(node_id)
                .and_then(|node| node.properties.get("opacity"))
                .and_then(|property| property.keyframe_by_id(id))
                .expect("identified key should exist");
            assert_eq!(keyframe.time, OrderedFloat(2.0));
            assert_eq!(keyframe.value, PropertyValue::Number(OrderedFloat(75.0)));
            assert_eq!(
                keyframe.easing,
                crate::animation::EasingFunction::EaseOutQuad
            );
        }

        let saved = manager.save_project().expect("project should serialize");
        let loaded = Project::load(&saved).expect("project should deserialize");
        let loaded_keyframe = loaded
            .get_node(node_id)
            .and_then(|node| node.properties.get("opacity"))
            .and_then(|property| property.keyframe_by_id(id))
            .expect("save/load should preserve keyframe identity");
        assert_eq!(loaded_keyframe.time, OrderedFloat(2.0));
        assert_eq!(
            loaded_keyframe.easing,
            crate::animation::EasingFunction::EaseOutQuad
        );

        manager
            .remove_keyframe_by_id(owner, PropertyTarget::Direct, "opacity", id)
            .expect("service should remove the identified key");
        let read = shared.read().expect("project should remain readable");
        let property = read
            .get_node(node_id)
            .and_then(|node| node.properties.get("opacity"))
            .expect("property should remain as a constant");
        assert_eq!(property.evaluator, "constant");
        assert_eq!(
            property.value(),
            Some(&PropertyValue::Number(OrderedFloat(75.0)))
        );
    }

    #[test]
    fn reordered_nested_property_targets_keep_editing_the_same_instance_id() {
        let mut project = Project::new("nested property identity");
        let mut node = Node::new("solid", NodeContent::Generator(GeneratorContent::Solid));
        let node_id = node.id;

        let selected_effect = effect("selected effect");
        let selected_effect_id = selected_effect.id;
        node.effects = vec![selected_effect, effect("other effect")];

        let selected_style =
            StyleInstance::new("selected style", labelled_properties("selected style"));
        let selected_style_id = selected_style.id;
        node.styles = vec![
            selected_style,
            StyleInstance::new("other style", labelled_properties("other style")),
        ];

        let selected_effector = EffectorInstance::new(
            "selected effector",
            labelled_properties("selected effector"),
        );
        let selected_effector_id = selected_effector.id;
        node.effectors = vec![
            selected_effector,
            EffectorInstance::new("other effector", labelled_properties("other effector")),
        ];

        let selected_decorator = DecoratorInstance::new(
            "selected decorator",
            labelled_properties("selected decorator"),
        );
        let selected_decorator_id = selected_decorator.id;
        node.decorators = vec![
            selected_decorator,
            DecoratorInstance::new("other decorator", labelled_properties("other decorator")),
        ];

        node.effects.reverse();
        node.styles.reverse();
        node.effectors.reverse();
        node.decorators.reverse();
        project.add_node(node);

        let mut clip = Clip::new("clip", 0.0, 1.0);
        let clip_id = clip.id;
        let selected_clip_effect = effect("selected clip effect");
        let selected_clip_effect_id = selected_clip_effect.id;
        clip.effects = vec![selected_clip_effect, effect("other clip effect")];
        clip.effects.reverse();
        project.add_clip(clip);

        let shared = Arc::new(RwLock::new(project));
        let manager = ProjectManager::new(Arc::clone(&shared), Arc::new(PluginManager::default()));
        let updates = [
            (
                PropertyOwner::Node(node_id),
                PropertyTarget::Effect(selected_effect_id),
                "updated effect",
            ),
            (
                PropertyOwner::Node(node_id),
                PropertyTarget::Style(selected_style_id),
                "updated style",
            ),
            (
                PropertyOwner::Node(node_id),
                PropertyTarget::Effector(selected_effector_id),
                "updated effector",
            ),
            (
                PropertyOwner::Node(node_id),
                PropertyTarget::Decorator(selected_decorator_id),
                "updated decorator",
            ),
            (
                PropertyOwner::Clip(clip_id),
                PropertyTarget::Effect(selected_clip_effect_id),
                "updated clip effect",
            ),
        ];

        for (owner, target, value) in updates {
            assert!(
                manager
                    .update_property_or_keyframe(
                        owner,
                        target,
                        "amount",
                        0.0,
                        PropertyValue::String(value.to_string()),
                        None,
                    )
                    .is_ok(),
                "{target:?} should resolve by persistent id after reorder"
            );
        }

        let read = shared
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let owner_targets = [
            (
                PropertyOwner::Node(node_id),
                PropertyTarget::Effect(selected_effect_id),
                "updated effect",
            ),
            (
                PropertyOwner::Node(node_id),
                PropertyTarget::Style(selected_style_id),
                "updated style",
            ),
            (
                PropertyOwner::Node(node_id),
                PropertyTarget::Effector(selected_effector_id),
                "updated effector",
            ),
            (
                PropertyOwner::Node(node_id),
                PropertyTarget::Decorator(selected_decorator_id),
                "updated decorator",
            ),
            (
                PropertyOwner::Clip(clip_id),
                PropertyTarget::Effect(selected_clip_effect_id),
                "updated clip effect",
            ),
        ];
        for (owner, target, expected) in owner_targets {
            let properties =
                crate::editor::handlers::property_ops::property_map(&read, owner, target);
            let Ok(properties) = properties else {
                panic!("{target:?} should remain resolvable after reorder");
            };
            assert_eq!(
                properties.get("amount").and_then(Property::value),
                Some(&PropertyValue::String(expected.to_string()))
            );
        }
    }

    #[test]
    fn missing_nested_instance_ids_are_explicit_errors() {
        let mut project = Project::new("missing nested targets");
        let node = Node::new("solid", NodeContent::Generator(GeneratorContent::Solid));
        let node_id = node.id;
        project.add_node(node);
        let shared = Arc::new(RwLock::new(project));
        let manager = ProjectManager::new(Arc::clone(&shared), Arc::new(PluginManager::default()));

        let missing_ids = [
            (Uuid::new_v4(), "effect"),
            (Uuid::new_v4(), "style"),
            (Uuid::new_v4(), "effector"),
            (Uuid::new_v4(), "decorator"),
        ];
        let targets = [
            PropertyTarget::Effect(missing_ids[0].0),
            PropertyTarget::Style(missing_ids[1].0),
            PropertyTarget::Effector(missing_ids[2].0),
            PropertyTarget::Decorator(missing_ids[3].0),
        ];

        for ((missing_id, kind), target) in missing_ids.into_iter().zip(targets) {
            let result = manager.update_property_or_keyframe(
                PropertyOwner::Node(node_id),
                target,
                "amount",
                0.0,
                PropertyValue::String("must not be written".to_string()),
                None,
            );
            let Err(LibraryError::Project(message)) = result else {
                panic!("missing {kind} id should return a project error");
            };
            assert!(message.contains(&missing_id.to_string()));
            assert!(message.contains("not found"));
        }
    }

    #[test]
    fn generator_factories_materialize_every_converter_default_and_content_value() {
        let shared = Arc::new(RwLock::new(Project::new("generator factories")));
        let manager = ProjectManager::new(Arc::clone(&shared), Arc::new(PluginManager::default()));
        let canvas = (1920, 1080);

        let text = "Factory text";
        let font = "Arial";
        let (text_width, text_height) = measure_text_size(text, font, 100.0);
        let Ok(text_node) = manager.create_text_node(text, font, canvas.0, canvas.1) else {
            panic!("text node factory should succeed");
        };
        assert_eq!(
            text_node.content,
            NodeContent::Generator(GeneratorContent::Text)
        );
        assert_converter_properties(
            &manager,
            &text_node,
            "text",
            (canvas.0, canvas.1, text_width as u64, text_height as u64),
        );
        assert_property_value(&text_node, "text", PropertyValue::String(text.to_string()));
        assert_property_value(
            &text_node,
            "font_family",
            PropertyValue::String(font.to_string()),
        );
        assert_default_styles(&manager, &text_node, &["fill"]);

        let path = "M 0 0 H 120 V 80 H 0 Z";
        let Ok(shape_node) = manager.create_shape_node(path, canvas.0, canvas.1, 120, 80) else {
            panic!("shape node factory should succeed");
        };
        assert_eq!(
            shape_node.content,
            NodeContent::Generator(GeneratorContent::Shape)
        );
        assert_converter_properties(
            &manager,
            &shape_node,
            "shape",
            (canvas.0, canvas.1, 120, 80),
        );
        assert_property_value(&shape_node, "path", PropertyValue::String(path.to_string()));
        assert_default_styles(&manager, &shape_node, &["fill", "stroke"]);

        let shader = "half4 main(float2 p) { return half4(1); }";
        let Ok(sksl_node) = manager.create_sksl_node(shader, canvas.0, canvas.1) else {
            panic!("SkSL node factory should succeed");
        };
        assert_eq!(
            sksl_node.content,
            NodeContent::Generator(GeneratorContent::SkSL)
        );
        assert_converter_properties(
            &manager,
            &sksl_node,
            "sksl",
            (canvas.0, canvas.1, canvas.0, canvas.1),
        );
        assert_property_value(
            &sksl_node,
            "shader",
            PropertyValue::String(shader.to_string()),
        );
        assert_default_styles(&manager, &sksl_node, &[]);

        let color = Color {
            r: 12,
            g: 34,
            b: 56,
            a: 78,
        };
        let Ok(solid_node) = manager.create_solid_node(color.clone(), canvas.0, canvas.1) else {
            panic!("solid node factory should succeed");
        };
        assert_eq!(
            solid_node.content,
            NodeContent::Generator(GeneratorContent::Solid)
        );
        assert_converter_properties(
            &manager,
            &solid_node,
            "solid",
            (canvas.0, canvas.1, canvas.0, canvas.1),
        );
        assert_property_value(&solid_node, "color", PropertyValue::Color(color.clone()));
        assert_default_styles(&manager, &solid_node, &[]);

        for node in [&text_node, &shape_node, &sksl_node, &solid_node] {
            for required_transform in ["position", "scale", "rotation", "anchor", "opacity"] {
                assert!(
                    node.properties.get(required_transform).is_some(),
                    "{} omitted {required_transform}",
                    node.name
                );
            }
        }

        for (node, expected_kind) in [
            (&text_node, "Text"),
            (&shape_node, "Shape"),
            (&sksl_node, "SkSL"),
            (&solid_node, "Solid"),
        ] {
            let Ok(serialized) = serde_json::to_value(node) else {
                panic!("{} should serialize", node.name);
            };
            assert_eq!(serialized["content"]["data"], expected_kind);
            assert!(serialized["content"]["data"].is_string());
        }

        let text_id = text_node.id;
        let shape_id = shape_node.id;
        let sksl_id = sksl_node.id;
        let solid_id = solid_node.id;
        let mut project = Project::new("generator roundtrip");
        for node in [text_node, shape_node, sksl_node, solid_node] {
            project.add_node(node);
        }
        let Ok(saved) = project.save() else {
            panic!("generator project should save");
        };
        let Ok(loaded) = Project::load(&saved) else {
            panic!("generator project should load");
        };
        let loaded_expectations = [
            (text_id, "text", PropertyValue::String(text.to_string())),
            (shape_id, "path", PropertyValue::String(path.to_string())),
            (sksl_id, "shader", PropertyValue::String(shader.to_string())),
            (solid_id, "color", PropertyValue::Color(color)),
        ];
        for (node_id, property_key, expected) in loaded_expectations {
            let Some(node) = loaded.get_node(node_id) else {
                panic!("generator {node_id} should survive save/load");
            };
            assert_property_value(node, property_key, expected);
        }
    }

    #[test]
    fn generator_clip_factories_share_consumer_defaults_and_detach_style_authority() {
        let shared = Arc::new(RwLock::new(Project::new("shared generator factory")));
        let manager = ProjectManager::new(Arc::clone(&shared), Arc::new(PluginManager::default()));

        let Ok(direct_text) = manager.create_text_node("same", DEFAULT_TEXT_FONT, 640, 480) else {
            panic!("direct text factory should succeed");
        };
        let Ok(text_bundle) = manager.create_text_clip("same", 0.0, 1.0, 640, 480) else {
            panic!("text clip factory should succeed");
        };
        let Some(clip_text) = text_bundle.primary_node() else {
            panic!("text clip should have one output node");
        };
        assert_eq!(clip_text.content, direct_text.content);
        assert_eq!(clip_text.properties, direct_text.properties);
        assert!(clip_text.styles.is_empty());
        assert_eq!(direct_text.styles.len(), 1);
        assert_eq!(text_bundle.graph.nodes.len(), 2);
        assert_eq!(text_bundle.graph.connections.len(), 1);
        assert_eq!(text_bundle.graph.connections[0].order, 0);

        let Ok(direct_shape) = manager.create_shape_node(DEFAULT_SHAPE_PATH, 640, 480, 100, 100)
        else {
            panic!("direct shape factory should succeed");
        };
        let Ok(shape_bundle) = manager.create_shape_clip(0.0, 1.0, 640, 480) else {
            panic!("shape clip factory should succeed");
        };
        let Some(clip_shape) = shape_bundle.primary_node() else {
            panic!("shape clip should have one output node");
        };
        assert_eq!(clip_shape.content, direct_shape.content);
        assert_eq!(clip_shape.properties, direct_shape.properties);
        assert!(clip_shape.styles.is_empty());
        assert_eq!(direct_shape.styles.len(), 2);
        assert_eq!(shape_bundle.graph.nodes.len(), 3);
        assert_eq!(
            shape_bundle
                .graph
                .connections
                .iter()
                .map(|connection| connection.order)
                .collect::<Vec<_>>(),
            vec![0, 1]
        );

        let Ok(direct_sksl) = manager.create_sksl_node(DEFAULT_SKSL_SHADER, 640, 480) else {
            panic!("direct SkSL factory should succeed");
        };
        let Ok(sksl_bundle) = manager.create_sksl_clip(0.0, 1.0, 640, 480) else {
            panic!("SkSL clip factory should succeed");
        };
        let Some(clip_sksl) = sksl_bundle.primary_node() else {
            panic!("SkSL clip should have one output node");
        };
        assert_eq!(clip_sksl.content, direct_sksl.content);
        assert_eq!(clip_sksl.properties, direct_sksl.properties);
    }
}
