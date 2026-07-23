use egui::{Pos2, Rect};

use crate::{GraphFrame, GroupDescriptor, ItemId, PortDescriptor, WireDescriptor};

const PORT_HIT_RADIUS: f32 = 9.0;
const WIRE_HIT_RADIUS: f32 = 8.0;
const GROUP_RESIZE_HIT_WIDTH: f32 = 7.0;

pub(super) fn selectable<NodeId, PortId, WireId, GroupId, Key>(
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    position: Pos2,
) -> Option<ItemId<NodeId, GroupId, WireId>>
where
    NodeId: Clone + Eq,
    WireId: Clone,
    GroupId: Clone + Eq,
{
    frame
        .selection_order
        .iter()
        .rev()
        .find(|item| selectable_rect(frame, item).is_some_and(|rect| rect.contains(position)))
        .cloned()
}

pub(super) fn selectable_rect<NodeId, PortId, WireId, GroupId, Key>(
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    item: &ItemId<NodeId, GroupId, WireId>,
) -> Option<Rect>
where
    NodeId: Eq,
    GroupId: Eq,
{
    match item {
        ItemId::Node(node_id) => frame
            .nodes
            .iter()
            .find(|node| node.id == *node_id)
            .map(|node| node.rect),
        ItemId::Group(group_id) => frame
            .groups
            .iter()
            .find(|group| group.id == *group_id)
            .map(|group| group.header_rect),
        ItemId::Wire(_) => None,
    }
}

pub(super) fn move_handle<NodeId, PortId, WireId, GroupId, Key>(
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    item: &ItemId<NodeId, GroupId, WireId>,
    position: Pos2,
) -> bool
where
    NodeId: Eq,
    GroupId: Eq,
{
    match item {
        ItemId::Node(node_id) => frame
            .nodes
            .iter()
            .find(|node| node.id == *node_id)
            .is_some_and(|node| node.header_rect.contains(position)),
        ItemId::Group(group_id) => frame
            .groups
            .iter()
            .find(|group| group.id == *group_id)
            .is_some_and(|group| group.header_rect.contains(position)),
        ItemId::Wire(_) => false,
    }
}

pub(super) fn deepest_group<'a, NodeId, PortId, WireId, GroupId, Key>(
    frame: &'a GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    position: Pos2,
) -> Option<&'a GroupDescriptor<'a, GroupId>> {
    frame
        .groups
        .iter()
        .filter(|group| group.rect.contains(position))
        .min_by(|left, right| {
            let left_area = left.rect.width() * left.rect.height();
            let right_area = right.rect.width() * right.rect.height();
            left_area.total_cmp(&right_area)
        })
}

pub(super) fn group_resize<'a, NodeId, PortId, WireId, GroupId, Key>(
    frame: &'a GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    position: Pos2,
) -> Option<&'a GroupDescriptor<'a, GroupId>> {
    let scale = frame.transform.scaling.abs().max(f32::EPSILON);
    let width = GROUP_RESIZE_HIT_WIDTH / scale;
    frame.groups.iter().rev().find(|group| {
        if !group.resizable || !group.rect.expand(width).contains(position) {
            return false;
        }
        (position.x - group.rect.right()).abs() <= width
            || (position.y - group.rect.bottom()).abs() <= width
    })
}

pub(super) fn port<'a, NodeId, PortId, WireId, GroupId, Key>(
    frame: &'a GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    position: Pos2,
) -> Option<&'a PortDescriptor<'a, NodeId, PortId, GroupId, Key>> {
    let scale = frame.transform.scaling.abs().max(f32::EPSILON);
    let radius = PORT_HIT_RADIUS / scale;
    frame
        .ports
        .iter()
        .rev()
        .find(|port| port.connectable && port.center.distance(position) <= radius)
}

pub(super) fn wire<'a, NodeId, PortId, WireId, GroupId, Key>(
    frame: &'a GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    position: Pos2,
) -> Option<&'a WireDescriptor<PortId, WireId>> {
    let scale = frame.transform.scaling.abs().max(f32::EPSILON);
    let radius = WIRE_HIT_RADIUS / scale;
    frame
        .wires
        .iter()
        .rev()
        .find(|wire| wire.curve.distance_to(position) <= radius)
}
