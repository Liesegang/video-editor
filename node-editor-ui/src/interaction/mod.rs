//! Domain-neutral pointer orchestration and host intents.

use egui::{Pos2, Rect, Vec2};

use crate::layout_swipe::LayoutSwipeState;
use crate::{ItemId, LayoutSwipeHitArea, LayoutSwipeIntent};

mod hit;
mod keyboard;
mod layout_swipe_preflight;
mod lifecycle;
mod selection;
mod transient;

pub(crate) use hit::wire_selection_target;
pub(crate) use layout_swipe_preflight::layout_swipe_wants_pointer;
pub(crate) use lifecycle::interact;

/// A mutation request against the host's authoritative graph.
///
/// Outputs contain only host IDs and values needed to perform one edit. The
/// crate never applies them to a shadow graph.
#[derive(Clone, Debug, PartialEq)]
pub enum EditorOutput<NodeId, PortId, WireId, GroupId> {
    Select {
        items: Vec<ItemId<NodeId, GroupId, WireId>>,
        primary: Option<ItemId<NodeId, GroupId, WireId>>,
    },
    Move {
        items: Vec<ItemId<NodeId, GroupId, WireId>>,
        /// Item whose header physically captured the pointer. This is not
        /// necessarily the host's authoritative selection primary.
        grabbed: ItemId<NodeId, GroupId, WireId>,
        delta: Vec2,
    },
    /// Ends a Move gesture that emitted at least one [`Self::Move`] intent.
    ///
    /// The host must close the movement transaction for either outcome.
    /// [`MoveEndOutcome::Cancelled`] keeps the positions already applied but
    /// forbids release-only behavior such as reparenting or wire splicing.
    /// Only [`MoveEndOutcome::Released`] represents a real primary release.
    MoveEnd {
        outcome: MoveEndOutcome,
    },
    /// A non-mutating directional-layout gesture for the host to interpret.
    LayoutSwipe(LayoutSwipeIntent<NodeId>),
    Connect {
        from: PortId,
        to: PortId,
    },
    /// Atomically moves one endpoint of an existing authored wire.
    ///
    /// Unlike a `Disconnect` followed by `Connect`, this intent keeps the
    /// host's wire identity and edge metadata intact and never exposes an
    /// invalid intermediate graph to validation or rendering.
    Reconnect {
        wire: WireId,
        from: PortId,
        to: PortId,
    },
    Disconnect {
        wire: WireId,
    },
    Delete {
        items: Vec<ItemId<NodeId, GroupId, WireId>>,
    },
    Reparent {
        nodes: Vec<NodeId>,
        parent: Option<GroupId>,
    },
    ResizeGroup {
        group: GroupId,
        rect: Rect,
    },
    DeselectWire {
        wire: WireId,
    },
}

/// Why a Move gesture that changed position ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveEndOutcome {
    /// The primary button produced a real release event.
    Released,
    /// Escape, pointer/capture loss, or disabled movement ended the gesture.
    Cancelled,
}

/// Enables coherent subsets while a host incrementally replaces an existing
/// renderer. Disabled gestures emit no intent and retain no state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InteractionOptions {
    pub select: bool,
    /// Precise curve selection is independently gated at overview scale.
    pub select_wires: bool,
    /// Rectangle selection is independently gated at overview scale.
    pub marquee: bool,
    pub move_items: bool,
    pub connect: bool,
    pub disconnect: bool,
    pub delete: bool,
    pub reparent: bool,
    pub resize_groups: bool,
    /// Hold-A directional layout gesture target policy.
    pub layout_swipe: LayoutSwipeHitArea,
}

impl InteractionOptions {
    pub const ALL: Self = Self {
        select: true,
        select_wires: true,
        marquee: true,
        move_items: true,
        connect: true,
        disconnect: true,
        delete: true,
        reparent: true,
        resize_groups: true,
        layout_swipe: LayoutSwipeHitArea::Header,
    };

    /// Selection-only migration slice for hosts retaining another movement
    /// implementation.
    pub const SELECTION: Self = Self {
        select: true,
        select_wires: true,
        marquee: true,
        move_items: false,
        connect: false,
        disconnect: false,
        delete: false,
        reparent: false,
        resize_groups: false,
        layout_swipe: LayoutSwipeHitArea::Header,
    };

    /// Production migration slice for selection and header-owned movement.
    /// Reparenting and Group resizing remain host-owned because they require
    /// persisted hierarchy constraints and transaction policy.
    pub const SELECTION_AND_MOVE: Self = Self {
        select: true,
        select_wires: true,
        marquee: true,
        move_items: true,
        connect: false,
        disconnect: false,
        delete: false,
        reparent: false,
        resize_groups: false,
        layout_swipe: LayoutSwipeHitArea::Header,
    };

    /// Overview interaction keeps large semantic targets and blank-canvas
    /// deselection available while precise wire and marquee gestures are off.
    pub const OVERVIEW_SELECTION: Self = Self {
        select: true,
        select_wires: false,
        marquee: false,
        move_items: false,
        connect: false,
        disconnect: false,
        delete: false,
        reparent: false,
        resize_groups: false,
        layout_swipe: LayoutSwipeHitArea::Node,
    };
}

impl Default for InteractionOptions {
    fn default() -> Self {
        Self::ALL
    }
}

#[derive(Clone, Debug)]
pub(super) enum Movable<NodeId, GroupId> {
    Node(NodeId),
    Group(GroupId),
}

#[derive(Clone, Debug)]
pub(super) enum Gesture<NodeId, PortId, WireId, GroupId> {
    /// Claims an otherwise-unowned Node body press without promoting it into
    /// movement. An interactive host widget preempts this through
    /// `pointer_blocked` and keeps its own click or drag lifecycle.
    Hold {
        transform: egui::emath::TSTransform,
    },
    Marquee {
        start: Pos2,
        current: Pos2,
        additive: bool,
        transform: egui::emath::TSTransform,
    },
    Move {
        items: Vec<Movable<NodeId, GroupId>>,
        grabbed: Movable<NodeId, GroupId>,
        start_screen: Pos2,
        started: bool,
        previous: Pos2,
        current: Pos2,
        transform: egui::emath::TSTransform,
    },
    Connect {
        from: PortId,
        current: Pos2,
        transform: egui::emath::TSTransform,
    },
    Reconnect {
        wire: WireId,
        endpoint: crate::wire::ReconnectEndpoint,
        current: Pos2,
        transform: egui::emath::TSTransform,
    },
    Resize {
        group: GroupId,
        initial_rect: Rect,
        start: Pos2,
        current: Pos2,
        transform: egui::emath::TSTransform,
    },
    LayoutSwipe(LayoutSwipeState<NodeId>),
}

impl<NodeId, PortId, WireId, GroupId> Gesture<NodeId, PortId, WireId, GroupId> {
    const fn transform(&self) -> egui::emath::TSTransform {
        match self {
            Self::Hold { transform }
            | Self::Marquee { transform, .. }
            | Self::Move { transform, .. }
            | Self::Connect { transform, .. }
            | Self::Reconnect { transform, .. }
            | Self::Resize { transform, .. } => *transform,
            Self::LayoutSwipe(gesture) => gesture.transform(),
        }
    }
}

/// Pointer-lifetime state only. It deliberately contains no graph snapshot,
/// authoritative selection, position map, connection list, undo entry, or
/// render cache.
#[derive(Clone, Debug)]
pub struct InteractionState<NodeId, PortId, WireId, GroupId> {
    pub(super) gesture: Option<Gesture<NodeId, PortId, WireId, GroupId>>,
}

impl<NodeId, PortId, WireId, GroupId> Default
    for InteractionState<NodeId, PortId, WireId, GroupId>
{
    fn default() -> Self {
        Self { gesture: None }
    }
}

impl<NodeId, PortId, WireId, GroupId> InteractionState<NodeId, PortId, WireId, GroupId> {
    /// Transform frozen for the active direct-manipulation gesture.
    pub fn locked_transform(&self) -> Option<egui::emath::TSTransform> {
        self.gesture.as_ref().map(Gesture::transform)
    }

    pub const fn is_marquee_active(&self) -> bool {
        matches!(self.gesture, Some(Gesture::Marquee { .. }))
    }

    /// Whether one Node or Group header owns the current primary gesture.
    pub const fn is_move_active(&self) -> bool {
        matches!(self.gesture, Some(Gesture::Move { .. }))
    }

    /// Whether the host must suppress competing move, reparent, pan, and zoom
    /// behavior while a directional-layout gesture owns the pointer.
    pub const fn is_layout_swipe_active(&self) -> bool {
        matches!(self.gesture, Some(Gesture::LayoutSwipe(_)))
    }

    pub const fn is_active(&self) -> bool {
        self.gesture.is_some()
    }

    pub fn cancel(&mut self) {
        self.gesture = None;
    }

    /// Cancel transient state and report whether a position-changing Move was
    /// active. Internal input paths use this to emit one typed Move end.
    pub(super) fn cancel_started_move(&mut self) -> bool {
        let moved = matches!(self.gesture, Some(Gesture::Move { started: true, .. }));
        self.cancel();
        moved
    }
}
