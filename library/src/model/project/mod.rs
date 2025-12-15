pub mod asset;
pub mod project;
pub mod property;

use crate::model::project::property::PropertyMap;
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use serde_json;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Track {
    pub id: Uuid, // Added UUID field
    pub name: String,
    pub clips: Vec<TrackClip>,
}

impl Track {
    pub fn new(name: &str) -> Self {
        Self {
            id: Uuid::new_v4(), // Initialize with a new UUID
            name: name.to_string(),
            clips: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "lowercase")] // Serialize as "video", "image", etc.
pub enum TrackClipKind {
    Video,
    Image,
    Audio,
    Text,
    Shape,
    SkSL,
    Composition,
    // Add other kinds as needed
}

impl std::fmt::Display for TrackClipKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            TrackClipKind::Video => "video",
            TrackClipKind::Image => "image",
            TrackClipKind::Audio => "audio",
            TrackClipKind::Text => "text",
            TrackClipKind::Shape => "shape",
            TrackClipKind::SkSL => "sksl",
            TrackClipKind::Composition => "composition",
        };
        write!(f, "{}", s)
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct TrackClip {
    pub id: Uuid,                   // Added UUID field
    pub reference_id: Option<Uuid>, // ID of the referenced Asset or Composition
    #[serde(rename = "type")]
    pub kind: TrackClipKind,
    #[serde(default)]
    pub in_frame: u64, // Renamed from start_time (timeline start in frames)
    #[serde(default)]
    pub out_frame: u64, // Renamed from end_time (timeline end in frames)
    #[serde(default)]
    pub source_begin_frame: u64, // Frame where source content begins
    #[serde(default)]
    pub duration_frame: Option<u64>, // Duration of source content in frames, None for static/infinite

    #[serde(default = "default_fps")]
    pub fps: f64, // This fps likely refers to the source content fps

    #[serde(default)]
    pub properties: PropertyMap,
    #[serde(default)]
    pub effects: Vec<EffectConfig>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct EffectConfig {
    pub effect_type: String,
    pub properties: PropertyMap,
}

impl TrackClip {
    pub fn new(
        id: Uuid,
        reference_id: Option<Uuid>,
        kind: TrackClipKind,
        in_frame: u64,               // Renamed parameter
        out_frame: u64,              // Renamed parameter
        source_begin_frame: u64,     // New parameter
        duration_frame: Option<u64>, // New parameter
        fps: f64,
        properties: PropertyMap,
        effects: Vec<EffectConfig>,
    ) -> Self {
        Self {
            id,
            reference_id,
            kind,
            in_frame,
            out_frame,
            source_begin_frame,
            duration_frame,
            fps,
            properties,
            effects,
        }
    }

    // Ported helper constructors from Entity
    pub fn create_video(
        reference_id: Option<Uuid>,
        file_path: &str,
        in_frame: u64,
        out_frame: u64,
        source_begin_frame: u64,
        duration_frame: u64,
        fps: f64,
    ) -> Self {
        let mut props = PropertyMap::new();
        props.set(
            "file_path".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::String(file_path.to_string()),
            ),
        );
        // Default transform
        props.set(
            "position_x".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(960.0)),
            ),
        );
        props.set(
            "position_y".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(540.0)),
            ),
        );
        props.set(
            "scale_x".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(100.0)),
            ),
        );
        props.set(
            "scale_y".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(100.0)),
            ),
        );
        props.set(
            "rotation".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(0.0)),
            ),
        );
        props.set(
            "anchor_x".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(0.0)),
            ),
        );
        props.set(
            "anchor_y".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(0.0)),
            ),
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
        )
    }

    pub fn create_image(
        reference_id: Option<Uuid>,
        file_path: &str,
        in_frame: u64,
        out_frame: u64,
    ) -> Self {
        let mut props = PropertyMap::new();
        props.set(
            "file_path".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::String(file_path.to_string()),
            ),
        );

        // Default transform
        props.set(
            "position_x".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(960.0)),
            ),
        );
        props.set(
            "position_y".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(540.0)),
            ),
        );
        props.set(
            "scale_x".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(100.0)),
            ),
        );
        props.set(
            "scale_y".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(100.0)),
            ),
        );
        props.set(
            "rotation".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(0.0)),
            ),
        );
        props.set(
            "anchor_x".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(0.0)),
            ),
        );
        props.set(
            "anchor_y".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(0.0)),
            ),
        );

        TrackClip::new(
            Uuid::new_v4(),
            reference_id,
            TrackClipKind::Image,
            in_frame,
            out_frame,
            0,
            None, // Image is static
            0.0,
            props,
            Vec::new(),
        )
    }

    pub fn create_text(text: &str, in_frame: u64, out_frame: u64) -> Self {
        let mut props = PropertyMap::new();
        // User requested default: "this is sample text", Arial, White

        let font_size = 100.0;

        props.set(
            "text".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::String(text.to_string()),
            ),
        );
        props.set(
            "font_family".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::String("Arial".to_string()),
            ),
        );
        props.set(
            "size".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(font_size)),
            ),
        );
        props.set(
            "color".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Color(
                    crate::model::frame::color::Color {
                        r: 255,
                        g: 255,
                        b: 255,
                        a: 255,
                    },
                ),
            ),
        );

        // Measure text for anchor centering
        let font_mgr = skia_safe::FontMgr::default();
        let typeface = font_mgr
            .match_family_style("Arial", skia_safe::FontStyle::normal())
            .unwrap_or_else(|| {
                font_mgr
                    .match_family_style("Arial", skia_safe::FontStyle::normal())
                    .expect("Failed to load default font")
            }); // Fallback

        let mut font = skia_safe::Font::default();
        font.set_typeface(typeface);
        font.set_size(font_size as f32);

        let width =
            crate::rendering::text_layout::measure_text_width(text, "Arial", font_size as f32);
        let (_, metrics) = font.metrics();
        // Calculate height consistent with entity_converters logic
        let height = metrics.descent - metrics.ascent;

        let anchor_x = width as f64 / 2.0;
        let anchor_y = height as f64 / 2.0;

        // Default transform
        props.set(
            "position_x".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(960.0)),
            ),
        );
        props.set(
            "position_y".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(540.0)),
            ),
        );
        props.set(
            "scale_x".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(100.0)),
            ),
        );
        props.set(
            "scale_y".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(100.0)),
            ),
        );
        props.set(
            "rotation".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(0.0)),
            ),
        );
        props.set(
            "anchor_x".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(anchor_x)),
            ),
        );
        props.set(
            "anchor_y".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(anchor_y)),
            ),
        );
        props.set(
            "opacity".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(100.0)),
            ),
        );

        TrackClip::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::Text,
            in_frame,
            out_frame,
            0,
            None, // Text is static
            0.0,
            props,
            Vec::new(),
        )
    }

    pub fn create_shape(in_frame: u64, out_frame: u64) -> Self {
        let mut props = PropertyMap::new();

        // Default Shape Properties
        // User requested: Heart (White Border, Red Fill)
        let heart_path = "M 50,30 A 20,20 0,0,1 90,30 C 90,55 50,85 50,85 C 50,85 10,55 10,30 A 20,20 0,0,1 50,30 Z";
        props.set(
            "path".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::String(heart_path.to_string()),
            ),
        );

        // Styles: Fill Red (#FF0000), Stroke White (#FFFFFF, width 2.0)
        let styles = vec![
            crate::model::frame::draw_type::DrawStyle::Fill {
                color: crate::model::frame::color::Color {
                    r: 255,
                    g: 0,
                    b: 0,
                    a: 255,
                },
            },
            crate::model::frame::draw_type::DrawStyle::Stroke {
                color: crate::model::frame::color::Color {
                    r: 255,
                    g: 255,
                    b: 255,
                    a: 255,
                },
                width: 5.0,
                cap: Default::default(),
                join: Default::default(),
                miter: 4.0,
            },
        ];

        // Serialize styles to PropertyValue::Array
        let styles_json = serde_json::to_value(styles).unwrap();
        let styles_array = match styles_json {
            serde_json::Value::Array(arr) => arr
                .into_iter()
                .map(crate::model::project::property::PropertyValue::from)
                .collect(),
            _ => vec![],
        };

        props.set(
            "styles".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Array(styles_array),
            ),
        );

        props.set(
            "width".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(100.0)),
            ),
        );
        props.set(
            "height".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(100.0)),
            ),
        );

        // Transform
        props.set(
            "position_x".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(960.0)),
            ),
        );
        props.set(
            "position_y".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(540.0)),
            ),
        );
        props.set(
            "scale_x".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(100.0)),
            ),
        );
        props.set(
            "scale_y".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(100.0)),
            ),
        );
        props.set(
            "rotation".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(0.0)),
            ),
        );
        props.set(
            "anchor_x".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(0.0)),
            ),
        );
        props.set(
            "anchor_y".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(0.0)),
            ),
        );
        props.set(
            "opacity".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(100.0)),
            ),
        );

        TrackClip::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::Shape,
            in_frame,
            out_frame,
            0,
            None,
            0.0,
            props,
            Vec::new(),
        )
    }

    pub fn create_sksl(in_frame: u64, out_frame: u64) -> Self {
        let mut props = PropertyMap::new();

        // Default ShaderToy-compatible shader
        // Note: Uniforms (iResolution, iTime, etc.) are automatically injected by the renderer.
        let default_shader = r#"
half4 main(float2 fragCoord) {
    float2 uv = fragCoord / iResolution.xy;
    float3 col = 0.5 + 0.5*cos(iTime+uv.xyx+float3(0,2,4));
    return half4(col,1.0);
}
"#;

        props.set(
            "shader".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::String(default_shader.to_string()),
            ),
        );

        // Default transform
        props.set(
            "position_x".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(960.0)),
            ),
        );
        props.set(
            "position_y".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(540.0)),
            ),
        );
        props.set(
            "scale_x".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(100.0)),
            ),
        );
        props.set(
            "scale_y".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(100.0)),
            ),
        );
        props.set(
            "rotation".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(0.0)),
            ),
        );
        props.set(
            "anchor_x".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(960.0)),
            ),
        );
        props.set(
            "anchor_y".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(540.0)),
            ),
        );
        props.set(
            "opacity".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(100.0)),
            ),
        );

        TrackClip::new(
            Uuid::new_v4(),
            None,
            TrackClipKind::SkSL,
            in_frame,
            out_frame,
            0,
            None,
            0.0,
            props,
            Vec::new(),
        )
    }

    // Helper for consistency with Entity
    pub fn set_constant_property(
        &mut self,
        key: &str,
        value: crate::model::project::property::PropertyValue,
    ) {
        self.properties.set(
            key.to_string(),
            crate::model::project::property::Property::constant(value),
        );
    }
}

const fn default_fps() -> f64 {
    0.0
}
