use crate::project::clip::{TrackClip, TrackClipKind};
use crate::project::property::{Property, PropertyMap, PropertyValue};
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
        reference_id: Option<Uuid>,
        file_path: &str,
        in_frame: u64,
        out_frame: u64,
        source_begin_frame: i64,
        duration_frame: u64,
        fps: f64,
    ) -> TrackClip {
        let defs = TrackClip::get_definitions_for_kind(&TrackClipKind::Video);
        let mut props = PropertyMap::from_definitions(&defs);

        props.set(
            "file_path".to_string(),
            Property::constant(PropertyValue::String(file_path.to_string())),
        );

        TrackClip::new(
            Uuid::new_v4(),
            reference_id,
            TrackClipKind::Video,
            in_frame,
            out_frame,
            source_begin_frame,
            Some(duration_frame),
            fps,
            props,
        )
    }

    pub fn create_image_clip(
        reference_id: Option<Uuid>,
        file_path: &str,
        in_frame: u64,
        out_frame: u64,
        fps: f64,
    ) -> TrackClip {
        let defs = TrackClip::get_definitions_for_kind(&TrackClipKind::Image);
        let mut props = PropertyMap::from_definitions(&defs);

        props.set(
            "file_path".to_string(),
            Property::constant(PropertyValue::String(file_path.to_string())),
        );

        TrackClip::new(
            Uuid::new_v4(),
            reference_id,
            TrackClipKind::Image,
            in_frame,
            out_frame,
            0,
            None,
            fps,
            props,
        )
    }

    pub fn create_text_clip(text: &str, in_frame: u64, out_frame: u64, fps: f64) -> TrackClip {
        let defs = TrackClip::get_definitions_for_kind(&TrackClipKind::Text);
        let mut props = PropertyMap::from_definitions(&defs);

        props.set(
            "text".to_string(),
            Property::constant(PropertyValue::String(text.to_string())),
        );

        TrackClip::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::Text,
            in_frame,
            out_frame,
            0,
            None,
            fps,
            props,
        )
    }

    pub fn create_shape_clip(in_frame: u64, out_frame: u64, fps: f64) -> TrackClip {
        let defs = TrackClip::get_definitions_for_kind(&TrackClipKind::Shape);
        let mut props = PropertyMap::from_definitions(&defs);

        let heart_path = "M 50,30 A 20,20 0,0,1 90,30 C 90,55 50,85 50,85 C 50,85 10,55 10,30 A 20,20 0,0,1 50,30 Z";
        props.set(
            "path".to_string(),
            Property::constant(PropertyValue::String(heart_path.to_string())),
        );

        TrackClip::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::Shape,
            in_frame,
            out_frame,
            0,
            None,
            fps,
            props,
        )
    }

    pub fn create_sksl_clip(in_frame: u64, out_frame: u64, fps: f64) -> TrackClip {
        let defs = TrackClip::get_definitions_for_kind(&TrackClipKind::SkSL);
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

        TrackClip::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::SkSL,
            in_frame,
            out_frame,
            0,
            None,
            fps,
            props,
        )
    }
}
