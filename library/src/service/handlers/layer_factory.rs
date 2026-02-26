use crate::error::LibraryError;
use crate::plugin::PluginManager;
use crate::project::project::Project;
use crate::project::property::{Property, PropertyMap, PropertyValue};
use crate::project::source::{SourceData, SourceKind};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

use super::source_handler::SourceHandler;

/// Factory for creating layers with their respective sources and graph nodes.
///
/// A "layer" in RuViE is a container (Node::Layer(LayerData)) holding a source node
/// along with its graph pipeline (transform, style.fill for text/shape, etc.).
pub struct LayerFactory;

impl LayerFactory {
    /// Create a complete layer with the given source, adding it to the project.
    ///
    /// This is the main entry point for adding a source to a track. It:
    /// 1. Adds the source node to the track
    /// 2. Creates a layer container with default graph nodes (transform, style.fill, etc.)
    /// 3. Wires up all connections
    ///
    /// Returns the source node ID.
    pub fn create_layer(
        project: &Arc<RwLock<Project>>,
        plugin_manager: &PluginManager,
        composition_id: Uuid,
        track_id: Uuid,
        source: SourceData,
        in_frame: u64,
        out_frame: u64,
        insert_index: Option<usize>,
    ) -> Result<Uuid, LibraryError> {
        let source_kind = source.kind.clone();
        let source_id = SourceHandler::add_source_to_track(
            project,
            composition_id,
            track_id,
            source,
            in_frame,
            out_frame,
            insert_index,
        )?;

        // Auto-create default graph nodes (transform, style.fill) for the new source
        if let Err(e) = SourceHandler::setup_source_graph_nodes(
            project,
            plugin_manager,
            track_id,
            source_id,
            &source_kind,
        ) {
            log::error!(
                "Failed to setup graph nodes for source {}. Rolling back: {}",
                source_id,
                e
            );
            let _ = SourceHandler::remove_source_from_track(project, track_id, source_id);
            return Err(e);
        }

        Ok(source_id)
    }

    // --- Source builders (pure data creation, no project mutation) ---

    pub fn build_audio_source(
        reference_id: Option<Uuid>,
        file_path: &str,
        in_frame: u64,
        out_frame: u64,
        source_begin_frame: i64,
        duration_frame: u64,
        fps: f64,
    ) -> SourceData {
        let defs = SourceData::get_definitions_for_kind(&SourceKind::Audio);
        let mut props = PropertyMap::from_definitions(&defs);

        props.set(
            "file_path".to_string(),
            Property::constant(PropertyValue::String(file_path.to_string())),
        );

        SourceData::new(
            Uuid::new_v4(),
            reference_id,
            SourceKind::Audio,
            in_frame,
            out_frame,
            source_begin_frame,
            Some(duration_frame),
            fps,
            props,
        )
    }

    pub fn build_video_source(
        reference_id: Option<Uuid>,
        file_path: &str,
        in_frame: u64,
        out_frame: u64,
        source_begin_frame: i64,
        duration_frame: u64,
        fps: f64,
    ) -> SourceData {
        let defs = SourceData::get_definitions_for_kind(&SourceKind::Video);
        let mut props = PropertyMap::from_definitions(&defs);

        props.set(
            "file_path".to_string(),
            Property::constant(PropertyValue::String(file_path.to_string())),
        );

        SourceData::new(
            Uuid::new_v4(),
            reference_id,
            SourceKind::Video,
            in_frame,
            out_frame,
            source_begin_frame,
            Some(duration_frame),
            fps,
            props,
        )
    }

    pub fn build_image_source(
        reference_id: Option<Uuid>,
        file_path: &str,
        in_frame: u64,
        out_frame: u64,
        fps: f64,
    ) -> SourceData {
        let defs = SourceData::get_definitions_for_kind(&SourceKind::Image);
        let mut props = PropertyMap::from_definitions(&defs);

        props.set(
            "file_path".to_string(),
            Property::constant(PropertyValue::String(file_path.to_string())),
        );

        SourceData::new(
            Uuid::new_v4(),
            reference_id,
            SourceKind::Image,
            in_frame,
            out_frame,
            0,
            None,
            fps,
            props,
        )
    }

    pub fn build_text_source(text: &str, in_frame: u64, out_frame: u64, fps: f64) -> SourceData {
        let defs = SourceData::get_definitions_for_kind(&SourceKind::Text);
        let mut props = PropertyMap::from_definitions(&defs);

        props.set(
            "text".to_string(),
            Property::constant(PropertyValue::String(text.to_string())),
        );

        SourceData::new(
            Uuid::new_v4(),
            None,
            SourceKind::Text,
            in_frame,
            out_frame,
            0,
            None,
            fps,
            props,
        )
    }

    pub fn build_shape_source(in_frame: u64, out_frame: u64, fps: f64) -> SourceData {
        let defs = SourceData::get_definitions_for_kind(&SourceKind::Shape);
        let mut props = PropertyMap::from_definitions(&defs);

        let heart_path = "M 50,30 A 20,20 0,0,1 90,30 C 90,55 50,85 50,85 C 50,85 10,55 10,30 A 20,20 0,0,1 50,30 Z";
        props.set(
            "path".to_string(),
            Property::constant(PropertyValue::String(heart_path.to_string())),
        );

        SourceData::new(
            Uuid::new_v4(),
            None,
            SourceKind::Shape,
            in_frame,
            out_frame,
            0,
            None,
            fps,
            props,
        )
    }

    pub fn build_sksl_source(in_frame: u64, out_frame: u64, fps: f64) -> SourceData {
        let defs = SourceData::get_definitions_for_kind(&SourceKind::SkSL);
        let mut props = PropertyMap::from_definitions(&defs);

        let default_shader = r#"
half4 main(float2 fragCoord) {
    float2 uv = fragCoord / iResolution.xy;
    float3 col = 0.5 + 0.5*cos(iTime+uv.xyx+float3(0,2,4));
    return half4(col,1.0);
}
"#;

        props.set(
            "shader".to_string(),
            Property::constant(PropertyValue::String(default_shader.to_string())),
        );

        SourceData::new(
            Uuid::new_v4(),
            None,
            SourceKind::SkSL,
            in_frame,
            out_frame,
            0,
            None,
            fps,
            props,
        )
    }
}
