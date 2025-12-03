pub mod project;
pub mod conversion;

use crate::model::frame::color::Color;
use crate::model::frame::transform::{Position, Scale, Transform};
use serde::{Deserialize, Serialize};

use super::frame::draw_type::{DrawStyle, PathEffect};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Track {
  pub name: String,
  pub entities: Vec<TrackEntity>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TimeRange {
  pub start: f64,
  pub end: f64,
  pub fps: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum Property<T> {
  Constant { value: T },
  Keyframe { keyframes: Vec<Keyframe<T>> },
  Expression { expression: String },
}

impl<
  T: Default
    + Clone
    + std::ops::Add<Output = T>
    + std::ops::Sub<Output = T>
    + std::ops::Mul<f64, Output = T>,
> Property<T>
{
  pub fn get_value(&self, time: f64) -> T {
    match self {
      Property::Constant { value } => value.clone(),
      Property::Keyframe { keyframes } => {
        if keyframes.is_empty() {
          return T::default();
        } else if time <= keyframes[0].time {
          return keyframes[0].value.clone();
        } else if time >= keyframes.last().unwrap().time {
          return keyframes.last().unwrap().value.clone();
        }

        let keyframe = keyframes.iter().rev().find(|k| k.time <= time).unwrap();
        let next_keyframe = keyframes.iter().find(|k| k.time > time).unwrap();

        let t = (time - keyframe.time) / (next_keyframe.time - keyframe.time);
        keyframe.value.clone() + (next_keyframe.value.clone() - keyframe.value.clone()) * t
      }
      Property::Expression { expression } => eval(expression),
    }
  }
}

fn eval<T>(_expression: &str) -> T {
  unimplemented!()
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Keyframe<T> {
  pub time: f64,
  pub value: T,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct PositionProperty {
  pub x: Property<f64>,
  pub y: Property<f64>,
}

impl PositionProperty {
  pub fn get_value(&self, time: f64) -> Position {
    Position {
      x: self.x.get_value(time),
      y: self.y.get_value(time),
    }
  }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ScaleProperty {
  pub x: Property<f64>,
  pub y: Property<f64>,
}

impl ScaleProperty {
  pub fn get_value(&self, time: f64) -> Scale {
    Scale {
      x: self.x.get_value(time),
      y: self.y.get_value(time),
    }
  }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TransformProperty {
  pub position: PositionProperty,
  pub scale: ScaleProperty,
  pub anchor: PositionProperty,
  pub rotation: Property<f64>,
}

impl TransformProperty {
  pub fn get_value(&self, time: f64) -> Transform {
    Transform {
      position: self.position.get_value(time),
      scale: self.scale.get_value(time),
      anchor: self.anchor.get_value(time),
      rotation: self.rotation.get_value(time),
    }
  }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum TrackEntity {
  Video {
    file_path: String,
    zero: f64,
    #[serde(flatten)]
    time_range: TimeRange,
    #[serde(flatten)]
    transform: TransformProperty,
  },
  Image {
    file_path: String,
    #[serde(flatten)]
    time_range: TimeRange,
    #[serde(flatten)]
    transform: TransformProperty,
  },
  Text {
    text: String,
    font: String,
    size: Property<f64>,
    color: Color,
    #[serde(flatten)]
    time_range: TimeRange,
    #[serde(flatten)]
    transform: TransformProperty,
  },
  Shape {
    path: String,
    styles: Vec<DrawStyle>,
    path_effects: Vec<PathEffect>,
    #[serde(flatten)]
    time_range: TimeRange,
    #[serde(flatten)]
    transform: TransformProperty,
  },
}
