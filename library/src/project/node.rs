use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::composition::Composition;
use super::graph_node::GraphNode;
use super::layer::LayerData;
use super::source::SourceData;
use super::track::TrackData;

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "node_type")]
pub enum Node {
    Track(TrackData),
    Layer(LayerData),
    Composition(Composition),
    #[serde(alias = "Clip")]
    Source(SourceData),
    Graph(GraphNode),
}

impl Node {
    /// Get the ID of this node
    pub fn id(&self) -> Uuid {
        match self {
            Node::Track(t) => t.id,
            Node::Layer(l) => l.id,
            Node::Composition(c) => c.id,
            Node::Source(s) => s.id,
            Node::Graph(g) => g.id,
        }
    }
}
