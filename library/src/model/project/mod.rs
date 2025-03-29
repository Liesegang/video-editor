use serde::{Deserialize, Serialize};
use crate::model::frame::color::Color;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Project {
  pub name: String,
  pub compositions: Vec<Composition>,
}

impl Project {
  pub fn load(json_str: &str) -> Result<Project, serde_json::Error> {
    serde_json::from_str(json_str)
  }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Composition {
  pub name: String,
  pub width: u64,
  pub height: u64,
  pub fps: f64,
  pub duration: u64,
  pub background_color: Color,
  pub color_profile: String,
  pub tracks: Vec<Track>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Track {
  pub name: String,
  pub entities: Vec<TrackEntity>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TimeRange {
  pub start: u64,
  pub end: u64,
  pub fps: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Property<T> {
  Constant {
    value: T,
  },
  Keyframe {
    keyframes: Vec<Keyframe<T>>,
  },
  Expression {
    expression: String,
  },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Keyframe<T> {
  pub time: u64,
  pub value: T,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PositionProperty {
  pub x: Property<f64>,
  pub y: Property<f64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ScaleProperty {
  pub x: Property<f64>,
  pub y: Property<f64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Transform {
  pub position: PositionProperty,
  pub scale: ScaleProperty,
  pub rotation: Property<f64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum TrackEntity {
  Video {
    file_path: String,
    #[serde(flatten)]
    time_range: TimeRange,
    #[serde(flatten)]
    transform: Transform,
  },
  Image {
    file_path: String,
    #[serde(flatten)]
    time_range: TimeRange,
    #[serde(flatten)]
    transform: Transform,
  },
  // Text {
  //   text: String,
  //   font: String,
  //   #[serde(flatten)]
  //   time_range: TimeRange,
  // #[serde(flatten)]
  // transform: Transform,
  // },
  // Shape {
  //   path: String,
  //   styles: Vec<DrawStyle>,
  //   path_effects: Vec<PathEffect>,
  // #[serde(flatten)]
  // transform: Transform,
  // },
}

/* sample json
{
  "name": "My Project",
  "compositions": [
    {
      "name": "My Composition",
      "width": 1920,
      "height": 1080,
      "background_color": {
        "r": 0,
        "g": 0,
        "b": 0,
        "a": 255
      },
      "color_profile": "srgb",
      "tracks": [
        {
          "name": "My Track",
          "entities": [
            {
              "type": "video",
              "file_path": "path/to/video.mp4",
              "time_range": {
                "start": 0,
                "end": 100,
                "fps": 24
              },
              "transform": {
                "position": {
                  "x": {
                    "type": "constant",
                    "value": 0
                  },
                  "y": {
                    "type": "constant",
                    "value": 0
                  }
                },
                "scale": {
                  "x": {
                    "type": "constant",
                    "value": 1
                  },
                  "y": {
                    "type": "constant",
                    "value": 1
                  }
                },
                "rotation": {
                  "type": "constant",
                  "value": 0
                }
              }
            }
          ]
        }
      ]
    }
  ]
}
*/