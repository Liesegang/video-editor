pub mod asset;
pub mod clip_helpers;
pub mod project;
pub mod property;
pub mod style; // Added

use crate::model::project::property::{PropertyMap, Vec2};
use crate::model::project::style::StyleInstance;
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use serde_json;
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Track {
    pub id: Uuid, // Added UUID field
    pub name: String,
    #[serde(default)]
    pub clips: Vec<TrackClip>,
    #[serde(default)]
    pub children: Vec<Track>, // Sub-folders
}

impl Track {
    pub fn new(name: &str) -> Self {
        Self {
            id: Uuid::new_v4(), // Initialize with a new UUID
            name: name.to_string(),
            clips: Vec::new(),
            children: Vec::new(),
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
    pub id: Uuid,
    pub reference_id: Option<Uuid>,
    #[serde(rename = "type")]
    pub kind: TrackClipKind,
    #[serde(default)]
    pub in_frame: u64,
    #[serde(default)]
    pub out_frame: u64,
    #[serde(default)]
    pub source_begin_frame: i64, // Changed from u64 to i64
    #[serde(default)]
    pub duration_frame: Option<u64>,

    #[serde(default = "default_fps")]
    pub fps: f64,

    #[serde(default)]
    pub properties: PropertyMap,
    #[serde(default)]
    pub styles: Vec<StyleInstance>,
    #[serde(default)]
    pub effects: Vec<EffectConfig>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct EffectConfig {
    pub id: Uuid,
    pub effect_type: String,
    pub properties: PropertyMap,
}

impl std::hash::Hash for EffectConfig {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl PartialEq for EffectConfig {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.effect_type == other.effect_type
            && self.properties == other.properties
    }
}

impl Eq for EffectConfig {}

impl TrackClip {
    pub fn new(
        id: Uuid,
        reference_id: Option<Uuid>,
        kind: TrackClipKind,
        in_frame: u64,
        out_frame: u64,
        source_begin_frame: i64, // Changed from u64 to i64
        duration_frame: Option<u64>,
        fps: f64,
        properties: PropertyMap,
        styles: Vec<StyleInstance>,
        effects: Vec<EffectConfig>,
    ) -> Self {
        Self {
            id,
            reference_id,
            kind,
            in_frame,
            out_frame,
            source_begin_frame, // Changed to i64
            duration_frame,
            fps,
            properties,
            styles,
            effects,
        }
    }

    // Ported helper constructors from Entity
    pub fn default_property_definitions(
        &self,
        canvas_width: u64,
        canvas_height: u64,
        clip_width: u64,
        clip_height: u64,
    ) -> Vec<crate::plugin::PropertyDefinition> {
        use crate::model::project::property::PropertyValue;
        use crate::plugin::{PropertyDefinition, PropertyUiType};
        use ordered_float::OrderedFloat;

        let mut definitions = Vec::new();

        if matches!(
            self.kind,
            TrackClipKind::Image
                | TrackClipKind::Video
                | TrackClipKind::Text
                | TrackClipKind::Shape
                | TrackClipKind::SkSL
        ) {
            let mut transform_defs = vec![
                PropertyDefinition {
                    name: "position".to_string(),
                    label: "Position".to_string(),
                    ui_type: PropertyUiType::Vec2 {
                        suffix: "px".to_string(),
                    },
                    default_value: PropertyValue::Vec2(Vec2 {
                        x: OrderedFloat(canvas_width as f64 / 2.0),
                        y: OrderedFloat(canvas_height as f64 / 2.0),
                    }),
                    category: "Transform".to_string(),
                },
                PropertyDefinition {
                    name: "scale".to_string(),
                    label: "Scale".to_string(),
                    ui_type: PropertyUiType::Vec2 {
                        suffix: "%".to_string(),
                    },
                    default_value: PropertyValue::Vec2(Vec2 {
                        x: OrderedFloat(100.0),
                        y: OrderedFloat(100.0),
                    }),
                    category: "Transform".to_string(),
                },
                PropertyDefinition {
                    name: "rotation".to_string(),
                    label: "Rotation".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: -360.0,
                        max: 360.0,
                        step: 1.0,
                        suffix: "deg".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(0.0)),
                    category: "Transform".to_string(),
                },
                PropertyDefinition {
                    name: "anchor".to_string(),
                    label: "Anchor".to_string(),
                    ui_type: PropertyUiType::Vec2 {
                        suffix: "px".to_string(),
                    },
                    default_value: PropertyValue::Vec2(Vec2 {
                        x: OrderedFloat(clip_width as f64 / 2.0),
                        y: OrderedFloat(clip_height as f64 / 2.0),
                    }),
                    category: "Transform".to_string(),
                },
                PropertyDefinition {
                    name: "opacity".to_string(),
                    label: "Opacity".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: 0.0,
                        max: 100.0,
                        step: 1.0,
                        suffix: "%".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(100.0)),
                    category: "Transform".to_string(),
                },
            ];
            definitions.append(&mut transform_defs);
        }

        if matches!(self.kind, TrackClipKind::Text) {
            let text_defs = vec![
                PropertyDefinition {
                    name: "text".to_string(),
                    label: "Content".to_string(),
                    ui_type: PropertyUiType::Text,
                    default_value: PropertyValue::String("Text".to_string()),
                    category: "Text".to_string(),
                },
                PropertyDefinition {
                    name: "font_family".to_string(),
                    label: "Font".to_string(),
                    ui_type: PropertyUiType::Font,
                    default_value: PropertyValue::String("Arial".to_string()),
                    category: "Text".to_string(),
                },
                PropertyDefinition {
                    name: "size".to_string(),
                    label: "Font Size".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: 1.0,
                        max: 1000.0,
                        step: 1.0,
                        suffix: "px".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(100.0)),
                    category: "Text".to_string(),
                },
            ];
            definitions.extend(text_defs);
        }

        if matches!(self.kind, TrackClipKind::Shape) {
            let shape_defs = vec![
                PropertyDefinition {
                    name: "path".to_string(),
                    label: "Path Data".to_string(),
                    ui_type: PropertyUiType::MultilineText,
                    default_value: PropertyValue::String("".to_string()),
                    category: "Shape".to_string(),
                },
                PropertyDefinition {
                    name: "width".to_string(),
                    label: "Width".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: 0.0,
                        max: 10000.0,
                        step: 1.0,
                        suffix: "px".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(100.0)),
                    category: "Shape".to_string(),
                },
                PropertyDefinition {
                    name: "height".to_string(),
                    label: "Height".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: 0.0,
                        max: 10000.0,
                        step: 1.0,
                        suffix: "px".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(100.0)),
                    category: "Shape".to_string(),
                },
            ];
            definitions.extend(shape_defs);
        }

        if matches!(self.kind, TrackClipKind::SkSL) {
            let shader_defs = vec![PropertyDefinition {
                name: "shader".to_string(),
                label: "Shader Code".to_string(),
                ui_type: PropertyUiType::MultilineText,
                default_value: PropertyValue::String("".to_string()),
                category: "Shader".to_string(),
            }];
            definitions.extend(shader_defs);
        }

        definitions
    }

    // Ported helper constructors from Entity
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
        let mut props = PropertyMap::new();
        props.set(
            "file_path".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::String(file_path.to_string()),
            ),
        );
        // Default transform: Position at canvas center
        props.set(
            "position".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(canvas_width as f64 / 2.0),
                    y: OrderedFloat(canvas_height as f64 / 2.0),
                }),
            ),
        );
        props.set(
            "scale".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(100.0),
                    y: OrderedFloat(100.0),
                }),
            ),
        );
        props.set(
            "rotation".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(0.0)),
            ),
        );
        props.set(
            "anchor".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(0.0),
                    y: OrderedFloat(0.0),
                }),
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
            Vec::new(),
        )
    }

    pub fn create_image(
        reference_id: Option<Uuid>,
        file_path: &str,
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Self {
        let mut props = PropertyMap::new();
        props.set(
            "file_path".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::String(file_path.to_string()),
            ),
        );

        // Default transform: Position at canvas center
        props.set(
            "position".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(canvas_width as f64 / 2.0),
                    y: OrderedFloat(canvas_height as f64 / 2.0),
                }),
            ),
        );
        props.set(
            "scale".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(100.0),
                    y: OrderedFloat(100.0),
                }),
            ),
        );
        props.set(
            "rotation".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(0.0)),
            ),
        );
        props.set(
            "anchor".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(0.0),
                    y: OrderedFloat(0.0),
                }),
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
            Vec::new(),
        )
    }

    pub fn create_text(
        text: &str,
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Self {
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
        let mut styles = Vec::new();
        // Default Fill
        let mut fill_props = PropertyMap::new();
        fill_props.set(
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
        fill_props.set(
            "offset".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(0.0)),
            ),
        );

        styles.push(StyleInstance::new("fill", fill_props));

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
            "position".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(canvas_width as f64 / 2.0),
                    y: OrderedFloat(canvas_height as f64 / 2.0),
                }),
            ),
        );
        props.set(
            "scale".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(100.0),
                    y: OrderedFloat(100.0),
                }),
            ),
        );
        props.set(
            "rotation".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(0.0)),
            ),
        );
        props.set(
            "anchor".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(anchor_x),
                    y: OrderedFloat(anchor_y),
                }),
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
            styles,
            Vec::new(),
        )
    }

    pub fn create_shape(
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Self {
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

        // Default Styles
        let mut styles = Vec::new();

        // Fill Red
        let mut fill_props = PropertyMap::new();
        fill_props.set(
            "color".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Color(
                    crate::model::frame::color::Color {
                        r: 255,
                        g: 0,
                        b: 0,
                        a: 255,
                    },
                ),
            ),
        );
        fill_props.set(
            "offset".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(0.0)),
            ),
        );
        styles.push(StyleInstance::new("fill", fill_props));

        // Stroke White
        let mut stroke_props = PropertyMap::new();
        stroke_props.set(
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
        stroke_props.set(
            "width".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(5.0)),
            ),
        );
        stroke_props.set(
            "offset".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(0.0)),
            ),
        );
        styles.push(StyleInstance::new("stroke", stroke_props));

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

        // Transform: Position at canvas center
        props.set(
            "position".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(canvas_width as f64 / 2.0),
                    y: OrderedFloat(canvas_height as f64 / 2.0),
                }),
            ),
        );
        props.set(
            "scale".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(100.0),
                    y: OrderedFloat(100.0),
                }),
            ),
        );
        props.set(
            "rotation".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(0.0)),
            ),
        );
        props.set(
            "anchor".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(50.0),
                    y: OrderedFloat(50.0),
                }),
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
            styles,
            Vec::new(),
        )
    }

    pub fn create_sksl(
        in_frame: u64,
        out_frame: u64,
        canvas_width: u32,
        canvas_height: u32,
    ) -> Self {
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
            "position".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(canvas_width as f64 / 2.0),
                    y: OrderedFloat(canvas_height as f64 / 2.0),
                }),
            ),
        );
        props.set(
            "scale".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(100.0),
                    y: OrderedFloat(100.0),
                }),
            ),
        );
        props.set(
            "rotation".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Number(OrderedFloat(0.0)),
            ),
        );
        // For SkSL which usually fills screen, we might center anchor too (if it fills screen).
        // Let's assume it matches canvas size for now.
        props.set(
            "anchor".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(canvas_width as f64 / 2.0),
                    y: OrderedFloat(canvas_height as f64 / 2.0),
                }),
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
