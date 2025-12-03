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
  pub properties: PropertyMap,
}

pub const START_TIME_KEY: &str = "__start_time";
pub const END_TIME_KEY: &str = "__end_time";
pub const FPS_KEY: &str = "__fps";
