use serde::{Deserialize, Serialize};

use crate::model::frame::color::Color;
use crate::model::frame::frame::FrameInfo;
use crate::model::project::entity::Entity;

use super::{
  PositionProperty, Property, ScaleProperty, TimeRange, Track, TrackEntity, TransformProperty,
};

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

  pub fn render_frame(&self, time: f64) -> FrameInfo {
    let mut frame = FrameInfo {
      width: self.width,
      height: self.height,
      background_color: self.background_color.clone(),
      color_profile: self.color_profile.clone(),
      objects: Vec::new(),
    };

    for entity in &self.cached_entities {
      if entity.start_time <= time && entity.end_time >= time {
        if let Some(frame_entity) = entity.to_frame_entity(time) {
          frame.objects.push(frame_entity);
        }
      }
    }

    frame
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

impl Project {
  pub fn create_sample() -> Self {
    let mut project = Project::new("サンプルプロジェクト");

    let mut composition = Composition::new("メイン", 1920, 1080, 30.0, 10.0);

    let text_entity = TrackEntity::Text {
      text: "サンプルテキスト".to_string(),
      font: "sans-serif".to_string(),
      size: Property::Constant { value: 48.0 },
      color: Color {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
      },
      time_range: TimeRange {
        start: 0.0,
        end: 5.0,
        fps: 30.0,
      },
      transform: TransformProperty {
        position: PositionProperty {
          x: Property::Constant { value: 960.0 },
          y: Property::Constant { value: 540.0 },
        },
        scale: ScaleProperty {
          x: Property::Constant { value: 1.0 },
          y: Property::Constant { value: 1.0 },
        },
        anchor: PositionProperty {
          x: Property::Constant { value: 0.0 },
          y: Property::Constant { value: 0.0 },
        },
        rotation: Property::Constant { value: 0.0 },
      },
    };

    let image_entity = TrackEntity::Image {
      file_path: "sample.png".to_string(),
      time_range: TimeRange {
        start: 1.0,
        end: 8.0,
        fps: 30.0,
      },
      transform: TransformProperty {
        position: PositionProperty {
          x: Property::Constant { value: 960.0 },
          y: Property::Constant { value: 540.0 },
        },
        scale: ScaleProperty {
          x: Property::Constant { value: 1.0 },
          y: Property::Constant { value: 1.0 },
        },
        anchor: PositionProperty {
          x: Property::Constant { value: 0.0 },
          y: Property::Constant { value: 0.0 },
        },
        rotation: Property::Constant { value: 0.0 },
      },
    };

    let track = Track {
      name: "トラック 1".to_string(),
      entities: vec![text_entity, image_entity],
    };

    composition.add_track(track);

    project.add_composition(composition);
    project
  }
}
