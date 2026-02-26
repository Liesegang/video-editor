use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::property::PropertyMap;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(rename_all = "lowercase")] // Serialize as "video", "image", etc.
pub enum SourceKind {
    Video,
    Image,
    Audio,
    Text,
    Shape,
    SkSL,
    Composition,
}

impl std::fmt::Display for SourceKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            SourceKind::Video => "video",
            SourceKind::Image => "image",
            SourceKind::Audio => "audio",
            SourceKind::Text => "text",
            SourceKind::Shape => "shape",
            SourceKind::SkSL => "sksl",
            SourceKind::Composition => "composition",
        };
        write!(f, "{}", s)
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct SourceData {
    pub id: Uuid,
    pub reference_id: Option<Uuid>,
    #[serde(rename = "type")]
    pub kind: SourceKind,
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

impl SourceData {
    /// Whether this source produces shape output (e.g. Text, Shape).
    /// Used to determine if shape chain nodes (effectors, decorators, styles) are applicable.
    pub fn has_shape_output(&self) -> bool {
        matches!(self.kind, SourceKind::Text | SourceKind::Shape)
    }

    /// Whether this source produces image output (e.g. Video, Image, SkSL, Composition).
    pub fn has_image_output(&self) -> bool {
        !matches!(self.kind, SourceKind::Audio)
    }

    pub fn new(
        id: Uuid,
        reference_id: Option<Uuid>,
        kind: SourceKind,
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
            PropertyTarget::GraphNode(_) => None,
            _ => None,
        }
    }

    pub fn get_definitions_for_kind(
        kind: &SourceKind,
    ) -> Vec<crate::project::property::PropertyDefinition> {
        use crate::project::property::{PropertyDefinition, PropertyUiType, PropertyValue};
        use ordered_float::OrderedFloat;

        let mut defs = vec![];

        match kind {
            SourceKind::Audio => {
                defs.push(PropertyDefinition::new(
                    "file_path",
                    PropertyUiType::Text,
                    "File Path",
                    PropertyValue::String("".to_string()),
                ));
            }
            SourceKind::Video | SourceKind::Image => {
                defs.push(PropertyDefinition::new(
                    "file_path",
                    PropertyUiType::Text,
                    "File Path",
                    PropertyValue::String("".to_string()),
                ));
            }
            SourceKind::Text => {
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
            }
            SourceKind::Shape => {
                defs.push(PropertyDefinition::new(
                    "path",
                    PropertyUiType::Text,
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
            }
            SourceKind::SkSL => {
                defs.push(PropertyDefinition::new(
                    "shader",
                    PropertyUiType::MultilineText,
                    "Shader Code",
                    PropertyValue::String("".to_string()),
                ));
            }
            _ => {}
        }

        defs
    }
}

const fn default_fps() -> f64 {
    30.0
}
