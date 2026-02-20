use std::collections::HashMap;
use uuid::Uuid;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::clip::TrackClip;
use super::connection::{Connection, PinId};
use super::graph_node::GraphNode;
use super::node::Node;
use super::track::TrackData;
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
    /// 統一ノードレジストリ - 全Track/Clip/GraphNodeを格納
    #[serde(default)]
    pub nodes: HashMap<Uuid, Node>,
    /// Data-flow graph connections between node pins
    #[serde(default)]
    pub connections: Vec<Connection>,
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
            nodes: HashMap::new(),
            connections: Vec::new(),
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

    // ==================== Convenience Accessors ====================

    /// Get a clip by ID (convenience method)
    pub fn get_clip(&self, id: Uuid) -> Option<&TrackClip> {
        match self.nodes.get(&id)? {
            Node::Clip(c) => Some(c),
            _ => None,
        }
    }

    /// Get a mutable clip by ID (convenience method)
    pub fn get_clip_mut(&mut self, id: Uuid) -> Option<&mut TrackClip> {
        match self.nodes.get_mut(&id)? {
            Node::Clip(c) => Some(c),
            _ => None,
        }
    }

    /// Get a track by ID (convenience method)
    pub fn get_track(&self, id: Uuid) -> Option<&TrackData> {
        match self.nodes.get(&id)? {
            Node::Track(t) => Some(t),
            _ => None,
        }
    }

    /// Get a mutable track by ID (convenience method)
    pub fn get_track_mut(&mut self, id: Uuid) -> Option<&mut TrackData> {
        match self.nodes.get_mut(&id)? {
            Node::Track(t) => Some(t),
            _ => None,
        }
    }

    // ==================== Traversal Helpers ====================

    /// Collect all clips under a given node (recursively)
    pub fn collect_clips(&self, node_id: Uuid) -> Vec<&TrackClip> {
        let mut clips = Vec::new();
        self.collect_clips_recursive(node_id, &mut clips);
        clips
    }

    fn collect_clips_recursive<'a>(&'a self, node_id: Uuid, clips: &mut Vec<&'a TrackClip>) {
        match self.nodes.get(&node_id) {
            Some(Node::Clip(c)) => clips.push(c),
            Some(Node::Track(t)) => {
                for child_id in &t.child_ids {
                    self.collect_clips_recursive(*child_id, clips);
                }
            }
            _ => {}
        }
    }

    /// Iterate over all clips in the registry
    pub fn all_clips(&self) -> impl Iterator<Item = &TrackClip> {
        self.nodes.values().filter_map(|node| match node {
            Node::Clip(c) => Some(c),
            _ => None,
        })
    }

    /// Iterate over all tracks in the registry
    pub fn all_tracks(&self) -> impl Iterator<Item = &TrackData> {
        self.nodes.values().filter_map(|node| match node {
            Node::Track(t) => Some(t),
            _ => None,
        })
    }

    /// Find the parent track containing a given clip ID
    pub fn find_parent_track(&self, clip_id: Uuid) -> Option<Uuid> {
        for (id, node) in &self.nodes {
            if let Node::Track(track) = node {
                if track.child_ids.contains(&clip_id) {
                    return Some(*id);
                }
            }
        }
        None
    }

    // ==================== GraphNode Accessors ====================

    /// Get a graph node by ID
    pub fn get_graph_node(&self, id: Uuid) -> Option<&GraphNode> {
        match self.nodes.get(&id)? {
            Node::Graph(g) => Some(g),
            _ => None,
        }
    }

    /// Get a mutable graph node by ID
    pub fn get_graph_node_mut(&mut self, id: Uuid) -> Option<&mut GraphNode> {
        match self.nodes.get_mut(&id)? {
            Node::Graph(g) => Some(g),
            _ => None,
        }
    }

    /// Iterate over all graph nodes
    pub fn all_graph_nodes(&self) -> impl Iterator<Item = &GraphNode> {
        self.nodes.values().filter_map(|node| match node {
            Node::Graph(g) => Some(g),
            _ => None,
        })
    }

    // ==================== Connection Methods ====================

    /// Add a connection
    pub fn add_connection(&mut self, connection: Connection) {
        self.connections.push(connection);
    }

    /// Remove a connection by ID
    pub fn remove_connection(&mut self, connection_id: Uuid) -> Option<Connection> {
        let pos = self
            .connections
            .iter()
            .position(|c| c.id == connection_id)?;
        Some(self.connections.remove(pos))
    }

    /// Get all connections involving a specific node
    pub fn get_connections_for_node(&self, node_id: Uuid) -> Vec<&Connection> {
        self.connections
            .iter()
            .filter(|c| c.from.node_id == node_id || c.to.node_id == node_id)
            .collect()
    }

    /// Get the connection feeding into a specific input pin
    pub fn get_input_connection(&self, pin: &PinId) -> Option<&Connection> {
        self.connections
            .iter()
            .find(|c| c.to.node_id == pin.node_id && c.to.pin_name == pin.pin_name)
    }

    /// Remove all connections involving a specific node
    pub fn remove_connections_for_node(&mut self, node_id: Uuid) {
        self.connections
            .retain(|c| c.from.node_id != node_id && c.to.node_id != node_id);
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Composition {
    pub id: Uuid,
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

    /// 単一のルートトラックUUID
    pub root_track_id: Uuid,
}

impl Composition {
    /// Create a new composition with an auto-generated root track
    pub fn new(name: &str, width: u64, height: u64, fps: f64, duration: f64) -> (Self, TrackData) {
        let root_track = TrackData::new(&format!("{} - Root", name));
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
        };
        (comp, root_track)
    }

    /// Create a composition with an existing root track ID
    pub fn new_with_root(
        name: &str,
        width: u64,
        height: u64,
        fps: f64,
        duration: f64,
        root_track_id: Uuid,
    ) -> Self {
        Self {
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
            root_track_id,
        }
    }
}
