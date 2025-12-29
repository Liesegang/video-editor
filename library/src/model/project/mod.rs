pub mod asset;
pub mod clip_helpers;
pub mod effect;
pub mod ensemble;
pub mod project;
pub mod property;
pub mod style;
mod track_clip_factories; // Factory methods for TrackClip

pub use effect::EffectConfig;
pub use ensemble::{DecoratorInstance, EffectorInstance};

use crate::model::project::property::{PropertyMap, Vec2};
use crate::model::project::style::StyleInstance;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 統一ノード型 - 全要素を同一マップで管理
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "node_type")]
pub enum Node {
    Track(TrackData),
    Clip(TrackClip),
}

impl Node {
    /// Get the ID of this node
    pub fn id(&self) -> Uuid {
        match self {
            Node::Track(t) => t.id,
            Node::Clip(c) => c.id,
        }
    }
}

/// トラックデータ (フォルダ/グループ)
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct TrackData {
    pub id: Uuid,
    pub name: String,
    /// 子ノードのUUID (重ね順)
    #[serde(default)]
    pub child_ids: Vec<Uuid>,
}

impl TrackData {
    pub fn new(name: &str) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            child_ids: Vec::new(),
        }
    }

    /// Add a child node ID
    pub fn add_child(&mut self, child_id: Uuid) {
        self.child_ids.push(child_id);
    }

    /// Insert a child node ID at a specific index
    pub fn insert_child(&mut self, index: usize, child_id: Uuid) {
        if index <= self.child_ids.len() {
            self.child_ids.insert(index, child_id);
        } else {
            self.child_ids.push(child_id);
        }
    }

    /// Remove a child node ID
    pub fn remove_child(&mut self, child_id: Uuid) -> bool {
        if let Some(pos) = self.child_ids.iter().position(|id| *id == child_id) {
            self.child_ids.remove(pos);
            true
        } else {
            false
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
    #[serde(default)]
    pub effectors: Vec<EffectorInstance>,
    #[serde(default)]
    pub decorators: Vec<DecoratorInstance>,
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
        effectors: Vec<EffectorInstance>,
        decorators: Vec<DecoratorInstance>,
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
            effectors,
            decorators,
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

            // Ensemble properties
            let ensemble_defs = vec![
                // Core toggle
                PropertyDefinition {
                    name: "ensemble_enabled".to_string(),
                    label: "Enable Ensemble".to_string(),
                    ui_type: PropertyUiType::Bool,
                    default_value: PropertyValue::Boolean(false),
                    category: "Ensemble".to_string(),
                },
                // Transform Effector
                PropertyDefinition {
                    name: "ensemble_transform_enabled".to_string(),
                    label: "Transform Enabled".to_string(),
                    ui_type: PropertyUiType::Bool,
                    default_value: PropertyValue::Boolean(false),
                    category: "Ensemble: Transform".to_string(),
                },
                PropertyDefinition {
                    name: "ensemble_transform_translate".to_string(),
                    label: "Translate".to_string(),
                    ui_type: PropertyUiType::Vec2 {
                        suffix: "px".to_string(),
                    },
                    default_value: PropertyValue::Vec2(Vec2 {
                        x: OrderedFloat(0.0),
                        y: OrderedFloat(0.0),
                    }),
                    category: "Ensemble: Transform".to_string(),
                },
                PropertyDefinition {
                    name: "ensemble_transform_rotate".to_string(),
                    label: "Rotate".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: -360.0,
                        max: 360.0,
                        step: 1.0,
                        suffix: "deg".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(0.0)),
                    category: "Ensemble: Transform".to_string(),
                },
                PropertyDefinition {
                    name: "ensemble_transform_scale".to_string(),
                    label: "Scale".to_string(),
                    ui_type: PropertyUiType::Vec2 {
                        suffix: "%".to_string(),
                    },
                    default_value: PropertyValue::Vec2(Vec2 {
                        x: OrderedFloat(100.0),
                        y: OrderedFloat(100.0),
                    }),
                    category: "Ensemble: Transform".to_string(),
                },
                // StepDelay Effector
                PropertyDefinition {
                    name: "ensemble_step_delay_enabled".to_string(),
                    label: "Step Delay Enabled".to_string(),
                    ui_type: PropertyUiType::Bool,
                    default_value: PropertyValue::Boolean(false),
                    category: "Ensemble: Step Delay".to_string(),
                },
                PropertyDefinition {
                    name: "ensemble_step_delay_per_element".to_string(),
                    label: "Delay Per Element".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: 0.0,
                        max: 2.0,
                        step: 0.01,
                        suffix: "s".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(0.1)),
                    category: "Ensemble: Step Delay".to_string(),
                },
                PropertyDefinition {
                    name: "ensemble_step_delay_duration".to_string(),
                    label: "Duration".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: 0.0,
                        max: 5.0,
                        step: 0.1,
                        suffix: "s".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(1.0)),
                    category: "Ensemble: Step Delay".to_string(),
                },
                PropertyDefinition {
                    name: "ensemble_step_delay_from_opacity".to_string(),
                    label: "From Opacity".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: 0.0,
                        max: 100.0,
                        step: 1.0,
                        suffix: "%".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(0.0)),
                    category: "Ensemble: Step Delay".to_string(),
                },
                PropertyDefinition {
                    name: "ensemble_step_delay_to_opacity".to_string(),
                    label: "To Opacity".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: 0.0,
                        max: 100.0,
                        step: 1.0,
                        suffix: "%".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(100.0)),
                    category: "Ensemble: Step Delay".to_string(),
                },
                // Opacity Effector
                PropertyDefinition {
                    name: "ensemble_opacity_enabled".to_string(),
                    label: "Opacity Enabled".to_string(),
                    ui_type: PropertyUiType::Bool,
                    default_value: PropertyValue::Boolean(false),
                    category: "Ensemble: Opacity".to_string(),
                },
                PropertyDefinition {
                    name: "ensemble_opacity_target".to_string(),
                    label: "Target Opacity".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: 0.0,
                        max: 100.0,
                        step: 1.0,
                        suffix: "%".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(50.0)),
                    category: "Ensemble: Opacity".to_string(),
                },
                // Randomize Effector
                PropertyDefinition {
                    name: "ensemble_randomize_enabled".to_string(),
                    label: "Randomize Enabled".to_string(),
                    ui_type: PropertyUiType::Bool,
                    default_value: PropertyValue::Boolean(false),
                    category: "Ensemble: Randomize".to_string(),
                },
                PropertyDefinition {
                    name: "ensemble_randomize_translate_range".to_string(),
                    label: "Translate Range".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: 0.0,
                        max: 100.0,
                        step: 1.0,
                        suffix: "px".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(10.0)),
                    category: "Ensemble: Randomize".to_string(),
                },
                PropertyDefinition {
                    name: "ensemble_randomize_rotate_range".to_string(),
                    label: "Rotate Range".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: 0.0,
                        max: 180.0,
                        step: 1.0,
                        suffix: "deg".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(15.0)),
                    category: "Ensemble: Randomize".to_string(),
                },
                PropertyDefinition {
                    name: "ensemble_randomize_seed".to_string(),
                    label: "Random Seed".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: 0.0,
                        max: 10000.0,
                        step: 1.0,
                        suffix: "".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(0.0)),
                    category: "Ensemble: Randomize".to_string(),
                },
                // Backplate Decorator
                PropertyDefinition {
                    name: "ensemble_backplate_enabled".to_string(),
                    label: "Backplate Enabled".to_string(),
                    ui_type: PropertyUiType::Bool,
                    default_value: PropertyValue::Boolean(false),
                    category: "Ensemble: Backplate".to_string(),
                },
                PropertyDefinition {
                    name: "ensemble_backplate_target".to_string(),
                    label: "Target".to_string(),
                    ui_type: PropertyUiType::Dropdown {
                        options: vec!["Char".to_string(), "Line".to_string(), "Block".to_string()],
                    },
                    default_value: PropertyValue::String("Block".to_string()),
                    category: "Ensemble: Backplate".to_string(),
                },
                PropertyDefinition {
                    name: "ensemble_backplate_shape".to_string(),
                    label: "Shape".to_string(),
                    ui_type: PropertyUiType::Dropdown {
                        options: vec![
                            "Rect".to_string(),
                            "RoundRect".to_string(),
                            "Circle".to_string(),
                        ],
                    },
                    default_value: PropertyValue::String("Rect".to_string()),
                    category: "Ensemble: Backplate".to_string(),
                },
                PropertyDefinition {
                    name: "ensemble_backplate_color".to_string(),
                    label: "Backplate Color".to_string(),
                    ui_type: PropertyUiType::Color,
                    default_value: PropertyValue::Color(crate::model::frame::color::Color {
                        r: 0,
                        g: 0,
                        b: 0,
                        a: 128,
                    }),
                    category: "Ensemble: Backplate".to_string(),
                },
                PropertyDefinition {
                    name: "ensemble_backplate_padding".to_string(),
                    label: "Backplate Padding".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: 0.0,
                        max: 50.0,
                        step: 1.0,
                        suffix: "px".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(5.0)),
                    category: "Ensemble: Backplate".to_string(),
                },
                PropertyDefinition {
                    name: "ensemble_backplate_corner_radius".to_string(),
                    label: "Corner Radius".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: 0.0,
                        max: 50.0,
                        step: 1.0,
                        suffix: "px".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(4.0)),
                    category: "Ensemble: Backplate".to_string(),
                },
                // Patch System
                PropertyDefinition {
                    name: "ensemble_patch_enabled".to_string(),
                    label: "Enable Patch".to_string(),
                    ui_type: PropertyUiType::Bool,
                    default_value: PropertyValue::Boolean(false),
                    category: "Ensemble: Patch".to_string(),
                },
                PropertyDefinition {
                    name: "ensemble_patch_indices".to_string(),
                    label: "Character Indices".to_string(),
                    ui_type: PropertyUiType::Text,
                    default_value: PropertyValue::String("0".to_string()),
                    category: "Ensemble: Patch".to_string(),
                },
                PropertyDefinition {
                    name: "ensemble_patch_translate".to_string(),
                    label: "Patch Translate".to_string(),
                    ui_type: PropertyUiType::Vec2 {
                        suffix: "px".to_string(),
                    },
                    default_value: PropertyValue::Vec2(crate::model::project::property::Vec2 {
                        x: OrderedFloat(0.0),
                        y: OrderedFloat(0.0),
                    }),
                    category: "Ensemble: Patch".to_string(),
                },
                PropertyDefinition {
                    name: "ensemble_patch_rotate".to_string(),
                    label: "Patch Rotate".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: -360.0,
                        max: 360.0,
                        step: 1.0,
                        suffix: "deg".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(0.0)),
                    category: "Ensemble: Patch".to_string(),
                },
                PropertyDefinition {
                    name: "ensemble_patch_scale".to_string(),
                    label: "Patch Scale".to_string(),
                    ui_type: PropertyUiType::Vec2 {
                        suffix: "%".to_string(),
                    },
                    default_value: PropertyValue::Vec2(crate::model::project::property::Vec2 {
                        x: OrderedFloat(100.0),
                        y: OrderedFloat(100.0),
                    }),
                    category: "Ensemble: Patch".to_string(),
                },
                PropertyDefinition {
                    name: "ensemble_patch_opacity".to_string(),
                    label: "Patch Opacity".to_string(),
                    ui_type: PropertyUiType::Float {
                        min: 0.0,
                        max: 100.0,
                        step: 1.0,
                        suffix: "%".to_string(),
                    },
                    default_value: PropertyValue::Number(OrderedFloat(100.0)),
                    category: "Ensemble: Patch".to_string(),
                },
            ];
            definitions.extend(ensemble_defs);
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
