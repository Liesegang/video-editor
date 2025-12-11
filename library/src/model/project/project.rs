use std::collections::HashMap;
use uuid::Uuid;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Track;
use crate::model::frame::color::Color;
use crate::model::project::TrackEntity; // Add this

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct Project {
    pub name: String,
    pub compositions: Vec<Composition>,
    #[serde(default)]
    pub export: ExportConfig,
}

#[derive(Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct ExportConfig {
    #[serde(default)]
    pub container: Option<String>,
    #[serde(default)]
    pub codec: Option<String>,
    #[serde(default)]
    pub pixel_format: Option<String>,
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
            export: ExportConfig::default(),
        }
    }

    pub fn load(json_str: &str) -> Result<Self, serde_json::Error> {
        let mut project: Project = serde_json::from_str(json_str)?;
        for composition in project.compositions.iter_mut() {
            composition.rebuild_entity_cache();
        }
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

    pub fn remove_composition(&mut self, id: Uuid) -> Option<Composition> {
        let index = self.compositions.iter().position(|c| c.id == id)?;
        Some(self.compositions.remove(index))
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq)]
pub struct Composition {
    pub id: Uuid, // Added UUID field
    pub name: String,
    pub width: u64,
    pub height: u64,
    pub fps: f64,
    pub duration: f64,
    pub background_color: Color,
    pub color_profile: String,

    pub tracks: Vec<Track>,

    #[serde(skip)]
    cached_entities: Vec<TrackEntity>,
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
            tracks: Vec::new(),
            cached_entities: Vec::new(),
        }
    }

    pub fn add_track(&mut self, track: Track) {
        self.tracks.push(track);
        self.rebuild_entity_cache();
    }

    pub fn get_track_mut(&mut self, id: Uuid) -> Option<&mut Track> {
        self.tracks.iter_mut().find(|t| t.id == id)
    }

    pub fn remove_track(&mut self, id: Uuid) -> Option<Track> {
        let index = self.tracks.iter().position(|t| t.id == id)?;
        let removed_track = self.tracks.remove(index);
        self.rebuild_entity_cache(); // Rebuild cache after removing a track
        Some(removed_track)
    }

    pub(crate) fn cached_entities(&self) -> &[TrackEntity] {
        // Change return type
        &self.cached_entities
    }

    pub fn rebuild_entity_cache(&mut self) {
        self.cached_entities = self
            .tracks
            .iter()
            .flat_map(|track| track.entities.iter())
            .cloned() // Clone TrackEntity directly
            .collect();
    }
}
