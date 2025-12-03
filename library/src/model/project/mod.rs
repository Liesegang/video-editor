pub mod conversion;
pub mod entity;
pub mod project;
pub mod property;

use crate::model::project::property::PropertyMap;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Track {
  pub name: String,
  pub entities: Vec<TrackEntity>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TrackEntity {
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
}

const fn default_fps() -> f64 {
  0.0
}
