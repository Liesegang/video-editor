use crate::model::frame::color::Color;
use crate::model::frame::draw_type::{DrawStyle, PathEffect};
use crate::model::frame::effect::ImageEffect;
use crate::model::frame::transform::Transform;
use crate::model::project::property::PropertyMap;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ImageSurface {
    #[serde(rename = "file_path")]
    pub file_path: String,
    #[serde(default)]
    pub effects: Vec<ImageEffect>,
    #[serde(flatten)]
    pub transform: Transform,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum FrameEntity {
    Video {
        #[serde(flatten)]
        surface: ImageSurface,
        frame_number: u64,
    },
    Image {
        #[serde(flatten)]
        surface: ImageSurface,
    },
    Text {
        text: String,
        font: String,
        size: f64,
        color: Color,
        #[serde(default)]
        effects: Vec<ImageEffect>,
        #[serde(flatten)]
        transform: Transform,
    },
    Shape {
        path: String,
        styles: Vec<DrawStyle>,
        path_effects: Vec<PathEffect>,
        #[serde(default)]
        effects: Vec<ImageEffect>,
        #[serde(flatten)]
        transform: Transform,
    },
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FrameObject {
    pub entity: FrameEntity,
    pub properties: PropertyMap,
}

pub trait ImageEntity {
    fn surface(&self) -> Option<&ImageSurface>;
}

impl ImageEntity for FrameEntity {
    fn surface(&self) -> Option<&ImageSurface> {
        match self {
            FrameEntity::Video { surface, .. } => Some(surface),
            FrameEntity::Image { surface } => Some(surface),
            _ => None,
        }
    }
}
