use std::collections::HashMap;
use uuid::Uuid;

use serde::{Deserialize, Serialize};

pub use super::composition::Composition;
use super::connection::{Connection, PinId};
pub use super::export_config::ExportConfig;
use super::graph_node::GraphNode;
use super::layer::LayerData;
use super::node::Node;
use super::source::SourceData;
use super::track::TrackData;

use crate::project::asset::Asset;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Project {
    pub name: String,
    #[serde(default)]
    pub composition_ids: Vec<Uuid>,
    /// Deprecated: kept for backward-compatible deserialization.
    /// On load, compositions are migrated into the node registry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub compositions: Vec<Composition>,
    #[serde(default)]
    pub assets: Vec<Asset>,
    #[serde(default)]
    pub export: ExportConfig,
    #[serde(default)]
    pub nodes: HashMap<Uuid, Node>,
    #[serde(default)]
    pub connections: Vec<Connection>,
}

impl Project {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            composition_ids: Vec::new(),
            compositions: Vec::new(),
            assets: Vec::new(),
            export: ExportConfig::default(),
            nodes: HashMap::new(),
            connections: Vec::new(),
        }
    }

    pub fn load(json_str: &str) -> Result<Self, serde_json::Error> {
        let mut project: Project = serde_json::from_str(json_str)?;
        project.migrate();
        Ok(project)
    }

    pub fn save(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Migrate legacy data into the unified node registry.
    fn migrate(&mut self) {
        // Migrate compositions from the legacy Vec into Node::Composition
        for comp in std::mem::take(&mut self.compositions) {
            let id = comp.id;
            self.composition_ids.push(id);
            self.nodes.insert(id, Node::Composition(comp));
        }

        // Migrate root_track_id → child_ids for compositions
        let comp_ids: Vec<Uuid> = self.composition_ids.clone();
        for comp_id in comp_ids {
            let root_id = match self.nodes.get(&comp_id) {
                Some(Node::Composition(c)) => c.root_track_id,
                _ => continue,
            };
            let Some(root_track_id) = root_id else {
                continue;
            };
            // Already migrated if child_ids is non-empty
            if let Some(Node::Composition(c)) = self.nodes.get(&comp_id) {
                if !c.child_ids.is_empty() {
                    continue;
                }
            }
            // Promote root track's children into composition's child_ids,
            // then remove the root track node.
            let promoted_children = self
                .nodes
                .get(&root_track_id)
                .and_then(|n| match n {
                    Node::Track(t) => Some(t.child_ids.clone()),
                    _ => None,
                })
                .unwrap_or_default();

            if let Some(Node::Composition(c)) = self.nodes.get_mut(&comp_id) {
                c.child_ids = promoted_children;
                c.root_track_id = None;
            }
            // Remove the old root track node
            self.nodes.remove(&root_track_id);
        }

        // Migrate is_layer tracks into Node::Layer
        let layer_ids: Vec<Uuid> = self
            .nodes
            .iter()
            .filter_map(|(id, n)| match n {
                Node::Track(t) if t.is_layer => Some(*id),
                _ => None,
            })
            .collect();
        for id in layer_ids {
            if let Some(Node::Track(t)) = self.nodes.remove(&id) {
                // Extract in_frame/out_frame from child Source
                let (in_frame, out_frame) = t
                    .child_ids
                    .iter()
                    .find_map(|child_id| match self.nodes.get(child_id) {
                        Some(Node::Source(s)) => Some((s.in_frame, s.out_frame)),
                        _ => None,
                    })
                    .unwrap_or((0, 0));

                self.nodes.insert(
                    id,
                    Node::Layer(LayerData {
                        id: t.id,
                        name: t.name,
                        child_ids: t.child_ids,
                        blend_mode: t.blend_mode,
                        opacity: t.opacity,
                        visible: t.visible,
                        in_frame,
                        out_frame,
                    }),
                );
            }
        }

        // For existing Layer nodes with in_frame=0, out_frame=0,
        // propagate timing from child Source if available
        let layers_to_fix: Vec<Uuid> = self
            .nodes
            .iter()
            .filter_map(|(id, n)| match n {
                Node::Layer(l) if l.in_frame == 0 && l.out_frame == 0 => Some(*id),
                _ => None,
            })
            .collect();
        for layer_id in layers_to_fix {
            let timing = {
                let layer = match self.nodes.get(&layer_id) {
                    Some(Node::Layer(l)) => l,
                    _ => continue,
                };
                layer.child_ids.iter().find_map(|cid| {
                    if let Some(Node::Source(s)) = self.nodes.get(cid) {
                        if s.in_frame > 0 || s.out_frame > 0 {
                            Some((s.in_frame, s.out_frame))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
            };
            if let Some((inf, outf)) = timing {
                if let Some(Node::Layer(l)) = self.nodes.get_mut(&layer_id) {
                    l.in_frame = inf;
                    l.out_frame = outf;
                }
            }
        }
    }

    // ==================== Composition Methods ====================

    pub fn add_composition(&mut self, composition: Composition) {
        let id = composition.id;
        self.composition_ids.push(id);
        self.nodes.insert(id, Node::Composition(composition));
    }

    pub fn get_composition(&self, id: Uuid) -> Option<&Composition> {
        match self.nodes.get(&id)? {
            Node::Composition(c) => Some(c),
            _ => None,
        }
    }

    pub fn get_composition_mut(&mut self, id: Uuid) -> Option<&mut Composition> {
        match self.nodes.get_mut(&id)? {
            Node::Composition(c) => Some(c),
            _ => None,
        }
    }

    pub fn remove_composition(&mut self, id: Uuid) -> Option<Composition> {
        self.composition_ids.retain(|cid| *cid != id);
        match self.nodes.remove(&id)? {
            Node::Composition(c) => Some(c),
            other => {
                // Put it back if it wasn't a composition
                self.nodes.insert(id, other);
                None
            }
        }
    }

    pub fn all_compositions(&self) -> impl Iterator<Item = &Composition> {
        self.composition_ids
            .iter()
            .filter_map(|id| self.get_composition(*id))
    }

    // ==================== Node Registry Methods ====================

    pub fn add_node(&mut self, node: Node) {
        self.nodes.insert(node.id(), node);
    }

    pub fn get_node(&self, id: Uuid) -> Option<&Node> {
        self.nodes.get(&id)
    }

    pub fn get_node_mut(&mut self, id: Uuid) -> Option<&mut Node> {
        self.nodes.get_mut(&id)
    }

    pub fn remove_node(&mut self, id: Uuid) -> Option<Node> {
        self.nodes.remove(&id)
    }

    // ==================== Convenience Accessors ====================

    pub fn get_source(&self, id: Uuid) -> Option<&SourceData> {
        match self.nodes.get(&id)? {
            Node::Source(s) => Some(s),
            _ => None,
        }
    }

    pub fn get_source_mut(&mut self, id: Uuid) -> Option<&mut SourceData> {
        match self.nodes.get_mut(&id)? {
            Node::Source(s) => Some(s),
            _ => None,
        }
    }

    pub fn get_track(&self, id: Uuid) -> Option<&TrackData> {
        match self.nodes.get(&id)? {
            Node::Track(t) => Some(t),
            _ => None,
        }
    }

    pub fn get_track_mut(&mut self, id: Uuid) -> Option<&mut TrackData> {
        match self.nodes.get_mut(&id)? {
            Node::Track(t) => Some(t),
            _ => None,
        }
    }

    pub fn get_layer(&self, id: Uuid) -> Option<&LayerData> {
        match self.nodes.get(&id)? {
            Node::Layer(l) => Some(l),
            _ => None,
        }
    }

    pub fn get_layer_mut(&mut self, id: Uuid) -> Option<&mut LayerData> {
        match self.nodes.get_mut(&id)? {
            Node::Layer(l) => Some(l),
            _ => None,
        }
    }

    // ==================== Traversal Helpers ====================

    /// Collect all sources under a given node (recursively through tracks and layers)
    pub fn collect_sources(&self, node_id: Uuid) -> Vec<&SourceData> {
        let mut sources = Vec::new();
        self.collect_sources_recursive(node_id, &mut sources);
        sources
    }

    fn collect_sources_recursive<'a>(&'a self, node_id: Uuid, sources: &mut Vec<&'a SourceData>) {
        match self.nodes.get(&node_id) {
            Some(Node::Source(s)) => sources.push(s),
            Some(Node::Composition(_)) | Some(Node::Track(_)) | Some(Node::Layer(_)) => {
                if let Some(children) = self.get_container_child_ids(node_id) {
                    for child_id in children.clone() {
                        self.collect_sources_recursive(child_id, sources);
                    }
                }
            }
            _ => {}
        }
    }

    /// Iterate over all sources in the registry
    pub fn all_sources(&self) -> impl Iterator<Item = &SourceData> {
        self.nodes.values().filter_map(|node| match node {
            Node::Source(s) => Some(s),
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

    /// Find the parent container (composition, track, or layer) of a given child node
    pub fn find_parent_container(&self, child_id: Uuid) -> Option<Uuid> {
        for (id, node) in &self.nodes {
            match node {
                Node::Composition(c) if c.child_ids.contains(&child_id) => return Some(*id),
                Node::Track(track) if track.child_ids.contains(&child_id) => return Some(*id),
                Node::Layer(layer) if layer.child_ids.contains(&child_id) => return Some(*id),
                _ => {}
            }
        }
        None
    }

    /// Alias for backward compatibility
    pub fn find_parent_track(&self, child_id: Uuid) -> Option<Uuid> {
        self.find_parent_container(child_id)
    }

    /// Get child_ids from any container node (Composition, Track, or Layer).
    pub fn get_container_child_ids(&self, id: Uuid) -> Option<&Vec<Uuid>> {
        match self.nodes.get(&id)? {
            Node::Composition(c) => Some(&c.child_ids),
            Node::Track(t) => Some(&t.child_ids),
            Node::Layer(l) => Some(&l.child_ids),
            _ => None,
        }
    }

    /// Get mutable child_ids from any container node (Composition, Track, or Layer).
    pub fn get_container_child_ids_mut(&mut self, id: Uuid) -> Option<&mut Vec<Uuid>> {
        match self.nodes.get_mut(&id)? {
            Node::Composition(c) => Some(&mut c.child_ids),
            Node::Track(t) => Some(&mut t.child_ids),
            Node::Layer(l) => Some(&mut l.child_ids),
            _ => None,
        }
    }

    /// Check whether `node_id` is reachable from `root_id` in the node tree.
    pub fn is_node_in_tree(&self, root_id: Uuid, node_id: Uuid) -> bool {
        if root_id == node_id {
            return true;
        }
        if let Some(children) = self.get_container_child_ids(root_id) {
            for child_id in children.clone() {
                if self.is_node_in_tree(child_id, node_id) {
                    return true;
                }
            }
        }
        false
    }

    // ==================== GraphNode Accessors ====================

    pub fn get_graph_node(&self, id: Uuid) -> Option<&GraphNode> {
        match self.nodes.get(&id)? {
            Node::Graph(g) => Some(g),
            _ => None,
        }
    }

    pub fn get_graph_node_mut(&mut self, id: Uuid) -> Option<&mut GraphNode> {
        match self.nodes.get_mut(&id)? {
            Node::Graph(g) => Some(g),
            _ => None,
        }
    }

    pub fn all_graph_nodes(&self) -> impl Iterator<Item = &GraphNode> {
        self.nodes.values().filter_map(|node| match node {
            Node::Graph(g) => Some(g),
            _ => None,
        })
    }

    // ==================== Connection Methods ====================

    pub fn add_connection(&mut self, connection: Connection) {
        self.connections.push(connection);
    }

    pub fn remove_connection(&mut self, connection_id: Uuid) -> Option<Connection> {
        let pos = self
            .connections
            .iter()
            .position(|c| c.id == connection_id)?;
        Some(self.connections.remove(pos))
    }

    pub fn get_connections_for_node(&self, node_id: Uuid) -> Vec<&Connection> {
        self.connections
            .iter()
            .filter(|c| c.from.node_id == node_id || c.to.node_id == node_id)
            .collect()
    }

    pub fn get_input_connection(&self, pin: &PinId) -> Option<&Connection> {
        self.connections
            .iter()
            .find(|c| c.to.node_id == pin.node_id && c.to.pin_name == pin.pin_name)
    }

    pub fn remove_connections_for_node(&mut self, node_id: Uuid) {
        self.connections
            .retain(|c| c.from.node_id != node_id && c.to.node_id != node_id);
    }
}
