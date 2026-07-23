use egui::{Pos2, Rect};

use crate::{after_click, after_marquee, GraphFrame, ItemId};

use super::{hit, EditorOutput, Movable};

const MARQUEE_DRAG_THRESHOLD: f32 = 4.0;

pub(super) fn finish_marquee<NodeId, PortId, WireId, GroupId, Key>(
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    start: Pos2,
    current: Pos2,
    additive: bool,
    transform: egui::emath::TSTransform,
    outputs: &mut Vec<EditorOutput<NodeId, PortId, WireId, GroupId>>,
) where
    NodeId: Clone + Eq,
    WireId: Clone + Eq,
    GroupId: Clone + Eq,
{
    if start.distance(current) < MARQUEE_DRAG_THRESHOLD {
        if !additive {
            clear(frame, outputs);
        }
        return;
    }

    let screen_rect = Rect::from_two_pos(start, current);
    let hits = frame
        .selection_order
        .iter()
        .filter(|item| {
            hit::selectable_rect(frame, item)
                .is_some_and(|rect| screen_rect.intersects(transform * rect))
        })
        .cloned()
        .collect::<Vec<_>>();
    let current = frame
        .selection
        .items
        .iter()
        .filter(|item| !matches!(item, ItemId::Wire(_)))
        .cloned()
        .collect::<Vec<_>>();
    let (items, primary) = after_marquee(&current, &hits, additive);
    outputs.push(EditorOutput::Select { items, primary });
    deselect_wires(frame, outputs);
}

pub(super) fn select_non_wire<NodeId, PortId, WireId, GroupId, Key>(
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    clicked: ItemId<NodeId, GroupId, WireId>,
    additive: bool,
    outputs: &mut Vec<EditorOutput<NodeId, PortId, WireId, GroupId>>,
) where
    NodeId: Clone + Eq,
    WireId: Clone + Eq,
    GroupId: Clone + Eq,
{
    let current = frame
        .selection
        .items
        .iter()
        .filter(|item| !matches!(item, ItemId::Wire(_)))
        .cloned()
        .collect::<Vec<_>>();
    let primary = frame
        .selection
        .primary
        .as_ref()
        .filter(|item| !matches!(item, ItemId::Wire(_)))
        .cloned();
    let (items, primary) = after_click(&current, primary, clicked, additive);
    outputs.push(EditorOutput::Select { items, primary });
    deselect_wires(frame, outputs);
}

pub(super) fn clear<NodeId, PortId, WireId, GroupId, Key>(
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    outputs: &mut Vec<EditorOutput<NodeId, PortId, WireId, GroupId>>,
) where
    WireId: Clone,
{
    outputs.push(EditorOutput::Select {
        items: Vec::new(),
        primary: None,
    });
    deselect_wires(frame, outputs);
}

pub(super) fn movable_snapshot<NodeId, PortId, WireId, GroupId, Key>(
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    clicked: ItemId<NodeId, GroupId, WireId>,
) -> Vec<Movable<NodeId, GroupId>>
where
    NodeId: Clone + Eq,
    WireId: Eq,
    GroupId: Clone + Eq,
{
    if !frame.selection.items.contains(&clicked) {
        return match clicked {
            ItemId::Node(node) => vec![Movable::Node(node)],
            ItemId::Group(group) => vec![Movable::Group(group)],
            ItemId::Wire(_) => Vec::new(),
        };
    }
    frame
        .selection
        .items
        .iter()
        .filter_map(|item| match item {
            ItemId::Node(node) => Some(Movable::Node(node.clone())),
            ItemId::Group(group) => Some(Movable::Group(group.clone())),
            ItemId::Wire(_) => None,
        })
        .collect()
}

fn deselect_wires<NodeId, PortId, WireId, GroupId, Key>(
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    outputs: &mut Vec<EditorOutput<NodeId, PortId, WireId, GroupId>>,
) where
    WireId: Clone,
{
    outputs.extend(frame.selection.items.iter().filter_map(|item| match item {
        ItemId::Wire(wire) => Some(EditorOutput::DeselectWire { wire: wire.clone() }),
        ItemId::Node(_) | ItemId::Group(_) => None,
    }));
}
