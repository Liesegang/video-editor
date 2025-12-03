use crate::model::frame::color::Color;
use crate::model::frame::draw_type::{DrawStyle, PathEffect};
use crate::model::frame::transform::Transform;
use crate::model::project::property::PropertyMap;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum FrameEntity {
  Video {
    file_path: String,
    frame_number: u64,
    #[serde(flatten)]
    transform: Transform,
  },
  Image {
    file_path: String,
    #[serde(flatten)]
    transform: Transform,
  },
  Text {
    text: String,
    font: String,
    size: f64,
    color: Color,
    #[serde(flatten)]
    transform: Transform,
  },
  Shape {
    path: String,
    styles: Vec<DrawStyle>,
    path_effects: Vec<PathEffect>,
    #[serde(flatten)]
    transform: Transform,
  },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FrameObject {
  pub entity: FrameEntity,
  pub properties: PropertyMap,
}
