use std::collections::HashMap;
use uuid::Uuid;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{Track, TrackItem};
use crate::model::frame::color::Color;

use crate::model::project::asset::Asset;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Project {
    pub name: String,
    pub compositions: Vec<Composition>,
    #[serde(default)]
    pub assets: Vec<Asset>,
    #[serde(default)]
    pub export: ExportConfig,
}

#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
pub struct ExportConfig {
    #[serde(default)]
    pub container: Option<String>,
    #[serde(default)]
    pub codec: Option<String>,
    #[serde(default)]
    pub pixel_format: Option<String>,
    #[serde(default)]
    pub width: Option<u64>,
    #[serde(default)]
    pub height: Option<u64>,
    #[serde(default)]
    pub fps: Option<f64>,
    #[serde(default)]
    pub video_bitrate: Option<u64>,
    #[serde(default)]
    pub audio_codec: Option<String>,
    #[serde(default)]
    pub audio_bitrate: Option<u64>,
    #[serde(default)]
    pub audio_channels: Option<u16>,
    #[serde(default)]
    pub audio_sample_rate: Option<u32>,
    #[serde(default)]
    pub crf: Option<u8>,
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub ffmpeg_path: Option<String>,
    #[serde(default)]
    pub parameters: HashMap<String, Value>,
}

impl Project {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            compositions: Vec::new(),
            assets: Vec::new(),
            export: ExportConfig::default(),
        }
    }

    pub fn load(json_str: &str) -> Result<Self, serde_json::Error> {
        let project: Project = serde_json::from_str(json_str)?;

        Ok(project)
    }

    pub fn save(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn add_composition(&mut self, composition: Composition) {
        self.compositions.push(composition);
    }

    pub fn get_composition_mut(&mut self, id: Uuid) -> Option<&mut Composition> {
        self.compositions.iter_mut().find(|c| c.id == id)
    }

    pub fn get_composition(&self, id: Uuid) -> Option<&Composition> {
        self.compositions.iter().find(|c| c.id == id)
    }

    pub fn remove_composition(&mut self, id: Uuid) -> Option<Composition> {
        let index = self.compositions.iter().position(|c| c.id == id)?;
        Some(self.compositions.remove(index))
    }

    /// Helper to get a mutable reference to a Track inside a Composition
    pub fn get_track_mut(&mut self, composition_id: Uuid, track_id: Uuid) -> Option<&mut Track> {
        self.get_composition_mut(composition_id)?
            .get_track_mut(track_id)
    }

    /// Helper to get a mutable reference to a Clip inside a Track inside a Composition
    pub fn get_clip_mut(
        &mut self,
        composition_id: Uuid,
        track_id: Uuid,
        clip_id: Uuid,
    ) -> Option<&mut crate::model::project::TrackClip> {
        self.get_track_mut(composition_id, track_id)?
            .clips_mut()
            .find(|c| c.id == clip_id)
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Composition {
    pub id: Uuid, // Added UUID field
    pub name: String,
    pub width: u64,
    pub height: u64,
    pub fps: f64,
    pub duration: f64,
    pub background_color: Color,
    pub color_profile: String,
    #[serde(default)]
    pub work_area_in: u64,
    #[serde(default)]
    pub work_area_out: u64,

    pub tracks: Vec<Track>,
}

impl Composition {
    pub fn new(name: &str, width: u64, height: u64, fps: f64, duration: f64) -> Self {
        Self {
            id: Uuid::new_v4(), // Initialize with a new UUID
            name: name.to_string(),
            width,
            height,
            fps,
            duration,
            background_color: Color {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            },
            color_profile: "sRGB".to_string(),
            work_area_in: 0,
            work_area_out: (duration * fps).ceil() as u64,
            tracks: Vec::new(),
        }
    }

    pub fn add_track(&mut self, track: Track) {
        self.tracks.push(track);
    }

    pub fn get_track_mut(&mut self, id: Uuid) -> Option<&mut Track> {
        fn find_in_track(track: &mut Track, id: Uuid) -> Option<&mut Track> {
            if track.id == id {
                return Some(track);
            }
            for item in &mut track.children {
                if let TrackItem::SubTrack(sub) = item {
                    if let Some(found) = find_in_track(sub, id) {
                        return Some(found);
                    }
                }
            }
            None
        }

        for track in &mut self.tracks {
            if let Some(found) = find_in_track(track, id) {
                return Some(found);
            }
        }
        None
    }

    pub fn get_track(&self, id: Uuid) -> Option<&Track> {
        fn find_in_track(track: &Track, id: Uuid) -> Option<&Track> {
            if track.id == id {
                return Some(track);
            }
            for item in &track.children {
                if let TrackItem::SubTrack(sub) = item {
                    if let Some(found) = find_in_track(sub, id) {
                        return Some(found);
                    }
                }
            }
            None
        }

        for track in &self.tracks {
            if let Some(found) = find_in_track(track, id) {
                return Some(found);
            }
        }
        None
    }

    pub fn remove_track(&mut self, id: Uuid) -> Option<Track> {
        // First check top-level
        if let Some(index) = self.tracks.iter().position(|t| t.id == id) {
            return Some(self.tracks.remove(index));
        }

        // Recursively search in children
        fn remove_from_track(track: &mut Track, id: Uuid) -> Option<Track> {
            // Check if any child is the target
            if let Some(index) = track.children.iter().position(|item| item.id() == id) {
                if let TrackItem::SubTrack(_) = &track.children[index] {
                    if let TrackItem::SubTrack(removed) = track.children.remove(index) {
                        return Some(removed);
                    }
                }
            }
            // Recurse into sub-tracks
            for item in &mut track.children {
                if let TrackItem::SubTrack(sub) = item {
                    if let Some(removed) = remove_from_track(sub, id) {
                        return Some(removed);
                    }
                }
            }
            None
        }

        for track in &mut self.tracks {
            if let Some(removed) = remove_from_track(track, id) {
                return Some(removed);
            }
        }
        None
    }
}
