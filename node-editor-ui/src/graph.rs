//! Borrowed, host-owned graph descriptors for one rendered frame.

use egui::{Pos2, Rect};

use crate::CubicBezier;

/// Opaque host-defined data type used to validate a connection gesture.
///
/// The editor never interprets the wrapped value. A host can use an enum,
/// integer intern key, UUID, or any other copyable equality key.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct TypeKey<Key>(Key);

impl<Key> TypeKey<Key> {
    pub const fn new(value: Key) -> Self {
        Self(value)
    }

    pub const fn value(&self) -> &Key {
        &self.0
    }
}

/// Stable physical-port identity when one logical host port has several
/// rendered instances (for example, a variadic input row per wire).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PortInstanceId<PortId, WireId> {
    pub port: PortId,
    pub wire: Option<WireId>,
}

impl<PortId, WireId> PortInstanceId<PortId, WireId> {
    pub const fn new(port: PortId, wire: Option<WireId>) -> Self {
        Self { port, wire }
    }
}

/// The graph entity that owns a port.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PortOwner<NodeId, GroupId> {
    Node(NodeId),
    Group(GroupId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PortDirection {
    Input,
    Output,
}

/// One selectable authoritative host identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ItemId<NodeId, GroupId, WireId> {
    Node(NodeId),
    Group(GroupId),
    Wire(WireId),
}

/// One Node's frame-local geometry and hierarchy membership.
#[derive(Clone, Copy, Debug)]
pub struct NodeDescriptor<'a, NodeId, GroupId> {
    pub id: NodeId,
    pub title: &'a str,
    /// Graph-space rectangle. [`GraphFrame::transform`] maps it to screen.
    pub rect: Rect,
    /// Graph-space movement handle and header paint surface.
    ///
    /// The body is deliberately not a Node movement handle: host controls such
    /// as sliders and drag values must retain ownership of their drag gesture.
    pub header_rect: Rect,
    pub parent: Option<GroupId>,
    pub enabled: bool,
}

/// A nested grouping/container descriptor.
#[derive(Clone, Copy, Debug)]
pub struct GroupDescriptor<'a, GroupId> {
    pub id: GroupId,
    pub title: &'a str,
    /// Full graph-space bounds used for containment and resize.
    pub rect: Rect,
    /// Graph-space hit surface used for click/move/marquee selection.
    /// Usually the header, so a group's content remains usable canvas.
    pub header_rect: Rect,
    pub parent: Option<GroupId>,
    pub resizable: bool,
}

/// One concrete rendered port.
#[derive(Clone, Debug)]
pub struct PortDescriptor<'a, NodeId, PortId, GroupId, Key> {
    pub id: PortId,
    pub owner: PortOwner<NodeId, GroupId>,
    pub label: &'a str,
    pub center: Pos2,
    pub direction: PortDirection,
    pub type_key: TypeKey<Key>,
    pub connectable: bool,
}

/// One physical authored wire. Derived decoration that cannot be edited need
/// not be projected as a [`WireDescriptor`].
#[derive(Clone, Debug)]
pub struct WireDescriptor<PortId, WireId> {
    pub id: WireId,
    pub from: PortId,
    pub to: PortId,
    pub curve: CubicBezier,
    pub editable: bool,
}

/// The host's current selection. It is read on every frame and is never
/// copied into [`crate::InteractionState`] as another source of truth.
#[derive(Clone, Copy, Debug)]
pub struct AuthoritativeSelection<'a, NodeId, GroupId, WireId> {
    pub items: &'a [ItemId<NodeId, GroupId, WireId>],
    pub primary: Option<ItemId<NodeId, GroupId, WireId>>,
}

impl<NodeId, GroupId, WireId> Default for AuthoritativeSelection<'_, NodeId, GroupId, WireId> {
    fn default() -> Self {
        Self {
            items: &[],
            primary: None,
        }
    }
}

/// Complete, borrowed UI projection of an authoritative host graph.
///
/// Descriptors use graph coordinates. `transform` is the sole graph-to-screen
/// mapping and `viewport` is in screen points. The frame owns no graph data;
/// all slices are borrowed for this call only.
#[derive(Clone, Copy, Debug)]
pub struct GraphFrame<'a, NodeId, PortId, WireId, GroupId, Key> {
    pub viewport: Rect,
    pub transform: egui::emath::TSTransform,
    pub nodes: &'a [NodeDescriptor<'a, NodeId, GroupId>],
    pub ports: &'a [PortDescriptor<'a, NodeId, PortId, GroupId, Key>],
    pub wires: &'a [WireDescriptor<PortId, WireId>],
    pub groups: &'a [GroupDescriptor<'a, GroupId>],
    /// Back-to-front order for Node and Group selection surfaces.
    ///
    /// This is a single cross-kind order so overlapping Nodes and Group
    /// headers use the host's real paint policy for click and marquee primary
    /// selection. Wires keep their independent authored paint order.
    pub selection_order: &'a [ItemId<NodeId, GroupId, WireId>],
    pub selection: AuthoritativeSelection<'a, NodeId, GroupId, WireId>,
}

impl<NodeId, PortId, WireId, GroupId, Key> GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key> {
    pub fn graph_position(&self, screen_position: Pos2) -> Pos2 {
        self.transform.inverse() * screen_position
    }

    pub fn screen_rect(&self, graph_rect: Rect) -> Rect {
        self.transform * graph_rect
    }

    pub fn screen_position(&self, graph_position: Pos2) -> Pos2 {
        self.transform * graph_position
    }
}
