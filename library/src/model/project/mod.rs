use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use uuid::Uuid;

pub mod asset;
pub mod clip_helpers;
pub mod effect;
pub mod ensemble;
pub mod property;
pub mod style;

use super::{Layer, Node, Track};
use crate::model::frame::color::Color;
use crate::model::project::asset::Asset;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Project {
    pub name: String,

    // --- 3. Composite (The Scope / Graph) ---
    /// Independent timelines/compositions.
    pub compositions: Vec<Composite>,

    // --- Global Resources (New!) ---
    /// Timeless node graphs (Particles, Physics Worlds, Shaders).
    /// Referenced by Layers through ReferenceContent.
    #[serde(default)]
    pub resources: HashMap<Uuid, ResourceGraph>,

    #[serde(default)]
    pub assets: Vec<Asset>,

    #[serde(default)]
    pub export: ExportConfig,

    /// Unified Node Registry (Tracks and Layers are stored flat here).
    /// Design Choice: Global registry allows easier UUID resolution.
    #[serde(default)]
    pub nodes: HashMap<Uuid, Node>,
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

// Renamed from Composition
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Composite {
    pub id: Uuid,
    pub name: String,
    pub width: u64,
    pub height: u64,
    pub fps: f64,
    pub duration: f64, // The defined duration of this composite
    pub background_color: Color,
    #[serde(default = "default_color_profile")]
    pub color_profile: String,

    #[serde(default)]
    pub work_area_in: u64,
    #[serde(default)]
    pub work_area_out: u64,

    // Reference to the root mixer track (Start of the mix tree)
    pub root_track_id: Uuid,

    // "node_graph" (Free-floating nodes) can coexist here.
    // Composite = Track Tree + Free Node Graph
    #[serde(default)]
    pub node_graph: crate::model::node_graph::NodeGraph,
}

fn default_color_profile() -> String {
    "sRGB".to_string()
}

// Entity for Global Resources
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct ResourceGraph {
    pub id: Uuid,
    pub name: String,
    pub kind: ResourceKind, // Particle, Shader, 3DScene, etc.
    pub node_graph: crate::model::node_graph::NodeGraph,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub enum ResourceKind {
    ParticleSystem,
    PhysicsWorld,
    ProceduralTexture,
}

impl Project {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            compositions: Vec::new(),
            resources: HashMap::new(),
            assets: Vec::new(),
            export: ExportConfig::default(),
            nodes: HashMap::new(),
        }
    }

    pub fn load(json_str: &str) -> Result<Self, serde_json::Error> {
        let project: Project = serde_json::from_str(json_str)?;
        Ok(project)
    }

    pub fn save(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    pub fn add_composition(&mut self, composition: Composite) {
        self.compositions.push(composition);
    }

    pub fn get_composition_mut(&mut self, id: Uuid) -> Option<&mut Composite> {
        self.compositions.iter_mut().find(|c| c.id == id)
    }

    pub fn get_composition(&self, id: Uuid) -> Option<&Composite> {
        self.compositions.iter().find(|c| c.id == id)
    }

    pub fn remove_composition(&mut self, id: Uuid) -> Option<Composite> {
        let index = self.compositions.iter().position(|c| c.id == id)?;
        Some(self.compositions.remove(index))
    }

    pub fn get_asset(&self, id: Uuid) -> Option<&Asset> {
        self.assets.iter().find(|a| a.id == id)
    }

    // ==================== Node Registry Methods ====================

    /// Add a node to the registry
    pub fn add_node(&mut self, node: Node) {
        self.nodes.insert(node.id(), node);
    }

    /// Get a node by ID
    pub fn get_node(&self, id: Uuid) -> Option<&Node> {
        self.nodes.get(&id)
    }

    /// Get a mutable node by ID
    pub fn get_node_mut(&mut self, id: Uuid) -> Option<&mut Node> {
        self.nodes.get_mut(&id)
    }

    /// Remove a node from the registry
    pub fn remove_node(&mut self, id: Uuid) -> Option<Node> {
        self.nodes.remove(&id)
    }

    // ==================== Trinity Helpers ====================

    pub fn get_layer(&self, id: Uuid) -> Option<&Layer> {
        match self.nodes.get(&id)? {
            Node::Layer(l) => Some(l),
            _ => None,
        }
    }

    pub fn get_layer_mut(&mut self, id: Uuid) -> Option<&mut Layer> {
        match self.nodes.get_mut(&id)? {
            Node::Layer(l) => Some(l),
            _ => None,
        }
    }

    pub fn get_track(&self, id: Uuid) -> Option<&Track> {
        match self.nodes.get(&id)? {
            Node::Track(t) => Some(t),
            _ => None,
        }
    }

    pub fn get_track_mut(&mut self, id: Uuid) -> Option<&mut Track> {
        match self.nodes.get_mut(&id)? {
            Node::Track(t) => Some(t),
            _ => None,
        }
    }

    /// Find the parent track containing a given node ID
    pub fn find_parent_track(&self, node_id: Uuid) -> Option<Uuid> {
        for (id, node) in &self.nodes {
            if let Node::Track(track) = node {
                if track.children.contains(&node_id) {
                    return Some(*id);
                }
            }
        }
        None
    }
}

impl Composite {
    pub fn new(name: &str, width: u64, height: u64, fps: f64, duration: f64) -> (Self, Track) {
        let root_track = Track::new(&format!("{} - Root", name));

        let comp = Self {
            id: Uuid::new_v4(),
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
            root_track_id: root_track.id,
            node_graph: crate::model::node_graph::NodeGraph::new(),
        };
        (comp, root_track)
    }
}
