pub mod conversion;
pub mod entity;
pub mod project;
pub mod property;

use crate::model::project::entity::EffectConfig;
use crate::model::project::property::PropertyMap;
use serde::{Deserialize, Serialize};
use uuid::Uuid; // Added Uuid import

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct Track {
    pub id: Uuid, // Added UUID field
    pub name: String,
    pub entities: Vec<TrackEntity>,
}

impl Track {
    pub fn new(name: &str) -> Self {
        Self {
            id: Uuid::new_v4(), // Initialize with a new UUID
            name: name.to_string(),
            entities: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct TrackEntity {
    pub id: Uuid, // Added UUID field
    #[serde(rename = "type")]
    pub entity_type: String,
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

impl TrackEntity {
    pub fn new(
        id: Uuid,
        entity_type: String,
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
            entity_type,
            in_frame,
            out_frame,
            source_begin_frame,
            duration_frame,
            fps,
            properties,
            effects,
        }
    }
}

const fn default_fps() -> f64 {
    0.0
}
