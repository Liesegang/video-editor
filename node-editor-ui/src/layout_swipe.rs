//! Domain-neutral directional-layout gesture values.

use egui::{emath::TSTransform, Modifiers, Pos2};

use crate::{GraphFrame, ItemId, NodeDescriptor};

/// The screen-space axis selected by a directional layout swipe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutSwipeAxis {
    Horizontal,
    Vertical,
}

/// Which part of a Node may arm a layout swipe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutSwipeHitArea {
    Disabled,
    Header,
    /// Useful for an overview where the detailed header is no longer a
    /// practical target.
    Node,
}

/// One phase in the layout-swipe ownership protocol.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutSwipePhase {
    Start,
    Update,
    Commit,
    Cancel,
}

/// Immutable host intent emitted while the UI owns a layout swipe.
///
/// Coordinates are screen points. `transform` and `modifiers` are frozen at
/// pointer press, so a host can interpret every phase against one stable
/// graph projection and one exact command mode. The host remains responsible
/// for mapping the axis, signed displacement, and modifiers to domain edits.
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutSwipeIntent<NodeId> {
    pub phase: LayoutSwipePhase,
    pub anchor: NodeId,
    pub start: Pos2,
    pub current: Pos2,
    pub axis: Option<LayoutSwipeAxis>,
    pub modifiers: Modifiers,
    pub transform: TSTransform,
}

pub(crate) const ACTIVATION_DISTANCE: f32 = 12.0;

pub(crate) fn hit_anchor<'a, NodeId, PortId, WireId, GroupId, Key>(
    frame: &'a GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    position: Pos2,
    hit_area: LayoutSwipeHitArea,
) -> Option<&'a NodeDescriptor<'a, NodeId, GroupId>>
where
    NodeId: Eq,
{
    if hit_area == LayoutSwipeHitArea::Disabled {
        return None;
    }
    frame.selection_order.iter().rev().find_map(|item| {
        let ItemId::Node(node_id) = item else {
            return None;
        };
        frame.nodes.iter().find(|node| {
            node.id == *node_id
                && match hit_area {
                    LayoutSwipeHitArea::Disabled => false,
                    LayoutSwipeHitArea::Header => node.header_rect.contains(position),
                    LayoutSwipeHitArea::Node => node.rect.contains(position),
                }
        })
    })
}

#[derive(Clone, Debug)]
pub(crate) struct LayoutSwipeState<NodeId> {
    anchor: NodeId,
    start: Pos2,
    current: Pos2,
    axis: Option<LayoutSwipeAxis>,
    modifiers: Modifiers,
    transform: TSTransform,
}

impl<NodeId> LayoutSwipeState<NodeId> {
    pub(crate) const fn new(
        anchor: NodeId,
        start: Pos2,
        modifiers: Modifiers,
        transform: TSTransform,
    ) -> Self {
        Self {
            anchor,
            start,
            current: start,
            axis: None,
            modifiers,
            transform,
        }
    }

    pub(crate) const fn transform(&self) -> TSTransform {
        self.transform
    }

    pub(crate) const fn is_activated(&self) -> bool {
        self.axis.is_some()
    }
}

impl<NodeId: Clone> LayoutSwipeState<NodeId> {
    pub(crate) fn start_intent(&self) -> LayoutSwipeIntent<NodeId> {
        self.intent(LayoutSwipePhase::Start)
    }

    pub(crate) fn update(&mut self, current: Pos2) -> Option<LayoutSwipeIntent<NodeId>> {
        if current == self.current {
            return None;
        }
        self.current = current;
        if self.axis.is_none() {
            let displacement = current - self.start;
            if displacement.length() < ACTIVATION_DISTANCE {
                return None;
            }
            self.axis = Some(if displacement.x.abs() >= displacement.y.abs() {
                LayoutSwipeAxis::Horizontal
            } else {
                LayoutSwipeAxis::Vertical
            });
        }
        Some(self.intent(LayoutSwipePhase::Update))
    }

    pub(crate) fn finish(
        mut self,
        phase: LayoutSwipePhase,
        current: Option<Pos2>,
    ) -> LayoutSwipeIntent<NodeId> {
        if let Some(current) = current {
            self.current = current;
        }
        self.intent(phase)
    }

    fn intent(&self, phase: LayoutSwipePhase) -> LayoutSwipeIntent<NodeId> {
        LayoutSwipeIntent {
            phase,
            anchor: self.anchor.clone(),
            start: self.start,
            current: self.current,
            axis: self.axis,
            modifiers: self.modifiers,
            transform: self.transform,
        }
    }
}
