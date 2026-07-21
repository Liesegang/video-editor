//! Vertical column packing and structural typed-Merge adjacency constraints.

use std::collections::BTreeMap;

use library::model::Project;
use uuid::Uuid;

use super::node_geometry::estimated_node_size;
use crate::ui::panels::node_editor::AUTO_LAYOUT_ROW_GAP;

pub(super) fn pack_column(
    project: &Project,
    group: &[Uuid],
    x: f32,
    origin_y: f32,
    positions: &mut BTreeMap<Uuid, [f32; 2]>,
) {
    let mut y = origin_y;
    for node_id in group {
        positions.insert(*node_id, [x, y]);
        y += estimated_node_size(project, *node_id).y + AUTO_LAYOUT_ROW_GAP;
    }
}

/// Keep each container's canonical Image and Sound Merge adjacent while
/// retaining the column's deterministic order for every unrelated Node.
pub(super) fn enforce_structural_pair_row(
    project: &Project,
    group: &[Uuid],
    positions: &mut BTreeMap<Uuid, [f32; 2]>,
    origin_y: f32,
) {
    let pair = project
        .compositions
        .iter()
        .map(|composition| {
            (
                composition.structural_merge_node_id,
                composition.structural_sound_merge_node_id,
            )
        })
        .chain(project.tracks.values().map(|track| {
            (
                track.structural_merge_node_id,
                track.structural_sound_merge_node_id,
            )
        }))
        .find(|(image, sound)| group.contains(image) && group.contains(sound));
    let Some((image, sound)) = pair else {
        return;
    };
    let Some(column_x) = positions.get(&image).map(|position| position[0]) else {
        return;
    };
    let mut ordered = group.to_vec();
    ordered.sort_by(|left, right| {
        positions
            .get(left)
            .map_or(origin_y, |position| position[1])
            .total_cmp(
                &positions
                    .get(right)
                    .map_or(origin_y, |position| position[1]),
            )
            .then_with(|| left.cmp(right))
    });
    let Some(image_index) = ordered.iter().position(|node_id| *node_id == image) else {
        return;
    };
    let Some(sound_index) = ordered.iter().position(|node_id| *node_id == sound) else {
        return;
    };
    let insert_at = image_index.min(sound_index);
    ordered.retain(|node_id| *node_id != image && *node_id != sound);
    ordered.insert(insert_at.min(ordered.len()), image);
    ordered.insert((insert_at + 1).min(ordered.len()), sound);
    pack_column(project, &ordered, column_x, origin_y, positions);
}
