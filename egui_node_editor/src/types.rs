//! Lightweight data types for the node editor UI.

use uuid::Uuid;

/// Information about a pin for rendering.
#[derive(Clone, Debug)]
pub struct PinInfo {
    pub name: String,
    pub display_name: String,
    pub is_output: bool,
}

impl PinInfo {
    pub fn input(name: &str, display_name: &str) -> Self {
        Self {
            name: name.to_string(),
            display_name: display_name.to_string(),
            is_output: false,
        }
    }

    pub fn output(name: &str, display_name: &str) -> Self {
        Self {
            name: name.to_string(),
            display_name: display_name.to_string(),
            is_output: true,
        }
    }
}

/// A connection between two pins (view data).
#[derive(Clone, Debug)]
pub struct ConnectionView {
    pub id: Uuid,
    pub from_node: Uuid,
    pub from_pin: String,
    pub to_node: Uuid,
    pub to_pin: String,
}

/// How a node should be displayed.
#[derive(Clone, Debug)]
pub enum NodeDisplay {
    /// A data-flow graph node with typed pins.
    Graph {
        type_id: String,
        display_name: String,
        pins: Vec<PinInfo>,
    },
    /// A container (track/composition) that holds child nodes.
    Container {
        name: String,
        child_ids: Vec<Uuid>,
        pins: Vec<PinInfo>,
    },
    /// A leaf node (clip) with fixed pins.
    Leaf {
        kind_label: String,
        pins: Vec<PinInfo>,
    },
}

/// Node type information for the context menu.
#[derive(Clone, Debug)]
pub struct NodeTypeInfo {
    pub type_id: String,
    pub display_name: String,
    pub category: String,
}
