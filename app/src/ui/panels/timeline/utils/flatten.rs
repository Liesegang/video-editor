use library::model::project::{Track, TrackClip, TrackItem};
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
        has_clips: bool,
        has_sub_tracks: bool,
    },
    /// A clip row (shown when parent track is expanded)
    ClipRow {
        clip: &'a TrackClip,
        parent_track: &'a Track,
        depth: usize,
        visible_row_index: usize,
        child_index: usize,
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

    pub fn depth(&self) -> usize {
        match self {
            DisplayRow::TrackHeader { depth, .. } => *depth,
            DisplayRow::ClipRow { depth, .. } => *depth,
        }
    }
}

/// Flatten tracks into display rows
/// - Track header always shown
/// - When collapsed: clips are drawn on the track header row (handled by clips.rs)
/// - When expanded: each clip gets its own row below the header
pub fn flatten_tracks_to_rows<'a>(
    tracks: &'a [Track],
    expanded_tracks: &HashSet<Uuid>,
) -> Vec<DisplayRow<'a>> {
    let mut rows = Vec::new();
    let mut current_row_index = 0;

    fn process_track<'a>(
        track: &'a Track,
        expanded_tracks: &HashSet<Uuid>,
        depth: usize,
        rows: &mut Vec<DisplayRow<'a>>,
        current_row_index: &mut usize,
    ) {
        let is_expanded = expanded_tracks.contains(&track.id);

        let has_clips = track
            .children
            .iter()
            .any(|item| matches!(item, TrackItem::Clip(_)));
        let has_sub_tracks = track
            .children
            .iter()
            .any(|item| matches!(item, TrackItem::SubTrack(_)));

        rows.push(DisplayRow::TrackHeader {
            track,
            depth,
            is_expanded,
            visible_row_index: *current_row_index,
            has_clips,
            has_sub_tracks,
        });
        *current_row_index += 1;

        if is_expanded {
            for (child_index, item) in track.children.iter().enumerate() {
                match item {
                    TrackItem::Clip(clip) => {
                        rows.push(DisplayRow::ClipRow {
                            clip,
                            parent_track: track,
                            depth: depth + 1,
                            visible_row_index: *current_row_index,
                            child_index,
                        });
                        *current_row_index += 1;
                    }
                    TrackItem::SubTrack(sub_track) => {
                        process_track(
                            sub_track,
                            expanded_tracks,
                            depth + 1,
                            rows,
                            current_row_index,
                        );
                    }
                }
            }
        }
    }

    for track in tracks {
        process_track(track, expanded_tracks, 0, &mut rows, &mut current_row_index);
    }

    rows
}

// Backward compatibility
pub struct DisplayTrack<'a> {
    pub track: &'a Track,
    pub depth: usize,
    pub is_expanded: bool,
    pub visible_row_index: usize,
    pub is_folder: bool,
}

pub fn flatten_tracks<'a>(
    tracks: &'a [Track],
    expanded_tracks: &HashSet<Uuid>,
) -> Vec<DisplayTrack<'a>> {
    let mut display_tracks = Vec::new();
    let mut current_row_index = 0;

    fn recurse<'a>(
        tracks: &'a [Track],
        expanded_tracks: &HashSet<Uuid>,
        depth: usize,
        display_tracks: &mut Vec<DisplayTrack<'a>>,
        current_row_index: &mut usize,
    ) {
        for track in tracks {
            let is_expanded = expanded_tracks.contains(&track.id);
            let sub_tracks: Vec<&'a Track> = track
                .children
                .iter()
                .filter_map(|item| match item {
                    TrackItem::SubTrack(t) => Some(t),
                    _ => None,
                })
                .collect();
            let clips: Vec<&'a TrackClip> = track
                .children
                .iter()
                .filter_map(|item| match item {
                    TrackItem::Clip(c) => Some(c),
                    _ => None,
                })
                .collect();

            let is_folder = !sub_tracks.is_empty() || !clips.is_empty();

            display_tracks.push(DisplayTrack {
                track,
                depth,
                is_expanded,
                visible_row_index: *current_row_index,
                is_folder,
            });
            *current_row_index += 1;

            if is_expanded && !clips.is_empty() {
                for _clip in &clips {
                    *current_row_index += 1;
                }
            }

            if is_expanded && !sub_tracks.is_empty() {
                recurse_sub_tracks(
                    &sub_tracks,
                    expanded_tracks,
                    depth + 1,
                    display_tracks,
                    current_row_index,
                );
            }
        }
    }

    fn recurse_sub_tracks<'a>(
        sub_tracks: &[&'a Track],
        expanded_tracks: &HashSet<Uuid>,
        depth: usize,
        display_tracks: &mut Vec<DisplayTrack<'a>>,
        current_row_index: &mut usize,
    ) {
        for track in sub_tracks {
            let is_expanded = expanded_tracks.contains(&track.id);
            let child_sub_tracks: Vec<&'a Track> = track
                .children
                .iter()
                .filter_map(|item| match item {
                    TrackItem::SubTrack(t) => Some(t),
                    _ => None,
                })
                .collect();
            let clips: Vec<&'a TrackClip> = track
                .children
                .iter()
                .filter_map(|item| match item {
                    TrackItem::Clip(c) => Some(c),
                    _ => None,
                })
                .collect();

            let is_folder = !child_sub_tracks.is_empty() || !clips.is_empty();

            display_tracks.push(DisplayTrack {
                track,
                depth,
                is_expanded,
                visible_row_index: *current_row_index,
                is_folder,
            });
            *current_row_index += 1;

            if is_expanded && !clips.is_empty() {
                for _clip in &clips {
                    *current_row_index += 1;
                }
            }

            if is_expanded && !child_sub_tracks.is_empty() {
                recurse_sub_tracks(
                    &child_sub_tracks,
                    expanded_tracks,
                    depth + 1,
                    display_tracks,
                    current_row_index,
                );
            }
        }
    }

    recurse(
        tracks,
        expanded_tracks,
        0,
        &mut display_tracks,
        &mut current_row_index,
    );
    display_tracks
}
