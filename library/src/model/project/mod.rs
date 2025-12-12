pub mod asset;
pub mod project;
pub mod property;

use crate::model::project::property::PropertyMap;
use ordered_float::OrderedFloat;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, PartialEq)]
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
            TrackClipKind::Composition => "composition",
        };
        write!(f, "{}", s)
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
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
            0.0,
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
        props.set(
            "text".to_string(),
            crate::model::project::property::Property::constant(
                crate::model::project::property::PropertyValue::String(text.to_string()),
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
