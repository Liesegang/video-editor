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
    pub start_time: f64,
    #[serde(default)]
    pub end_time: f64,
    #[serde(default = "default_fps")]
    pub fps: f64,
    #[serde(default)]
    pub properties: PropertyMap,
    #[serde(default)]
    pub effects: Vec<EffectConfig>,
}

impl TrackEntity {
    pub fn new(id: Uuid, entity_type: String, start_time: f64, end_time: f64, fps: f64, properties: PropertyMap, effects: Vec<EffectConfig>) -> Self {
        Self {
            id,
            entity_type,
            start_time,
            end_time,
            fps,
            properties,
            effects,
        }
    }
}

const fn default_fps() -> f64 {
    0.0
}
