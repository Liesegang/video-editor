use crate::state::context_types::{NodeEditorEditableWire, NodeEditorPendingEdit};
use eframe::egui;
use library::model::project::{PortAddress, PortOwner};
use library::model::property::{Property, PropertyValue};
use library::model::{BlendMode, Node};
use uuid::Uuid;

#[derive(Debug)]
pub(in crate::ui::panels::node_editor) enum NodeEdit {
    Connect {
        from: PortAddress,
        to: PortAddress,
    },
    ConnectAtIndex {
        from: PortAddress,
        to: PortAddress,
        canonical_index: usize,
    },
    Disconnect {
        from: PortAddress,
        to: PortAddress,
    },
    DisconnectConnection {
        connection_id: Uuid,
    },
    DisconnectWires {
        wires: Vec<NodeEditorEditableWire>,
    },
    ReconnectConnection {
        connection_id: Uuid,
        from: PortAddress,
        to: PortAddress,
    },
    SetConnectionBlendMode {
        connection_id: Uuid,
        blend_mode: BlendMode,
    },
    ReorderConnection {
        connection_id: Uuid,
        new_order: i64,
    },
    SpliceExistingNode {
        connection_id: Uuid,
        node_id: Uuid,
    },
    InsertNodeOnConnection {
        connection_id: Uuid,
        node: Box<Node>,
        position: egui::Pos2,
        composition_id: Uuid,
    },
    SetOutputNode {
        owner: PortOwner,
        node_id: Option<Uuid>,
    },
    SetAudioOutputNode {
        owner: PortOwner,
        node_id: Option<Uuid>,
    },
    Delete {
        owner: PortOwner,
    },
    SetEnabled {
        node_id: Uuid,
        enabled: bool,
    },
    SetBypassed {
        node_id: Uuid,
        bypassed: bool,
    },
    RenameContainer {
        owner: PortOwner,
        name: String,
    },
    ResizeContainer {
        owner: PortOwner,
        size: [f32; 2],
    },
    ToggleContainer {
        owner: PortOwner,
    },
    Rename {
        node_id: Uuid,
        name: String,
    },
    ReplaceProperty {
        node_id: Uuid,
        key: String,
        property: Property,
    },
    SetProperty {
        owner: PortOwner,
        key: String,
        time: f64,
        value: PropertyValue,
    },
}

#[derive(Debug)]
pub(in crate::ui::panels::node_editor) enum QueuedNodeEdit {
    Atomic(NodeEdit),
    Continuous {
        pending: NodeEditorPendingEdit,
        edit: Option<NodeEdit>,
        finished: bool,
    },
}
