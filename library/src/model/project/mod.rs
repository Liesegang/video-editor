pub mod asset;
pub mod clip_helpers;
pub mod effect;
pub mod project;
pub mod property;
pub mod style;
mod track_clip_factories; // Factory methods for TrackClip

pub use effect::EffectConfig;

use crate::model::project::property::{PropertyMap, Vec2};
use crate::model::project::style::StyleInstance;
use serde::{Deserialize, Serialize};
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
    ) -> Vec<property::PropertyDefinition> {
        use crate::model::project::property::PropertyValue;
        use ordered_float::OrderedFloat;
        use property::{PropertyDefinition, PropertyUiType};

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

    /// Update or upsert a clip property value/keyframe.
    /// If property exists and is keyframed, upserts keyframe at time.
    /// If property is constant or doesn't exist, sets as constant.
    pub fn update_property_or_keyframe(
        &mut self,
        key: &str,
        time: f64,
        value: property::PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) {
        if let Some(prop) = self.properties.get_mut(key) {
            if prop.evaluator == "keyframe" {
                prop.upsert_keyframe(time, value, easing);
            } else {
                self.properties
                    .set(key.to_string(), property::Property::constant(value));
            }
        } else {
            self.properties
                .set(key.to_string(), property::Property::constant(value));
        }
    }

    /// Update effect property at specified index.
    pub fn update_effect_property(
        &mut self,
        effect_index: usize,
        key: &str,
        time: f64,
        value: property::PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), &'static str> {
        let effect = self
            .effects
            .get_mut(effect_index)
            .ok_or("Effect not found")?;
        effect.update_property_or_keyframe(key, time, value, easing);
        Ok(())
    }

    /// Update style property at specified index.
    pub fn update_style_property(
        &mut self,
        style_index: usize,
        key: &str,
        time: f64,
        value: property::PropertyValue,
        easing: Option<crate::animation::EasingFunction>,
    ) -> Result<(), &'static str> {
        let style = self.styles.get_mut(style_index).ok_or("Style not found")?;
        style.update_property_or_keyframe(key, time, value, easing);
        Ok(())
    }
}

const fn default_fps() -> f64 {
    30.0
}
