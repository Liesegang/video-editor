use library::model::project::PortOwner;
use library::model::Project;
use uuid::Uuid;

use crate::ui::panels::node_editor::{AutoLayoutPlan, MIN_CONTAINER_SIZE};

pub(in crate::ui::panels::node_editor) fn apply_auto_layout(
    project: &mut Project,
    composition_id: Uuid,
    plan: &AutoLayoutPlan,
) -> bool {
    let mut changed = false;
    for (node_id, position) in &plan.node_positions {
        let Some(node) = project.get_node_mut(*node_id) else {
            continue;
        };
        if node.ui_position != *position {
            node.ui_position = *position;
            changed = true;
        }
    }
    for (clip_id, layout) in &plan.clip_layouts {
        let Some(clip) = project.get_clip_mut(*clip_id) else {
            continue;
        };
        if clip.ui_position != layout.position {
            clip.ui_position = layout.position;
            changed = true;
        }
        if clip.ui_size != layout.size {
            clip.ui_size = layout.size;
            changed = true;
        }
    }
    for (track_id, layout) in &plan.track_layouts {
        let Some(track) = project.get_track_mut(*track_id) else {
            continue;
        };
        if track.ui_position != layout.position {
            track.ui_position = layout.position;
            changed = true;
        }
        if track.ui_size != layout.size {
            track.ui_size = layout.size;
            changed = true;
        }
    }
    if let (Some(size), Some(composition)) = (
        plan.composition_size,
        project.get_composition_mut(composition_id),
    ) {
        if composition.ui_size != size {
            composition.ui_size = size;
            changed = true;
        }
    }
    changed
}

pub(in crate::ui::panels::node_editor) fn translate_container(
    project: &mut Project,
    owner: PortOwner,
    delta: [f32; 2],
) -> bool {
    if delta[0].abs() < f32::EPSILON && delta[1].abs() < f32::EPSILON {
        return false;
    }
    match owner {
        PortOwner::Composition(composition_id) => {
            let Some((track_ids, node_ids)) = project
                .get_composition(composition_id)
                .map(|composition| (composition.track_ids.clone(), composition.node_ids.clone()))
            else {
                return false;
            };
            if let Some(composition) = project.get_composition_mut(composition_id) {
                translate_position(&mut composition.ui_position, delta);
            }
            for track_id in track_ids {
                translate_track_tree(project, track_id, delta);
            }
            translate_nodes(project, &node_ids, delta);
            true
        }
        PortOwner::Track(track_id) => translate_track_tree(project, track_id, delta),
        PortOwner::Clip(clip_id) => translate_clip_tree(project, clip_id, delta),
        PortOwner::Node(_) => false,
    }
}

fn translate_track_tree(project: &mut Project, track_id: Uuid, delta: [f32; 2]) -> bool {
    let Some((clip_ids, node_ids)) = project
        .get_track(track_id)
        .map(|track| (track.clip_ids.clone(), track.node_ids.clone()))
    else {
        return false;
    };
    if let Some(track) = project.get_track_mut(track_id) {
        translate_position(&mut track.ui_position, delta);
    }
    translate_nodes(project, &node_ids, delta);
    for clip_id in clip_ids {
        translate_clip_tree(project, clip_id, delta);
    }
    true
}

fn translate_clip_tree(project: &mut Project, clip_id: Uuid, delta: [f32; 2]) -> bool {
    let Some(node_ids) = project.get_clip(clip_id).map(|clip| clip.node_ids.clone()) else {
        return false;
    };
    if let Some(clip) = project.get_clip_mut(clip_id) {
        translate_position(&mut clip.ui_position, delta);
    }
    translate_nodes(project, &node_ids, delta);
    true
}

fn translate_nodes(project: &mut Project, node_ids: &[Uuid], delta: [f32; 2]) {
    for node_id in node_ids {
        if let Some(node) = project.get_node_mut(*node_id) {
            translate_position(&mut node.ui_position, delta);
        }
    }
}

fn translate_position(position: &mut [f32; 2], delta: [f32; 2]) {
    position[0] += delta[0];
    position[1] += delta[1];
}

pub(in crate::ui::panels::node_editor) fn set_container_size(
    project: &mut Project,
    owner: PortOwner,
    size: [f32; 2],
) -> bool {
    let size = [
        size[0].max(MIN_CONTAINER_SIZE.x),
        size[1].max(MIN_CONTAINER_SIZE.y),
    ];
    match owner {
        PortOwner::Composition(id) => {
            let Some(composition) = project.get_composition_mut(id) else {
                return false;
            };
            if composition.ui_size == size {
                return false;
            }
            composition.ui_size = size;
            true
        }
        PortOwner::Track(id) => {
            let Some(track) = project.get_track_mut(id) else {
                return false;
            };
            if track.ui_size == size {
                return false;
            }
            track.ui_size = size;
            true
        }
        PortOwner::Clip(id) => {
            let Some(clip) = project.get_clip_mut(id) else {
                return false;
            };
            if clip.ui_size == size {
                return false;
            }
            clip.ui_size = size;
            true
        }
        PortOwner::Node(_) => false,
    }
}

pub(in crate::ui::panels::node_editor) fn set_container_geometry(
    project: &mut Project,
    owner: PortOwner,
    position: [f32; 2],
    size: [f32; 2],
) -> bool {
    let size = [
        size[0].max(MIN_CONTAINER_SIZE.x),
        size[1].max(MIN_CONTAINER_SIZE.y),
    ];
    match owner {
        PortOwner::Composition(id) => project.get_composition_mut(id).is_some_and(|item| {
            if item.ui_position == position && item.ui_size == size {
                return false;
            }
            item.ui_position = position;
            item.ui_size = size;
            true
        }),
        PortOwner::Track(id) => project.get_track_mut(id).is_some_and(|item| {
            if item.ui_position == position && item.ui_size == size {
                return false;
            }
            item.ui_position = position;
            item.ui_size = size;
            true
        }),
        PortOwner::Clip(id) => project.get_clip_mut(id).is_some_and(|item| {
            if item.ui_position == position && item.ui_size == size {
                return false;
            }
            item.ui_position = position;
            item.ui_size = size;
            true
        }),
        PortOwner::Node(_) => false,
    }
}
