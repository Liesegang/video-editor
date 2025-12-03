use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use log::{warn};

use crate::model::frame::entity::FrameEntity;
use crate::model::frame::transform::Transform;
use crate::model::property::{Property, PropertyMap, PropertyValue};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Entity {
  pub entity_type: String,

  #[serde(default)]
  pub start_time: f64,
  #[serde(default)]
  pub end_time: f64,

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
      properties: PropertyMap::new(),
      custom_data: HashMap::new(),
    }
  }

  pub fn to_frame_entity(&self, time: f64) -> Option<FrameEntity> {
    match self.entity_type.as_str() {
      "video" => {
        let file_path = match self.properties.get_value("file_path", time) {
          Some(PropertyValue::String(path)) => path,
          _ => return None,
        };

        let frame_number = self.properties.get_number("frame", time, 0.0) as u64;
        let (pos_x, pos_y) = self.properties.get_vec2("position", time, 0.0, 0.0);
        let (scale_x, scale_y) = self.properties.get_vec2("scale", time, 1.0, 1.0);
        let (anchor_x, anchor_y) = self.properties.get_vec2("anchor", time, 0.0, 0.0);
        let rotation = self.properties.get_number("rotation", time, 0.0);

        let transform = Transform {
          position: crate::model::frame::transform::Position { x: pos_x, y: pos_y },
          scale: crate::model::frame::transform::Scale {
            x: scale_x,
            y: scale_y,
          },
          anchor: crate::model::frame::transform::Position {
            x: anchor_x,
            y: anchor_y,
          },
          rotation,
        };

        Some(FrameEntity::Video {
          file_path,
          frame_number,
          transform,
        })
      }
      "image" => {
        let file_path = match self.properties.get_value("file_path", time) {
          Some(PropertyValue::String(path)) => path,
          _ => return None,
        };

        let (pos_x, pos_y) = self.properties.get_vec2("position", time, 0.0, 0.0);
        let (scale_x, scale_y) = self.properties.get_vec2("scale", time, 1.0, 1.0);
        let (anchor_x, anchor_y) = self.properties.get_vec2("anchor", time, 0.0, 0.0);
        let rotation = self.properties.get_number("rotation", time, 0.0);

        let transform = Transform {
          position: crate::model::frame::transform::Position { x: pos_x, y: pos_y },
          scale: crate::model::frame::transform::Scale {
            x: scale_x,
            y: scale_y,
          },
          anchor: crate::model::frame::transform::Position {
            x: anchor_x,
            y: anchor_y,
          },
          rotation,
        };

        Some(FrameEntity::Image {
          file_path,
          transform,
        })
      }
      "text" => {
        let text = match self.properties.get_value("text", time) {
          Some(PropertyValue::String(text)) => text,
          _ => return None,
        };

        let font = match self.properties.get_value("font", time) {
          Some(PropertyValue::String(font)) => font,
          _ => "Arial".to_string(),
        };

        let size = self.properties.get_number("size", time, 12.0);

        let color = match self.properties.get_value("color", time) {
          Some(PropertyValue::Color { r, g, b, a }) => {
            crate::model::frame::color::Color { r, g, b, a }
          }
          _ => crate::model::frame::color::Color {
            r: 0,
            g: 0,
            b: 0,
            a: 255,
          },
        };

        let (pos_x, pos_y) = self.properties.get_vec2("position", time, 0.0, 0.0);
        let (scale_x, scale_y) = self.properties.get_vec2("scale", time, 1.0, 1.0);
        let (anchor_x, anchor_y) = self.properties.get_vec2("anchor", time, 0.0, 0.0);
        let rotation = self.properties.get_number("rotation", time, 0.0);

        let transform = Transform {
          position: crate::model::frame::transform::Position { x: pos_x, y: pos_y },
          scale: crate::model::frame::transform::Scale {
            x: scale_x,
            y: scale_y,
          },
          anchor: crate::model::frame::transform::Position {
            x: anchor_x,
            y: anchor_y,
          },
          rotation,
        };

        Some(FrameEntity::Text {
          text,
          font,
          size,
          color,
          transform,
        })
      }
      "shape" => {
        let path = match self.properties.get_value("path", time) {
          Some(PropertyValue::String(path)) => path,
          _ => return None,
        };

        let (pos_x, pos_y) = self.properties.get_vec2("position", time, 0.0, 0.0);
        let (scale_x, scale_y) = self.properties.get_vec2("scale", time, 1.0, 1.0);
        let (anchor_x, anchor_y) = self.properties.get_vec2("anchor", time, 0.0, 0.0);
        let rotation = self.properties.get_number("rotation", time, 0.0);

        let transform = Transform {
          position: crate::model::frame::transform::Position { x: pos_x, y: pos_y },
          scale: crate::model::frame::transform::Scale {
            x: scale_x,
            y: scale_y,
          },
          anchor: crate::model::frame::transform::Position {
            x: anchor_x,
            y: anchor_y,
          },
          rotation,
        };

        let styles_prop = self.properties.get_value("styles", time).unwrap_or(PropertyValue::Array(vec![]));
        let styles = if let PropertyValue::Array(arr) = styles_prop {
             arr.iter().filter_map(|v| {
                 let json_val: serde_json::Value = v.into();
                 match serde_json::from_value(json_val) {
                    Ok(s) => Some(s),
                    Err(e) => {
                        warn!("Failed to parse style: {}", e);
                        None
                    }
                 }
             }).collect()
        } else {
            vec![]
        };

        let effects_prop = self.properties.get_value("path_effects", time).unwrap_or(PropertyValue::Array(vec![]));
        let path_effects = if let PropertyValue::Array(arr) = effects_prop {
             arr.iter().filter_map(|v| {
                 let json_val: serde_json::Value = v.into();
                 match serde_json::from_value(json_val) {
                    Ok(e) => Some(e),
                    Err(err) => {
                        warn!("Failed to parse path effect: {}", err);
                        None
                    }
                 }
             }).collect()
        } else {
            vec![]
        };

        Some(FrameEntity::Shape {
          path,
          transform,
          styles,
          path_effects,
        })
      }
      _ => None,
    }
  }

  pub fn set_property(&mut self, key: &str, property: Property) {
    self.properties.set(key.to_string(), property);
  }

  pub fn set_constant_property(&mut self, key: &str, value: PropertyValue) {
    self
      .properties
      .set(key.to_string(), Property::Constant { value });
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
