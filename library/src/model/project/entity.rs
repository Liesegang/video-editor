use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid; // Added Uuid import

use crate::model::project::property::{Property, PropertyMap, PropertyValue};

#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct EffectConfig {
    #[serde(rename = "type")]
    pub effect_type: String,
    #[serde(default)]
    pub properties: PropertyMap,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct Entity {
    pub id: Uuid, // Added UUID field
    pub entity_type: String,

    #[serde(default)]
    pub in_frame: u64, // Timeline start frame
    #[serde(default)]
    pub out_frame: u64, // Timeline end frame

    #[serde(default)]
    pub source_begin_frame: u64, // Frame where source content begins (Timeline coordinate of Source Frame 0)
    #[serde(default)]
    pub duration_frame: Option<u64>, // Duration of source content in frames, None for static/infinite

    #[serde(default)]
    pub fps: f64, // Source content FPS

    #[serde(default)]
    pub properties: PropertyMap,

    #[serde(default)]
    pub effects: Vec<EffectConfig>,

    #[serde(default)]
    pub custom_data: HashMap<String, PropertyValue>,
}

impl Entity {
    pub fn new(entity_type: &str) -> Self {
        Self {
            id: Uuid::new_v4(), // Initialize with a new UUID
            entity_type: entity_type.to_string(),
            in_frame: 0,
            out_frame: 0,
            source_begin_frame: 0,
            duration_frame: None,
            fps: 0.0,
            properties: PropertyMap::new(),
            effects: Vec::new(),
            custom_data: HashMap::new(),
        }
    }

    pub fn set_property(&mut self, key: &str, property: Property) {
        self.properties.set(key.to_string(), property);
    }

    pub fn set_constant_property(&mut self, key: &str, value: PropertyValue) {
        self.properties
            .set(key.to_string(), Property::constant(value));
    }

    // Helper constructors updated for frame-based timing
    pub fn create_video(
        file_path: &str,
        in_frame: u64,
        out_frame: u64,
        source_begin_frame: u64,
        duration_frame: u64,
    ) -> Self {
        let mut entity = Entity::new("video");
        entity.in_frame = in_frame;
        entity.out_frame = out_frame;
        entity.source_begin_frame = source_begin_frame;
        entity.duration_frame = Some(duration_frame);
        entity.set_constant_property("file_path", PropertyValue::String(file_path.to_string()));
        entity
    }

    pub fn create_image(file_path: &str, in_frame: u64, out_frame: u64) -> Self {
        let mut entity = Entity::new("image");
        entity.in_frame = in_frame;
        entity.out_frame = out_frame;
        entity.duration_frame = None; // Image is static
        entity.set_constant_property("file_path", PropertyValue::String(file_path.to_string()));
        entity
    }

    pub fn create_text(text: &str, in_frame: u64, out_frame: u64) -> Self {
        let mut entity = Entity::new("text");
        entity.in_frame = in_frame;
        entity.out_frame = out_frame;
        entity.duration_frame = None; // Text is static
        entity.set_constant_property("text", PropertyValue::String(text.to_string()));
        entity
    }
}
