use egui::{Pos2, Rect};

use crate::{
    GraphFrame, GroupDescriptor, ItemId, PortDescriptor, ReconnectEndpoint, WireDescriptor,
};

const WIRE_SELECTION_SAMPLES: u16 = 48;

const PORT_HIT_RADIUS: f32 = 9.0;
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

pub(super) fn node<'a, NodeId, PortId, WireId, GroupId, Key>(
    frame: &'a GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    position: Pos2,
) -> Option<&'a NodeId>
where
    NodeId: Eq,
{
    frame.selection_order.iter().rev().find_map(|item| {
        let ItemId::Node(node_id) = item else {
            return None;
        };
        frame
            .nodes
            .iter()
            .find(|node| node.id == *node_id && node.rect.contains(position))
            .map(|node| &node.id)
    })
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
        .filter(|port| port.connectable && port.center.distance(position) <= radius)
        .min_by(|left, right| {
            left.center
                .distance_sq(position)
                .total_cmp(&right.center.distance_sq(position))
        })
}

pub(super) fn wire<'a, NodeId, PortId, WireId, GroupId, Key>(
    frame: &'a GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    position: Pos2,
) -> Option<&'a WireDescriptor<PortId, WireId>> {
    let scale = frame.transform.scaling.abs().max(f32::EPSILON);
    frame.wires.iter().rev().find(|wire| {
        let geometry = wire.curve.interaction_geometry(scale);
        wire.curve.distance_to(position) <= geometry.body_hit_radius()
    })
}

pub(super) fn reconnect_handle<NodeId, PortId, WireId, GroupId, Key>(
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    position: Pos2,
) -> Option<(WireId, ReconnectEndpoint)>
where
    WireId: Clone + Eq,
{
    let scale = frame.transform.scaling.abs().max(f32::EPSILON);
    let selected_wire = match frame.selection.primary.as_ref() {
        Some(ItemId::Wire(wire)) => Some(wire),
        _ => frame.selection.items.iter().find_map(|item| match item {
            ItemId::Wire(wire) => Some(wire),
            ItemId::Node(_) | ItemId::Group(_) => None,
        }),
    }?;
    let wire = frame
        .wires
        .iter()
        .find(|wire| wire.editable && wire.id == *selected_wire)?;
    let geometry = wire.curve.interaction_geometry(scale);
    [ReconnectEndpoint::Source, ReconnectEndpoint::Target]
        .into_iter()
        .find(|endpoint| {
            geometry.reconnect_handle(*endpoint).distance(position)
                <= geometry.reconnect_handle_hit_radius()
        })
        .map(|endpoint| (wire.id.clone(), endpoint))
}

/// Finds a point on the rendered curve that the production interaction order
/// will attribute to `wire_id`, excluding Nodes, ports, resize edges, another
/// topmost wire, and a selected wire's reconnect handles.
pub(crate) fn wire_selection_target<NodeId, PortId, WireId, GroupId, Key>(
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    wire_id: &WireId,
) -> Option<Pos2>
where
    NodeId: Clone + Eq,
    WireId: Clone + Eq,
    GroupId: Clone + Eq,
{
    let curve = frame.wires.iter().find(|wire| &wire.id == wire_id)?.curve;
    let midpoint = WIRE_SELECTION_SAMPLES / 2;
    let mut samples = (1..WIRE_SELECTION_SAMPLES).collect::<Vec<_>>();
    samples.sort_by_key(|sample| sample.abs_diff(midpoint));
    samples.into_iter().find_map(|sample| {
        let graph_position = curve.point(f32::from(sample) / f32::from(WIRE_SELECTION_SAMPLES));
        let screen_position = frame.screen_position(graph_position);
        let available = frame.viewport.contains(screen_position)
            && reconnect_handle(frame, graph_position).is_none()
            && port(frame, graph_position).is_none()
            && group_resize(frame, graph_position).is_none()
            && selectable(frame, graph_position).is_none()
            && wire(frame, graph_position).is_some_and(|wire| &wire.id == wire_id);
        available.then_some(screen_position)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AuthoritativeSelection, PortDirection, PortOwner, TypeKey};

    fn compatible(source: &u8, target: &u8) -> bool {
        source == target
    }

    #[test]
    fn overlapping_hit_radii_choose_the_nearest_port_not_storage_order() {
        let ports = [
            crate::PortDescriptor {
                id: 1_u8,
                owner: PortOwner::<u8, u8>::Node(1_u8),
                label: "Exact",
                center: egui::pos2(20.0, 20.0),
                direction: PortDirection::Input,
                type_key: TypeKey::new(1_u8),
                connectable: true,
            },
            crate::PortDescriptor {
                id: 2_u8,
                owner: PortOwner::<u8, u8>::Node(2_u8),
                label: "Adjacent row",
                center: egui::pos2(20.0, 27.0),
                direction: PortDirection::Input,
                type_key: TypeKey::new(2_u8),
                connectable: true,
            },
        ];
        let frame: crate::GraphFrame<'_, u8, u8, u8, u8, u8> = crate::GraphFrame {
            viewport: egui::Rect::EVERYTHING,
            transform: egui::emath::TSTransform::IDENTITY,
            nodes: &[],
            ports: &ports,
            wires: &[],
            groups: &[],
            ports_compatible: compatible,
            selection_order: &[],
            selection: AuthoritativeSelection::default(),
        };

        assert_eq!(port(&frame, ports[0].center).map(|port| port.id), Some(1));
        assert_eq!(port(&frame, ports[1].center).map(|port| port.id), Some(2));
    }
}
