use std::collections::HashMap;

use library::model::project::Project;
use uuid::Uuid;

use crate::ui::layer_order::destination_index_after_removal;

use super::super::utils::flatten::DisplayRow;
use super::clips::ClipRowLayout;

pub(in crate::ui::panels::timeline) fn calculate_insert_index(
    mouse_y: f32,
    display_rows: &[DisplayRow<'_>],
    project: &Project,
    hovered_track_id: Uuid,
    layout: ClipRowLayout,
) -> Option<(usize, usize)> {
    let header_idx = display_rows.iter().position(|row| {
        row.track_id() == hovered_track_id && matches!(row, DisplayRow::TrackHeader { .. })
    })?;
    let markers = clip_insertion_markers(display_rows, hovered_track_id, project, layout);
    nearest_clip_insertion_slot(mouse_y, &markers).map(|slot| (slot, header_idx))
}

/// Logical insertion slots for an expanded Track. Slot 0 is before the first
/// canonical Clip and `clip_count` is after the last. The Timeline is
/// visually reversed, so canonical slot numbers descend as Y increases.
pub(in crate::ui::panels::timeline) fn clip_insertion_markers(
    display_rows: &[DisplayRow<'_>],
    track_id: Uuid,
    project: &Project,
    layout: ClipRowLayout,
) -> Vec<(usize, f32)> {
    let Some(header_row) = display_rows.iter().position(|row| {
        row.track_id() == track_id && matches!(row, DisplayRow::TrackHeader { .. })
    }) else {
        return Vec::new();
    };
    let Some(track) = project.get_track(track_id) else {
        return Vec::new();
    };
    let clip_count = track.clip_ids.len();
    (0..=clip_count)
        .map(|slot| {
            let boundary_row = header_row + 1 + (clip_count - slot);
            (
                slot,
                layout.content_min_y + boundary_row as f32 * layout.row_step() - layout.scroll_y,
            )
        })
        .collect()
}

pub(in crate::ui::panels::timeline) fn nearest_clip_insertion_slot(
    pointer_y: f32,
    markers: &[(usize, f32)],
) -> Option<usize> {
    markers
        .iter()
        .min_by(|(_, lhs_y), (_, rhs_y)| {
            (pointer_y - *lhs_y)
                .abs()
                .total_cmp(&(pointer_y - *rhs_y).abs())
        })
        .map(|(slot, _)| *slot)
}

/// Convert an insertion slot into the index expected after the source Clip
/// is detached. Adjacent same-Track slots are intentional no-ops.
pub(in crate::ui::panels::timeline) fn destination_index_for_clip_slot(
    same_track: bool,
    source_index: usize,
    insertion_slot: usize,
    target_clip_count: usize,
) -> Option<usize> {
    if !same_track {
        return Some(insertion_slot.min(target_clip_count));
    }
    let destination =
        destination_index_after_removal(source_index, insertion_slot, target_clip_count)?;
    (destination != source_index).then_some(destination)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui::panels::timeline) struct ClipReorderPreview {
    dragged_id: Uuid,
    source_track_id: Uuid,
    target_track_id: Uuid,
    source_index: usize,
    destination_index: usize,
}

impl ClipReorderPreview {
    #[cfg(test)]
    pub(in crate::ui::panels::timeline) fn source_index(self) -> usize {
        self.source_index
    }

    #[cfg(test)]
    pub(in crate::ui::panels::timeline) fn destination_index(self) -> usize {
        self.destination_index
    }
}

#[derive(Default)]
pub(in crate::ui::panels::timeline) struct ClipReorderProjection {
    track_rows: HashMap<Uuid, usize>,
    clip_rows: HashMap<Uuid, usize>,
}

impl ClipReorderProjection {
    pub(in crate::ui::panels::timeline) fn row_for(&self, row: &DisplayRow<'_>) -> Option<usize> {
        match row {
            DisplayRow::TrackHeader { track, .. } => self.track_rows.get(&track.id).copied(),
            DisplayRow::ClipRow { clip, .. } => self.clip_rows.get(&clip.id).copied(),
        }
    }

    pub(in crate::ui::panels::timeline) fn row_for_clip(&self, clip_id: Uuid) -> Option<usize> {
        self.clip_rows.get(&clip_id).copied()
    }

    #[cfg(test)]
    pub(in crate::ui::panels::timeline) fn row_for_track(&self, track_id: Uuid) -> Option<usize> {
        self.track_rows.get(&track_id).copied()
    }
}

pub(in crate::ui::panels::timeline) fn clip_reorder_preview(
    project: &Project,
    dragged_id: Uuid,
    source_track_id: Uuid,
    target_track_id: Uuid,
    canonical_insertion_slot: usize,
) -> Option<ClipReorderPreview> {
    let source = project.get_track(source_track_id)?;
    let source_index = source
        .clip_ids
        .iter()
        .position(|clip_id| *clip_id == dragged_id)?;
    let target_count = project.get_track(target_track_id)?.clip_ids.len();
    let destination_index = if source_track_id == target_track_id {
        destination_index_after_removal(source_index, canonical_insertion_slot, target_count)?
    } else {
        canonical_insertion_slot.min(target_count)
    };
    Some(ClipReorderPreview {
        dragged_id,
        source_track_id,
        target_track_id,
        source_index,
        destination_index,
    })
}

fn clip_ids_for_preview(
    project: &Project,
    track_id: Uuid,
    preview: ClipReorderPreview,
) -> Vec<Uuid> {
    let Some(track) = project.get_track(track_id) else {
        return Vec::new();
    };
    let mut clip_ids = track.clip_ids.clone();
    if track_id == preview.source_track_id {
        clip_ids.retain(|clip_id| *clip_id != preview.dragged_id);
    }
    if track_id == preview.target_track_id {
        let insertion_index = preview.destination_index.min(clip_ids.len());
        clip_ids.insert(insertion_index, preview.dragged_id);
    }
    clip_ids
}

pub(in crate::ui::panels::timeline) fn clip_reorder_projection(
    display_rows: &[DisplayRow<'_>],
    project: &Project,
    preview: ClipReorderPreview,
) -> ClipReorderProjection {
    let mut projection = ClipReorderProjection::default();
    let mut visible_row = 0;
    for row in display_rows {
        let DisplayRow::TrackHeader {
            track, is_expanded, ..
        } = row
        else {
            continue;
        };
        projection.track_rows.insert(track.id, visible_row);
        let track_row = visible_row;
        visible_row += 1;
        let clip_ids = clip_ids_for_preview(project, track.id, preview);
        if !is_expanded {
            for clip_id in clip_ids {
                projection.clip_rows.insert(clip_id, track_row);
            }
            continue;
        }
        for clip_id in clip_ids.into_iter().rev() {
            projection.clip_rows.insert(clip_id, visible_row);
            visible_row += 1;
        }
    }
    projection
}
