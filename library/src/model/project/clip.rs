use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::effect::EffectConfig;
use super::ensemble::{DecoratorInstance, EffectorInstance};
use super::property::{self, PropertyMap};
use super::style::StyleInstance;

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
    pub source_begin_frame: i64,
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
        source_begin_frame: i64,
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
            source_begin_frame,
            duration_frame,
            fps,
            properties,
            styles,
            effects,
            effectors,
            decorators,
        }
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
        effect
            .properties
            .update_property_or_keyframe(key, time, value, easing);
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
        style
            .properties
            .update_property_or_keyframe(key, time, value, easing);
        Ok(())
    }

    /// Unified accessor for property maps
    pub fn get_property_map_mut(
        &mut self,
        target: crate::model::project::property::PropertyTarget,
    ) -> Option<&mut crate::model::project::property::PropertyMap> {
        use crate::model::project::property::PropertyTarget;
        match target {
            PropertyTarget::Clip => Some(&mut self.properties),
            PropertyTarget::Effect(i) => self.effects.get_mut(i).map(|e| &mut e.properties),
            PropertyTarget::Style(i) => self.styles.get_mut(i).map(|s| &mut s.properties),
            PropertyTarget::Effector(i) => self.effectors.get_mut(i).map(|e| &mut e.properties),
            PropertyTarget::Decorator(i) => self.decorators.get_mut(i).map(|e| &mut e.properties),
            PropertyTarget::GraphNode(_) => None, // GraphNode properties are accessed via Project.nodes
        }
    }

    pub fn get_transform_definitions() -> Vec<crate::model::project::property::PropertyDefinition> {
        use crate::model::project::property::{
            PropertyDefinition, PropertyUiType, PropertyValue, Vec2,
        };
        use ordered_float::OrderedFloat;

        vec![
            PropertyDefinition::new(
                "position",
                PropertyUiType::Vec2 {
                    suffix: "".to_string(),
                },
                "Position",
                PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(0.0),
                    y: OrderedFloat(0.0),
                }),
            ),
            PropertyDefinition::new(
                "scale",
                PropertyUiType::Vec2 {
                    suffix: "".to_string(),
                },
                "Scale",
                PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(100.0),
                    y: OrderedFloat(100.0),
                }),
            ),
            PropertyDefinition::new(
                "rotation",
                PropertyUiType::Float {
                    min: -360.0,
                    max: 360.0,
                    step: 1.0,
                    suffix: "°".into(),
                    min_hard_limit: false,
                    max_hard_limit: false,
                },
                "Rotation",
                PropertyValue::Number(OrderedFloat(0.0)),
            ),
            PropertyDefinition::new(
                "anchor",
                PropertyUiType::Vec2 {
                    suffix: "".to_string(),
                },
                "Anchor Point",
                PropertyValue::Vec2(Vec2 {
                    x: OrderedFloat(0.0),
                    y: OrderedFloat(0.0),
                }),
            ),
            PropertyDefinition::new(
                "opacity",
                PropertyUiType::Float {
                    min: 0.0,
                    max: 100.0,
                    step: 1.0,
                    suffix: "%".into(),
                    min_hard_limit: true,
                    max_hard_limit: true,
                },
                "Opacity",
                PropertyValue::Number(OrderedFloat(100.0)),
            ),
        ]
    }

    pub fn get_definitions_for_kind(
        kind: &TrackClipKind,
    ) -> Vec<crate::model::project::property::PropertyDefinition> {
        use crate::model::project::property::{PropertyDefinition, PropertyUiType, PropertyValue};
        use ordered_float::OrderedFloat;

        let mut defs = vec![];

        // Specific properties
        match kind {
            TrackClipKind::Audio => {
                defs.push(PropertyDefinition::new(
                    "file_path",
                    PropertyUiType::Text,
                    "File Path",
                    PropertyValue::String("".to_string()),
                ));
            }
            TrackClipKind::Video | TrackClipKind::Image => {
                defs.push(PropertyDefinition::new(
                    "file_path",
                    PropertyUiType::Text,
                    "File Path",
                    PropertyValue::String("".to_string()),
                ));
                defs.extend(Self::get_transform_definitions());
            }
            TrackClipKind::Text => {
                defs.push(PropertyDefinition::new(
                    "text",
                    PropertyUiType::Text,
                    "Text",
                    PropertyValue::String("".to_string()),
                ));
                defs.push(PropertyDefinition::new(
                    "font_family",
                    PropertyUiType::Font,
                    "Font Family",
                    PropertyValue::String("Arial".to_string()),
                ));
                defs.push(PropertyDefinition::new(
                    "size",
                    PropertyUiType::Float {
                        min: 1.0,
                        max: 500.0,
                        step: 1.0,
                        suffix: "px".into(),
                        min_hard_limit: false,
                        max_hard_limit: false,
                    },
                    "Size",
                    PropertyValue::Number(OrderedFloat(100.0)),
                ));

                defs.extend(Self::get_transform_definitions());
            }
            TrackClipKind::Shape => {
                defs.push(PropertyDefinition::new(
                    "path",
                    PropertyUiType::Text, // Or specialized Path editor if we had one
                    "Path Data",
                    PropertyValue::String("".to_string()),
                ));
                defs.push(PropertyDefinition::new(
                    "width",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 10000.0,
                        step: 1.0,
                        suffix: "px".into(),
                        min_hard_limit: false,
                        max_hard_limit: false,
                    },
                    "Width",
                    PropertyValue::Number(OrderedFloat(100.0)),
                ));
                defs.push(PropertyDefinition::new(
                    "height",
                    PropertyUiType::Float {
                        min: 0.0,
                        max: 10000.0,
                        step: 1.0,
                        suffix: "px".into(),
                        min_hard_limit: false,
                        max_hard_limit: false,
                    },
                    "Height",
                    PropertyValue::Number(OrderedFloat(100.0)),
                ));

                defs.extend(Self::get_transform_definitions());
            }
            TrackClipKind::SkSL => {
                defs.push(PropertyDefinition::new(
                    "shader",
                    PropertyUiType::MultilineText,
                    "Shader Code",
                    PropertyValue::String("".to_string()),
                ));
                defs.extend(Self::get_transform_definitions());
            }
            _ => {}
        }

        defs
    }
}

const fn default_fps() -> f64 {
    30.0
}
