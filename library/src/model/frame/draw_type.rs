use serde::{Deserialize, Serialize};
use crate::model::frame::color::Color;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum JoinType {
    Round,
    Bevel,
    Miter,
}

impl Default for JoinType {
    fn default() -> Self {
        Self::Round
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum CapType {
    Round,
    Square,
    Butt,
}

impl Default for CapType {
    fn default() -> Self {
        Self::Square
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum DrawStyle {
    Fill {
        color: Color,
    },
    Stroke {
        #[serde(default)]
        color: Color,
        #[serde(default)]
        width: f32,
        #[serde(default)]
        cap: CapType,
        #[serde(default)]
        join: JoinType,
        #[serde(default)]
        miter: f32,
    }
}

impl Default for DrawStyle {
    fn default() -> Self {
        Self::Fill {
            color: Color { r: 255, g: 255, b: 255, a: 255 }
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(tag = "type")]
pub enum PathEffect {
    Dash {
        intervals: Vec<f32>,
        phase: f32,
    },
    Corner {
        radius: f32,
    },
    Discrete {
        seg_length: f32,
        deviation: f32,
        seed: u32,
    },
    Trim {
        start: f32,
        end: f32,
    },
}