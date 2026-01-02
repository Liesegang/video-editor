use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::model::property::PropertyValue;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct NodeGraph {
    pub id: Uuid,
    pub nodes: HashMap<Uuid, GraphNode>,
    pub edges: Vec<GraphEdge>,
}

impl NodeGraph {
    pub fn new() -> Self {
        Self {
            id: Uuid::new_v4(),
            nodes: HashMap::new(),
            edges: Vec::new(),
        }
    }

    pub fn add_node(&mut self, node: GraphNode) {
        self.nodes.insert(node.id, node);
    }

    pub fn add_edge(&mut self, edge: GraphEdge) {
        self.edges.push(edge);
    }
}

impl std::hash::Hash for NodeGraph {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct GraphNode {
    pub id: Uuid,
    pub kind: NodeKind,
    pub label: String,
    pub position: (f32, f32),
    pub inputs: Vec<Pin>,
    pub outputs: Vec<Pin>,
    pub properties: HashMap<String, PropertyValue>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum NodeKind {
    Input,
    Output,
    Filter,
    Composite,
    Generator,
    // Expanded Text/Ensemble Nodes
    Constant { value: PropertyValue }, // Generic constant provider
    TextLayout,                        // String + Font -> EnsembleData
    Effector { kind: String },         // EnsembleData -> EnsembleData
    Decorator { kind: String },        // EnsembleData -> Path
    ShapeFill,                         // Path + Color -> Image
    ShapeStroke,                       // Path + Color + Width -> Image
    // Represents an existing Project/Timeline entity
    ClipReference { clip_id: Uuid },
    TrackReference { track_id: Uuid },
}

impl GraphNode {
    pub fn new(kind: NodeKind, label: &str, position: (f32, f32)) -> Self {
        Self {
            id: Uuid::new_v4(),
            kind,
            label: label.to_string(),
            position,
            inputs: Vec::new(),
            outputs: Vec::new(),
            properties: HashMap::new(),
        }
    }

    pub fn with_input(mut self, name: &str, data_type: DataType) -> Self {
        self.inputs.push(Pin {
            id: Uuid::new_v4(),
            name: name.to_string(),
            data_type,
        });
        self
    }

    pub fn with_output(mut self, name: &str, data_type: DataType) -> Self {
        self.outputs.push(Pin {
            id: Uuid::new_v4(),
            name: name.to_string(),
            data_type,
        });
        self
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct GraphEdge {
    pub id: Uuid,
    pub output_node: Uuid,
    pub output_pin_index: usize,
    pub input_node: Uuid,
    pub input_pin_index: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Pin {
    pub id: Uuid,
    pub name: String,
    pub data_type: DataType,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum DataType {
    Any,
    Image,
    Audio,
    Scalar,
    Vector,
    Color,
    String,
    // New Types
    EnsembleData, // Carries text layout and transform info
    Path,         // Bezier path / Shape geometry
}
