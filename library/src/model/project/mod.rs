use serde::{Deserialize, Serialize};
use crate::model::frame::color::Color;
use crate::model::frame::entity::FrameEntity;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Project {
  pub name: String,
  pub compositions: Vec<Composition>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Composition {
  pub name: String,
  pub width: u32,
  pub height: u32,
  pub background_color: Color,
  pub color_profile: String,
  pub objects: Vec<FrameEntity>,
}