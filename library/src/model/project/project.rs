use serde::{Deserialize, Serialize};

use crate::model::frame::color::Color;
use crate::model::project::entity::Entity;

use super::Track;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Project {
  pub name: String,
  pub compositions: Vec<Composition>,
}

impl Project {
  pub fn new(name: &str) -> Self {
    Self {
      name: name.to_string(),
      compositions: Vec::new(),
    }
  }

  pub fn load(json_str: &str) -> Result<Self, serde_json::Error> {
    let mut project: Project = serde_json::from_str(json_str)?;
    for composition in project.compositions.iter_mut() {
      composition.rebuild_entity_cache();
    }
    Ok(project)
  }

  pub fn save(&self) -> Result<String, serde_json::Error> {
    serde_json::to_string(self)
  }

  pub fn add_composition(&mut self, composition: Composition) {
    let mut composition = composition;
    composition.rebuild_entity_cache();
    self.compositions.push(composition);
  }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Composition {
  pub name: String,
  pub width: u64,
  pub height: u64,
  pub fps: f64,
  pub duration: f64,
  pub background_color: Color,
  pub color_profile: String,

  pub tracks: Vec<Track>,

  #[serde(skip)]
  cached_entities: Vec<Entity>,
}

impl Composition {
  pub fn new(name: &str, width: u64, height: u64, fps: f64, duration: f64) -> Self {
    Self {
      name: name.to_string(),
      width,
      height,
      fps,
      duration,
      background_color: Color {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
      },
      color_profile: "sRGB".to_string(),
      tracks: Vec::new(),
      cached_entities: Vec::new(),
    }
  }

  pub fn add_track(&mut self, track: Track) {
    self.tracks.push(track);
    self.rebuild_entity_cache();
  }

  pub(crate) fn cached_entities(&self) -> &[Entity] {
    &self.cached_entities
  }

  pub fn rebuild_entity_cache(&mut self) {
    self.cached_entities = self
      .tracks
      .iter()
      .flat_map(|track| track.entities.iter())
      .map(|track_entity| track_entity.into())
      .collect();
  }
}
