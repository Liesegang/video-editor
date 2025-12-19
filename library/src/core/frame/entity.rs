use crate::core::frame::draw_type::{DrawStyle, PathEffect};
use crate::core::frame::effect::ImageEffect;
use crate::core::frame::transform::Transform;
use crate::core::model::property::PropertyMap;
use serde::{Deserialize, Serialize};

use ordered_float::OrderedFloat;
use std::hash::{Hash, Hasher};

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
pub struct ImageSurface {
    #[serde(rename = "file_path")]
    pub file_path: String,
    #[serde(default)]
    pub effects: Vec<ImageEffect>,
    #[serde(flatten)]
    pub transform: Transform,
}

use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug)]
pub struct StyleConfig {
    pub id: Uuid,
    pub style: DrawStyle,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type")]
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
        #[serde(default)]
        styles: Vec<StyleConfig>,
        #[serde(default)]
        effects: Vec<ImageEffect>,
        #[serde(flatten)]
        transform: Transform,
    },
    Shape {
        path: String,
        styles: Vec<StyleConfig>,
        path_effects: Vec<PathEffect>,
        #[serde(default)]
        effects: Vec<ImageEffect>,
        #[serde(flatten)]
        transform: Transform,
    },
    SkSL {
        shader: String,
        resolution: (f32, f32),
        #[serde(default)]
        effects: Vec<ImageEffect>,
        #[serde(flatten)]
        transform: Transform,
    },
}

impl Hash for FrameContent {
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            FrameContent::Video {
                surface,
                frame_number,
            } => {
                surface.hash(state);
                frame_number.hash(state);
            }
            FrameContent::Image { surface } => {
                surface.hash(state);
            }
            FrameContent::Text {
                text,
                font,
                size,
                styles,
                effects,
                transform,
            } => {
                text.hash(state);
                font.hash(state);
                OrderedFloat(*size).hash(state);
                styles.hash(state);
                effects.hash(state);
                transform.hash(state);
            }
            FrameContent::Shape {
                path,
                styles,
                path_effects,
                effects,
                transform,
            } => {
                path.hash(state);
                styles.hash(state);
                path_effects.hash(state);
                effects.hash(state);
                transform.hash(state);
            }
            FrameContent::SkSL {
                shader,
                resolution,
                effects,
                transform,
            } => {
                shader.hash(state);
                OrderedFloat(resolution.0).hash(state);
                OrderedFloat(resolution.1).hash(state);
                effects.hash(state);
                transform.hash(state);
            }
        }
    }
}

impl PartialEq for FrameContent {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (
                FrameContent::Video {
                    surface: s1,
                    frame_number: f1,
                },
                FrameContent::Video {
                    surface: s2,
                    frame_number: f2,
                },
            ) => s1 == s2 && f1 == f2,
            (FrameContent::Image { surface: s1 }, FrameContent::Image { surface: s2 }) => s1 == s2,
            (
                FrameContent::Text {
                    text: t1,
                    font: f1,
                    size: s1,
                    styles: st1,
                    effects: e1,
                    transform: tr1,
                },
                FrameContent::Text {
                    text: t2,
                    font: f2,
                    size: s2,
                    styles: st2,
                    effects: e2,
                    transform: tr2,
                },
            ) => {
                t1 == t2
                    && f1 == f2
                    && OrderedFloat(*s1) == OrderedFloat(*s2)
                    && st1 == st2
                    && e1 == e2
                    && tr1 == tr2
            }
            (
                FrameContent::Shape {
                    path: p1,
                    styles: st1,
                    path_effects: pe1,
                    effects: e1,
                    transform: tr1,
                },
                FrameContent::Shape {
                    path: p2,
                    styles: st2,
                    path_effects: pe2,
                    effects: e2,
                    transform: tr2,
                },
            ) => p1 == p2 && st1 == st2 && pe1 == pe2 && e1 == e2 && tr1 == tr2,
            (
                FrameContent::SkSL {
                    shader: s1,
                    resolution: r1,
                    effects: e1,
                    transform: tr1,
                },
                FrameContent::SkSL {
                    shader: s2,
                    resolution: r2,
                    effects: e2,
                    transform: tr2,
                },
            ) => {
                s1 == s2
                    && OrderedFloat(r1.0) == OrderedFloat(r2.0)
                    && OrderedFloat(r1.1) == OrderedFloat(r2.1)
                    && e1 == e2
                    && tr1 == tr2
            }
            _ => false,
        }
    }
}
impl Eq for FrameContent {}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Hash, Debug)] // Added Debug
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
