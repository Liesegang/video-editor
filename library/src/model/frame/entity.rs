use crate::model::frame::color::Color;
use crate::model::frame::draw_type::{DrawStyle, PathEffect};
use crate::model::frame::effect::ImageEffect;
use crate::model::frame::transform::Transform;
use crate::model::project::property::PropertyMap;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[derive(Debug)]
pub struct ImageSurface {
    #[serde(rename = "file_path")]
    pub file_path: String,
    #[serde(default)]
    pub effects: Vec<ImageEffect>,
    #[serde(flatten)]
    pub transform: Transform,
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
#[serde(tag = "type")]
#[derive(Debug)]
pub enum FrameContent {
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

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct FrameObject {
    pub content: FrameContent, // Renamed from entity: FrameEntity
    pub properties: PropertyMap,
}

pub trait ImageContent {
    fn get_surface(&self) -> Option<&ImageSurface>;
}

impl ImageContent for FrameContent {
    fn get_surface(&self) -> Option<&ImageSurface> {
        match self {
            FrameContent::Video { surface, .. } => Some(surface),
            FrameContent::Image { surface } => Some(surface),
            _ => None,
        }
    }
}
