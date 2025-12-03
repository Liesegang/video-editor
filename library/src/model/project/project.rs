use crate::model::frame::color::Color;
use crate::model::frame::entity::FrameEntity::{Image, Shape, Text, Video};
use crate::model::frame::frame::FrameInfo;
use serde::{Deserialize, Serialize};

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
    serde_json::from_str(json_str)
  }

  pub fn save(&self) -> Result<String, serde_json::Error> {
    serde_json::to_string(self)
  }

  pub fn add_composition(&mut self, composition: Composition) {
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
    }
  }

  pub fn add_track(&mut self, track: Track) {
    self.tracks.push(track);
  }

  pub fn render_frame(&self, time: f64) -> FrameInfo {
    let mut frame = FrameInfo {
      width: self.width,
      height: self.height,
      background_color: self.background_color.clone(),
      color_profile: self.color_profile.clone(),
      objects: Vec::new(),
    };

    for track in self.tracks.iter() {
      for entity in track.entities.iter() {
        match entity {
          TrackEntity::Video {
            file_path,
            time_range,
            transform,
            zero,
          } => {
            if time_range.start <= time && time_range.end >= time {
              let video = Video {
                file_path: file_path.clone(),
                frame_number: (zero + time - time_range.start) as u64,
                transform: transform.get_value(time),
              };
              frame.objects.push(video);
            }
          }
          TrackEntity::Image {
            file_path,
            time_range,
            transform,
          } => {
            if time_range.start <= time && time_range.end >= time {
              let image = Image {
                file_path: file_path.clone(),
                transform: transform.get_value(time),
              };
              frame.objects.push(image);
            }
          }
          TrackEntity::Text {
            text,
            font,
            size,
            color,
            time_range,
            transform,
          } => {
            if time_range.start <= time && time_range.end >= time {
              let text_entity = Text {
                text: text.clone(),
                font: font.clone(),
                size: size.get_value(time),
                color: color.clone(),
                transform: transform.get_value(time),
              };
              frame.objects.push(text_entity);
            }
          }
          TrackEntity::Shape {
            path,
            styles,
            path_effects,
            time_range,
            transform,
          } => {
            if time_range.start <= time && time_range.end >= time {
              let shape = Shape {
                path: path.clone(),
                styles: styles.clone(),
                path_effects: path_effects.clone(),
                transform: transform.get_value(time),
              };
              frame.objects.push(shape);
            }
          }
        }
      }
    }

    frame
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
