use crate::action::HistoryManager;
use crate::state::context_types::NodeEditorState;
use library::model::project::PortOwner;
use library::model::Project;
use std::sync::{Arc, RwLock};

use super::wire::{
    container_for_output_owner, disconnect_editable_wires, insert_node_on_connection,
    splice_existing_node_on_connection,
};
use crate::ui::panels::node_editor::{
    container_output_node_id, set_container_size, NodeEdit, QueuedNodeEdit,
};

pub(in crate::ui::panels::node_editor) fn apply_edit(
    project: &mut Project,
    edit: NodeEdit,
) -> bool {
    match edit {
        NodeEdit::Connect { from, to } => match project.connect_ports(from, to) {
            Ok(_) => true,
            Err(error) => {
                log::warn!("Cannot connect project ports: {error}");
                false
            }
        },
        NodeEdit::Disconnect { from, to } => project.disconnect_ports(&from, &to),
        NodeEdit::DisconnectConnection { connection_id } => {
            project.disconnect_connection(connection_id)
        }
        NodeEdit::DisconnectWires { wires } => disconnect_editable_wires(project, wires),
        NodeEdit::ReconnectConnection {
            connection_id,
            from,
            to,
        } => match project.reconnect_connection(connection_id, from, to) {
            Ok(()) => true,
            Err(error) => {
                log::warn!("Cannot reconnect project wire {connection_id}: {error}");
                false
            }
        },
        NodeEdit::SetConnectionBlendMode {
            connection_id,
            blend_mode,
        } => {
            let Some(connection) = project
                .connections
                .iter()
                .find(|connection| connection.id == connection_id)
            else {
                return false;
            };
            if connection.blend_mode == blend_mode {
                return false;
            }
            match project.set_connection_blend_mode(connection_id, blend_mode) {
                Ok(()) => true,
                Err(error) => {
                    log::warn!("Cannot set authored blend on wire {connection_id}: {error}");
                    false
                }
            }
        }
        NodeEdit::ReorderConnection {
            connection_id,
            new_order,
        } => {
            let Some(connection) = project
                .connections
                .iter()
                .find(|connection| connection.id == connection_id)
            else {
                return false;
            };
            let mut siblings = project
                .connections
                .iter()
                .filter(|candidate| candidate.to == connection.to)
                .map(|candidate| (candidate.order, candidate.id))
                .collect::<Vec<_>>();
            siblings.sort_by_key(|(order, id)| (*order, *id));
            let Some(current_index) = siblings.iter().position(|(_, id)| *id == connection_id)
            else {
                return false;
            };
            let target_index = new_order.max(0) as usize;
            let target_index = target_index.min(siblings.len().saturating_sub(1));
            if target_index == current_index {
                return false;
            }
            match project.reorder_connection(connection_id, target_index as i64) {
                Ok(()) => true,
                Err(error) => {
                    log::warn!("Cannot reorder wire {connection_id}: {error}");
                    false
                }
            }
        }
        NodeEdit::SpliceExistingNode {
            connection_id,
            node_id,
        } => splice_existing_node_on_connection(project, connection_id, node_id),
        NodeEdit::InsertNodeOnConnection {
            connection_id,
            node,
            position,
            composition_id,
        } => insert_node_on_connection(project, connection_id, *node, position, composition_id),
        NodeEdit::SetOutputNode { owner, node_id } => {
            let Some(container) = container_for_output_owner(owner) else {
                return false;
            };
            let before = container_output_node_id(project, owner);
            if before == node_id {
                return false;
            }
            match project.set_output_node(container, node_id) {
                Ok(()) => true,
                Err(error) => {
                    log::warn!("Cannot set container output node: {error}");
                    false
                }
            }
        }
        NodeEdit::Delete { owner } => match owner {
            PortOwner::Node(id) => project.remove_node(id).is_some(),
            PortOwner::Clip(id) => project.remove_clip(id).is_some(),
            PortOwner::Track(id) => project.remove_track(id).is_some(),
            PortOwner::Composition(_) => false,
        },
        NodeEdit::SetEnabled { node_id, enabled } => {
            project.get_node_mut(node_id).is_some_and(|node| {
                if node.enabled == enabled {
                    return false;
                }
                node.enabled = enabled;
                true
            })
        }
        NodeEdit::RenameContainer { owner, name } => match owner {
            PortOwner::Composition(id) => {
                let Some(composition) = project.get_composition_mut(id) else {
                    return false;
                };
                if composition.name == name {
                    return false;
                }
                composition.name = name;
                true
            }
            PortOwner::Track(id) => {
                let Some(track) = project.get_track_mut(id) else {
                    return false;
                };
                if track.name == name {
                    return false;
                }
                track.name = name;
                true
            }
            PortOwner::Clip(id) => {
                let Some(clip) = project.get_clip_mut(id) else {
                    return false;
                };
                if clip.name == name {
                    return false;
                }
                clip.name = name;
                true
            }
            PortOwner::Node(_) => false,
        },
        NodeEdit::ResizeContainer { owner, size } => set_container_size(project, owner, size),
        NodeEdit::ToggleContainer { owner } => match owner {
            PortOwner::Composition(id) => {
                let Some(composition) = project.get_composition_mut(id) else {
                    return false;
                };
                composition.ui_collapsed = !composition.ui_collapsed;
                true
            }
            PortOwner::Track(id) => {
                let Some(track) = project.get_track_mut(id) else {
                    return false;
                };
                track.ui_collapsed = !track.ui_collapsed;
                true
            }
            PortOwner::Clip(id) => {
                let Some(clip) = project.get_clip_mut(id) else {
                    return false;
                };
                clip.ui_collapsed = !clip.ui_collapsed;
                true
            }
            PortOwner::Node(_) => false,
        },
        NodeEdit::Rename { node_id, name } => project.get_node_mut(node_id).is_some_and(|node| {
            if node.name == name {
                return false;
            }
            node.name = name;
            true
        }),
        NodeEdit::SetProperty {
            owner,
            key,
            time,
            value,
        } => match owner {
            PortOwner::Clip(id) => project
                .get_clip_mut(id)
                .is_some_and(|clip| clip.update_property_or_keyframe(&key, time, value, None)),
            PortOwner::Node(id) => project
                .get_node_mut(id)
                .is_some_and(|node| node.update_property_or_keyframe(&key, time, value, None)),
            PortOwner::Composition(_) | PortOwner::Track(_) => false,
        },
    }
}

fn flush_pending_continuous_edit_with_project(
    project: &Project,
    history_manager: &mut HistoryManager,
    node_editor_state: &mut NodeEditorState,
) -> bool {
    if node_editor_state.pending_continuous_edit.take().is_none() {
        return false;
    }
    history_manager.push_project_state(project.clone());
    true
}

pub(in crate::ui::panels::node_editor) fn apply_queued_node_edits(
    project: &mut Project,
    queued_edits: Vec<QueuedNodeEdit>,
    history_manager: &mut HistoryManager,
    node_editor_state: &mut NodeEditorState,
) -> bool {
    let mut changed = false;
    let mut atomic_batch_changed = false;

    for queued in queued_edits {
        match queued {
            QueuedNodeEdit::Atomic(edit) => {
                flush_pending_continuous_edit_with_project(
                    project,
                    history_manager,
                    node_editor_state,
                );
                atomic_batch_changed |= apply_edit(project, edit);
                changed |= atomic_batch_changed;
            }
            QueuedNodeEdit::Continuous {
                pending,
                edit,
                finished,
            } => {
                if atomic_batch_changed {
                    history_manager.push_project_state(project.clone());
                    atomic_batch_changed = false;
                }
                if node_editor_state
                    .pending_continuous_edit
                    .as_ref()
                    .is_some_and(|current| current != &pending)
                {
                    flush_pending_continuous_edit_with_project(
                        project,
                        history_manager,
                        node_editor_state,
                    );
                }

                if edit.is_some_and(|edit| apply_edit(project, edit)) {
                    node_editor_state.pending_continuous_edit = Some(pending.clone());
                    changed = true;
                }
                if finished && node_editor_state.pending_continuous_edit.as_ref() == Some(&pending)
                {
                    flush_pending_continuous_edit_with_project(
                        project,
                        history_manager,
                        node_editor_state,
                    );
                }
            }
        }
    }

    if atomic_batch_changed {
        history_manager.push_project_state(project.clone());
    }
    changed
}

/// Commit a dirty inline Node Editor gesture when the panel stops being the
/// active tab or its owner disappears. The Project has already been updated;
/// this records exactly one final snapshot and clears only ephemeral UI state.
pub fn flush_pending_continuous_edit(
    project_lock: &Arc<RwLock<Project>>,
    history_manager: &mut HistoryManager,
    node_editor_state: &mut NodeEditorState,
) -> bool {
    if node_editor_state.pending_continuous_edit.is_none() {
        return false;
    }
    let Ok(project) = project_lock.read() else {
        return false;
    };
    flush_pending_continuous_edit_with_project(&project, history_manager, node_editor_state)
}
