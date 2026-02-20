use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::clip::TrackClip;
use super::graph_node::GraphNode;
use super::track::TrackData;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "node_type")]
pub enum Node {
    Track(TrackData),
    Clip(TrackClip),
    Graph(GraphNode),
}

impl Node {
    /// Get the ID of this node
    pub fn id(&self) -> Uuid {
        match self {
            Node::Track(t) => t.id,
            Node::Clip(c) => c.id,
            Node::Graph(g) => g.id,
        }
    }
}
