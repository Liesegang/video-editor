use crate::{GraphFrame, ItemId};

use super::{EditorOutput, InteractionOptions, InteractionState, MoveEndOutcome};

pub(super) fn outputs<NodeId, PortId, WireId, GroupId, Key>(
    ui: &egui::Ui,
    frame: &GraphFrame<'_, NodeId, PortId, WireId, GroupId, Key>,
    state: &mut InteractionState<NodeId, PortId, WireId, GroupId>,
    options: InteractionOptions,
) -> Vec<EditorOutput<NodeId, PortId, WireId, GroupId>>
where
    NodeId: Clone,
    WireId: Clone + Eq,
    GroupId: Clone,
{
    let wants_keyboard = ui.ctx().wants_keyboard_input();
    let (delete, escape) = ui.input(|input| {
        (
            input.key_pressed(egui::Key::Delete) || input.key_pressed(egui::Key::Backspace),
            input.key_pressed(egui::Key::Escape),
        )
    });
    if wants_keyboard {
        return Vec::new();
    }

    if escape {
        let moved = state.cancel_started_move();
        let mut outputs = frame
            .selection
            .items
            .iter()
            .filter_map(|item| match item {
                ItemId::Wire(wire) => Some(EditorOutput::DeselectWire { wire: wire.clone() }),
                ItemId::Node(_) | ItemId::Group(_) => None,
            })
            .collect::<Vec<_>>();
        if moved {
            outputs.push(EditorOutput::MoveEnd {
                outcome: MoveEndOutcome::Cancelled,
            });
        }
        return outputs;
    }

    if !delete || !options.delete {
        return Vec::new();
    }

    let mut outputs = Vec::new();
    let mut items = Vec::new();
    for item in frame.selection.items {
        match item {
            ItemId::Wire(wire) if options.disconnect => {
                if frame
                    .wires
                    .iter()
                    .any(|descriptor| descriptor.id == *wire && descriptor.editable)
                {
                    outputs.push(EditorOutput::Disconnect { wire: wire.clone() });
                }
            }
            ItemId::Node(node) => items.push(ItemId::Node(node.clone())),
            ItemId::Group(group) => items.push(ItemId::Group(group.clone())),
            ItemId::Wire(_) => {}
        }
    }
    if !items.is_empty() {
        outputs.push(EditorOutput::Delete { items });
    }
    outputs
}
