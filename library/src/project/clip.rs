use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::property::PropertyMap;

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
        }
    }

    // Helper for consistency with Entity
    pub fn set_constant_property(
        &mut self,
        key: &str,
        value: crate::project::property::PropertyValue,
    ) {
        self.properties.set(
            key.to_string(),
            crate::project::property::Property::constant(value),
        );
    }

    /// Unified accessor for property maps.
    /// For GraphNode targets, use `Project::get_graph_node_mut()` instead.
    pub fn get_property_map_mut(
        &mut self,
        target: crate::project::property::PropertyTarget,
    ) -> Option<&mut crate::project::property::PropertyMap> {
        use crate::project::property::PropertyTarget;
        match target {
            PropertyTarget::Clip => Some(&mut self.properties),
            PropertyTarget::GraphNode(_) => None, // GraphNode properties are accessed via Project.nodes
            _ => None,
        }
    }

    pub(crate) fn get_transform_definitions() -> Vec<crate::project::property::PropertyDefinition> {
        use crate::project::property::{PropertyDefinition, PropertyUiType, PropertyValue, Vec2};
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
    ) -> Vec<crate::project::property::PropertyDefinition> {
        use crate::project::property::{PropertyDefinition, PropertyUiType, PropertyValue};
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
