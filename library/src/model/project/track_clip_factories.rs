//! Factory methods for creating TrackClip instances.
//!
//! This file contains the `create_*` constructors for different clip types.

use super::{TrackClip, TrackClipKind};
use crate::model::frame::color::Color;
use crate::model::project::property::{Property, PropertyMap, PropertyValue, Vec2};
use crate::model::project::style::StyleInstance;
use ordered_float::OrderedFloat;
use uuid::Uuid;

impl TrackClip {
    /// Create an audio clip with default properties.
    pub fn create_audio(
        reference_id: Option<Uuid>,
        file_path: &str,
        in_frame: u64,
        out_frame: u64,
        source_begin_frame: i64,
        duration_frame: u64,
        fps: f64,
    ) -> Self {
        // Initialize properties from definitions
        let defs = TrackClip::get_definitions_for_kind(&TrackClipKind::Audio);
        let mut props = PropertyMap::from_definitions(&defs);

        // Set specific values
        props.set(
            "file_path".to_string(),
            Property::constant(PropertyValue::String(file_path.to_string())),
        );

        TrackClip {
            id: Uuid::new_v4(),
            reference_id,
            kind: TrackClipKind::Audio,
            in_frame,
            out_frame,
            source_begin_frame,
            duration_frame: Some(duration_frame),
            fps,
            properties: props,
            effects: Vec::new(),
            effectors: Vec::new(),
            decorators: Vec::new(),
            styles: Vec::new(),
        }
    }

    /// Create a video clip with default properties.
    pub fn create_video(
        reference_id: Option<Uuid>,
        file_path: &str,
        in_frame: u64,
        out_frame: u64,
        source_begin_frame: i64,
        duration_frame: u64,
        fps: f64,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Self {
        // Initialize properties from definitions
        let defs = TrackClip::get_definitions_for_kind(&TrackClipKind::Video);
        let mut props = PropertyMap::from_definitions(&defs);

        // Update default values with specific contexts (like canvas size)
        props.set(
            "file_path".to_string(),
            Property::constant(PropertyValue::String(file_path.to_string())),
        );
        props.set(
            "position".to_string(),
            Property::constant(PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(canvas_width as f64 / 2.0),
                y: OrderedFloat(canvas_height as f64 / 2.0),
            })),
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
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
    }

    /// Create an image clip with default properties.
    pub fn create_image(
        reference_id: Option<Uuid>,
        file_path: &str,
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
        fps: f64,
    ) -> Self {
        // Initialize properties from definitions
        let defs = TrackClip::get_definitions_for_kind(&TrackClipKind::Image);
        let mut props = PropertyMap::from_definitions(&defs);

        props.set(
            "file_path".to_string(),
            Property::constant(PropertyValue::String(file_path.to_string())),
        );
        props.set(
            "position".to_string(),
            Property::constant(PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(canvas_width as f64 / 2.0),
                y: OrderedFloat(canvas_height as f64 / 2.0),
            })),
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
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
    }

    /// Create a text clip with default properties.
    pub fn create_text(
        text: &str,
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
        fps: f64,
    ) -> Self {
        // Initialize properties from definitions
        let defs = TrackClip::get_definitions_for_kind(&TrackClipKind::Text);
        let mut props = PropertyMap::from_definitions(&defs);

        props.set(
            "text".to_string(),
            Property::constant(PropertyValue::String(text.to_string())),
        );
        // Defaults for font_family and size are already set by from_definitions, but we can override if needed
        let font_size = props.get_f64("size").unwrap_or(100.0);

        let mut styles = Vec::new();

        // Default fill style (white)
        let mut fill_props = PropertyMap::new();
        fill_props.set(
            "color".to_string(),
            Property::constant(PropertyValue::Color(Color {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            })),
        );
        styles.push(StyleInstance::new("fill", fill_props));

        // Center based on estimated text dimensions
        let estimated_text_width = text.chars().count() as f64 * font_size * 0.6;
        let anchor_x = estimated_text_width / 2.0;
        let anchor_y = font_size / 2.0;

        props.set(
            "position".to_string(),
            Property::constant(PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(canvas_width as f64 / 2.0),
                y: OrderedFloat(canvas_height as f64 / 2.0),
            })),
        );
        props.set(
            "anchor".to_string(),
            Property::constant(PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(anchor_x),
                y: OrderedFloat(anchor_y),
            })),
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
            styles,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
    }

    /// Create a shape clip with default properties (heart shape).
    pub fn create_shape(
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
        fps: f64,
    ) -> Self {
        // Initialize properties from definitions
        let defs = TrackClip::get_definitions_for_kind(&TrackClipKind::Shape);
        let mut props = PropertyMap::from_definitions(&defs);

        let heart_path = "M 50,30 A 20,20 0,0,1 90,30 C 90,55 50,85 50,85 C 50,85 10,55 10,30 A 20,20 0,0,1 50,30 Z";
        props.set(
            "path".to_string(),
            Property::constant(PropertyValue::String(heart_path.to_string())),
        );

        // Default styles for heart
        let mut styles = Vec::new();

        // Fill (red)
        let mut fill_props = PropertyMap::new();
        fill_props.set(
            "color".to_string(),
            Property::constant(PropertyValue::Color(Color {
                r: 255,
                g: 51,
                b: 51,
                a: 255,
            })),
        );
        styles.push(StyleInstance::new("fill", fill_props));

        // Stroke (white)
        let mut stroke_props = PropertyMap::new();
        stroke_props.set(
            "color".to_string(),
            Property::constant(PropertyValue::Color(Color {
                r: 255,
                g: 255,
                b: 255,
                a: 255,
            })),
        );
        stroke_props.set(
            "width".to_string(),
            Property::constant(PropertyValue::Number(OrderedFloat(2.0))),
        );
        stroke_props.set(
            "join".to_string(),
            Property::constant(PropertyValue::String("round".to_string())),
        );
        stroke_props.set(
            "cap".to_string(),
            Property::constant(PropertyValue::String("round".to_string())),
        );
        stroke_props.set(
            "miter_limit".to_string(),
            Property::constant(PropertyValue::Number(OrderedFloat(4.0))),
        );
        stroke_props.set(
            "dash_offset".to_string(),
            Property::constant(PropertyValue::Number(OrderedFloat(0.0))),
        );
        styles.push(StyleInstance::new("stroke", stroke_props));

        props.set(
            "position".to_string(),
            Property::constant(PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(canvas_width as f64 / 2.0),
                y: OrderedFloat(canvas_height as f64 / 2.0),
            })),
        );
        props.set(
            "anchor".to_string(),
            Property::constant(PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(50.0),
                y: OrderedFloat(50.0),
            })),
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
            styles,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
    }

    /// Create an SkSL shader clip with default properties.
    pub fn create_sksl(
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
        fps: f64,
    ) -> Self {
        // Initialize properties from definitions
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
        props.set(
            "position".to_string(),
            Property::constant(PropertyValue::Vec2(Vec2 {
                x: OrderedFloat(canvas_width as f64 / 2.0),
                y: OrderedFloat(canvas_height as f64 / 2.0),
            })),
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
            Vec::new(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )
    }
}
