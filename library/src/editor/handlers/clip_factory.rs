use crate::error::LibraryError;
use crate::model::project::clip::{TrackClip, TrackClipKind};
use crate::model::project::property::{Property, PropertyMap, PropertyValue};
use crate::plugin::PluginManager;
use uuid::Uuid;

pub struct ClipFactory;

impl ClipFactory {
    pub fn create_audio_clip(
        reference_id: Option<Uuid>,
        file_path: &str,
        in_frame: u64,
        out_frame: u64,
        source_begin_frame: i64,
        duration_frame: u64,
        fps: f64,
    ) -> TrackClip {
        let defs = TrackClip::get_definitions_for_kind(&TrackClipKind::Audio);
        let mut props = PropertyMap::from_definitions(&defs);

        props.set(
            "file_path".to_string(),
            Property::constant(PropertyValue::String(file_path.to_string())),
        );

        TrackClip::new(
            Uuid::new_v4(),
            reference_id,
            TrackClipKind::Audio,
            in_frame,
            out_frame,
            source_begin_frame,
            Some(duration_frame),
            fps,
            props,
        )
    }

    pub fn create_video_clip(
        plugin_manager: &PluginManager,
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
        let plugin = plugin_manager
            .get_entity_converter("video")
            .ok_or_else(|| LibraryError::plugin("Video converter plugin not found".to_string()))?;

        let defs = plugin.get_property_definitions(
            canvas_width as u64,
            canvas_height as u64,
            canvas_width as u64,
            canvas_height as u64,
        );
        let mut props = PropertyMap::from_definitions(&defs);

        props.set(
            "file_path".to_string(),
            Property::constant(PropertyValue::String(file_path.to_string())),
        );

        Ok(TrackClip::new(
            Uuid::new_v4(),
            reference_id,
            TrackClipKind::Video,
            in_frame,
            out_frame,
            source_begin_frame,
            Some(duration_frame),
            fps,
            props,
        ))
    }

    pub fn create_image_clip(
        plugin_manager: &PluginManager,
        reference_id: Option<Uuid>,
        file_path: &str,
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
        fps: f64,
    ) -> Result<TrackClip, LibraryError> {
        let plugin = plugin_manager
            .get_entity_converter("image")
            .ok_or_else(|| LibraryError::plugin("Image converter plugin not found".to_string()))?;

        let defs = plugin.get_property_definitions(
            canvas_width as u64,
            canvas_height as u64,
            canvas_width as u64,
            canvas_height as u64,
        );
        let mut props = PropertyMap::from_definitions(&defs);

        props.set(
            "file_path".to_string(),
            Property::constant(PropertyValue::String(file_path.to_string())),
        );

        Ok(TrackClip::new(
            Uuid::new_v4(),
            reference_id,
            TrackClipKind::Image,
            in_frame,
            out_frame,
            0,
            None,
            fps,
            props,
        ))
    }

    pub fn create_text_clip(
        plugin_manager: &PluginManager,
        text: &str,
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
        fps: f64,
    ) -> Result<TrackClip, LibraryError> {
        let plugin = plugin_manager
            .get_entity_converter("text")
            .ok_or_else(|| LibraryError::plugin("Text converter plugin not found".to_string()))?;

        // Measure text size
        let (w, h) = crate::plugin::entity_converter::measure_text_size(text, "Arial", 100.0);

        let defs = plugin.get_property_definitions(
            canvas_width as u64,
            canvas_height as u64,
            w as u64,
            h as u64,
        );
        let mut props = PropertyMap::from_definitions(&defs);

        props.set(
            "text".to_string(),
            Property::constant(PropertyValue::String(text.to_string())),
        );

        // Default fill style is now created as a graph node (style.fill)
        // by the caller after clip creation.

        Ok(TrackClip::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::Text,
            in_frame,
            out_frame,
            0,
            None,
            fps,
            props,
        ))
    }

    pub fn create_shape_clip(
        plugin_manager: &PluginManager,
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
        fps: f64,
    ) -> Result<TrackClip, LibraryError> {
        let plugin = plugin_manager
            .get_entity_converter("shape")
            .ok_or_else(|| LibraryError::plugin("Shape converter plugin not found".to_string()))?;

        let defs =
            plugin.get_property_definitions(canvas_width as u64, canvas_height as u64, 100, 100);
        let mut props = PropertyMap::from_definitions(&defs);

        let heart_path = "M 50,30 A 20,20 0,0,1 90,30 C 90,55 50,85 50,85 C 50,85 10,55 10,30 A 20,20 0,0,1 50,30 Z";
        props.set(
            "path".to_string(),
            Property::constant(PropertyValue::String(heart_path.to_string())),
        );

        // Default fill and stroke styles are now created as graph nodes
        // by the caller after clip creation.

        Ok(TrackClip::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::Shape,
            in_frame,
            out_frame,
            0,
            None,
            fps,
            props,
        ))
    }

    pub fn create_sksl_clip(
        plugin_manager: &PluginManager,
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
        fps: f64,
    ) -> Result<TrackClip, LibraryError> {
        let plugin = plugin_manager
            .get_entity_converter("sksl")
            .ok_or_else(|| LibraryError::plugin("SkSL converter plugin not found".to_string()))?;

        let defs = plugin.get_property_definitions(
            canvas_width as u64,
            canvas_height as u64,
            canvas_width as u64,
            canvas_height as u64,
        );
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

        Ok(TrackClip::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::SkSL,
            in_frame,
            out_frame,
            0,
            None,
            fps,
            props,
        ))
    }
}
