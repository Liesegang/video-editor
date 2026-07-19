use crate::model::frame::color::Color;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Asset {
    pub id: Uuid,
    pub name: String,
    pub path: String, // Path to the file
    pub kind: AssetKind,
    pub duration: Option<f64>, // Duration in seconds, if applicable
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f64>,
    /// Exact number of decodable source video frames, when the loader can
    /// determine it. Valid source frame indices use the half-open range
    /// `0..frame_count`; duration is not a substitute for this bound.
    #[serde(default)]
    pub frame_count: Option<u64>,

    // Metadata
    #[serde(default)]
    pub color: Color,
    #[serde(default)]
    pub stream_index: Option<usize>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub enum AssetKind {
    Video,
    Audio,
    Image,
    Model3D,
    Other,
}

impl Asset {
    pub fn new(name: &str, path: &str, kind: AssetKind) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            path: path.to_string(),
            kind,
            duration: None,
            width: None,
            height: None,
            fps: None,
            frame_count: None,
            color: Color {
                r: 100,
                g: 100,
                b: 100,
                a: 255,
            }, // Default gray
            stream_index: None,
        }
    }

    /// Maps source-local seconds to a source frame using the Asset FPS when
    /// valid, otherwise the caller's evaluation FPS. This mapping is shared
    /// by graph evaluation and the video entity converter so the persisted
    /// `frame_count` bound cannot disagree with the eventual load request.
    pub fn source_frame_number_at(&self, time: f64, evaluation_fps: f64) -> Option<u64> {
        if !time.is_finite() || time < 0.0 {
            return None;
        }
        let fps = self
            .fps
            .filter(|fps| fps.is_finite() && *fps > 0.0)
            .or_else(|| {
                (evaluation_fps.is_finite() && evaluation_fps > 0.0).then_some(evaluation_fps)
            })?;
        Some((time * fps).floor() as u64)
    }

    pub fn contains_source_frame(&self, frame_number: u64) -> bool {
        self.frame_count
            .is_none_or(|frame_count| frame_number < frame_count)
    }
}
