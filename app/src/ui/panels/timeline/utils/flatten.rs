use library::model::project::Project;
use library::model::{Clip, Track};
use std::collections::HashSet;
use uuid::Uuid;

/// Represents a single row in the timeline display
#[derive(Debug)]
pub enum DisplayRow<'a> {
    /// A track header row (always shown for each track)
    TrackHeader {
        track: &'a Track,
        depth: usize,
        is_expanded: bool,
        visible_row_index: usize,
    },
    /// A clip row (shown when parent track is expanded)
    ClipRow {
        clip: &'a Clip,
        parent_track: &'a Track,
        depth: usize,
        visible_row_index: usize,
    },
}

impl<'a> DisplayRow<'a> {
    pub fn visible_row_index(&self) -> usize {
        match self {
            DisplayRow::TrackHeader {
                visible_row_index, ..
            } => *visible_row_index,
            DisplayRow::ClipRow {
                visible_row_index, ..
            } => *visible_row_index,
        }
    }

    pub fn track_id(&self) -> Uuid {
        match self {
            DisplayRow::TrackHeader { track, .. } => track.id,
            DisplayRow::ClipRow { parent_track, .. } => parent_track.id,
        }
    }
}

/// Flatten tracks into display rows using the new Node-based structure
/// - Track header always shown
/// - When collapsed: clips are drawn on the track header row (handled by clips.rs)
/// - When expanded: each clip gets its own row below the header
pub fn flatten_tracks_to_rows<'a>(
    project: &'a Project,
    track_ids: &[Uuid],
    expanded_tracks: &HashSet<Uuid>,
) -> Vec<DisplayRow<'a>> {
    let mut rows = Vec::new();
    let mut current_row_index = 0;

    fn process_track<'a>(
        project: &'a Project,
        track_id: Uuid,
        expanded_tracks: &HashSet<Uuid>,
        depth: usize,
        rows: &mut Vec<DisplayRow<'a>>,
        current_row_index: &mut usize,
    ) {
        let Some(track) = project.get_track(track_id) else {
            return;
        };

        let is_expanded = expanded_tracks.contains(&track_id);

        rows.push(DisplayRow::TrackHeader {
            track,
            depth,
            is_expanded,
            visible_row_index: *current_row_index,
        });
        *current_row_index += 1;

        if is_expanded {
            // Later Clips render on top, so present them first in expanded tracks.
            for clip_id in track.clip_ids.iter().rev() {
                if let Some(clip) = project.get_clip(*clip_id) {
                    rows.push(DisplayRow::ClipRow {
                        clip,
                        parent_track: track,
                        depth: depth + 1,
                        visible_row_index: *current_row_index,
                    });
                    *current_row_index += 1;
                }
            }
        }
    }

    // Canonical Composition order is back-to-front. Present front-to-back so
    // the visually highest Track is also the layer rendered on top.
    for track_id in track_ids.iter().rev() {
        process_track(
            project,
            *track_id,
            expanded_tracks,
            0,
            &mut rows,
            &mut current_row_index,
        );
    }

    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_track_list_produces_no_rows() {
        let project = Project::new("test");

        assert!(flatten_tracks_to_rows(&project, &[], &HashSet::new()).is_empty());
    }

    #[test]
    fn top_level_tracks_present_front_to_back_and_show_headers() {
        let mut project = Project::new("test");
        let first = Track::new("first");
        let second = Track::new("second");
        let track_ids = vec![first.id, second.id];
        assert!(
            project.add_track(first).is_ok(),
            "container structural Merge insertion must succeed"
        );
        assert!(
            project.add_track(second).is_ok(),
            "container structural Merge insertion must succeed"
        );

        let rows = flatten_tracks_to_rows(&project, &track_ids, &HashSet::new());
        let row_track_ids: Vec<_> = rows.iter().map(DisplayRow::track_id).collect();

        assert_eq!(
            row_track_ids,
            track_ids.iter().rev().copied().collect::<Vec<_>>()
        );
        assert!(rows
            .iter()
            .all(|row| matches!(row, DisplayRow::TrackHeader { depth: 0, .. })));
    }

    #[test]
    fn timeline_projection_reads_reordered_composition_track_ids_immediately() {
        let mut project = Project::new("test");
        let (mut composition, first) =
            library::model::project::Composition::new("comp", 1920, 1080, 30.0, 10.0);
        let second = Track::new("second");
        let third = Track::new("third");
        let composition_id = composition.id;
        let ids = [first.id, second.id, third.id];
        composition.track_ids.extend([second.id, third.id]);
        assert!(
            project.add_track(first).is_ok(),
            "container structural Merge insertion must succeed"
        );
        assert!(
            project.add_track(second).is_ok(),
            "container structural Merge insertion must succeed"
        );
        assert!(
            project.add_track(third).is_ok(),
            "container structural Merge insertion must succeed"
        );
        assert!(
            project.add_composition(composition).is_ok(),
            "container structural Merge insertion must succeed"
        );

        project
            .move_track_within_composition(composition_id, ids[2], 0)
            .unwrap();
        let authoritative_order = project
            .get_composition(composition_id)
            .unwrap()
            .track_ids
            .clone();
        let rows = flatten_tracks_to_rows(&project, &authoritative_order, &HashSet::new());

        assert_eq!(
            rows.iter().map(DisplayRow::track_id).collect::<Vec<_>>(),
            vec![ids[1], ids[0], ids[2]]
        );
    }
}
