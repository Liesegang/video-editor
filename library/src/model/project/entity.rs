use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::model::project::property::{Property, PropertyMap, PropertyValue};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Entity {
  pub entity_type: String,

  #[serde(default)]
  pub start_time: f64,
  #[serde(default)]
  pub end_time: f64,
  #[serde(default)]
  pub fps: f64,

  #[serde(default)]
  pub properties: PropertyMap,

  #[serde(default)]
  pub custom_data: HashMap<String, PropertyValue>,
}

impl Entity {
  pub fn new(entity_type: &str) -> Self {
    Self {
      entity_type: entity_type.to_string(),
      start_time: 0.0,
      end_time: 0.0,
      fps: 0.0,
      properties: PropertyMap::new(),
      custom_data: HashMap::new(),
    }
  }

  pub fn set_property(&mut self, key: &str, property: Property) {
    self.properties.set(key.to_string(), property);
  }

  pub fn set_constant_property(&mut self, key: &str, value: PropertyValue) {
    self
      .properties
      .set(key.to_string(), Property::constant(value));
  }

  pub fn create_video(file_path: &str, start_time: f64, end_time: f64) -> Self {
    let mut entity = Entity::new("video");
    entity.start_time = start_time;
    entity.end_time = end_time;
    entity.set_constant_property("file_path", PropertyValue::String(file_path.to_string()));
    entity
  }

  pub fn create_image(file_path: &str, start_time: f64, end_time: f64) -> Self {
    let mut entity = Entity::new("image");
    entity.start_time = start_time;
    entity.end_time = end_time;
    entity.set_constant_property("file_path", PropertyValue::String(file_path.to_string()));
    entity
  }

  pub fn create_text(text: &str, start_time: f64, end_time: f64) -> Self {
    let mut entity = Entity::new("text");
    entity.start_time = start_time;
    entity.end_time = end_time;
    entity.set_constant_property("text", PropertyValue::String(text.to_string()));
    entity
  }
}
