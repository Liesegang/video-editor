use library::model::project::project::Project;
use library::model::project::{Node, TrackClip, TrackData};
use std::collections::HashSet;
use uuid::Uuid;

/// Represents a single row in the timeline display
#[derive(Debug)]
pub enum DisplayRow<'a> {
    /// A track header row (always shown for each track)
    TrackHeader {
        track: &'a TrackData,
        depth: usize,
        is_expanded: bool,
        visible_row_index: usize,
        has_clips: bool,
        has_sub_tracks: bool,
    },
    /// A clip row (shown when parent track is expanded)
    ClipRow {
        clip: &'a TrackClip,
        parent_track: &'a TrackData,
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

/// Flatten tracks into display rows using the new Node-based structure
/// - Track header always shown
/// - When collapsed: clips are drawn on the track header row (handled by clips.rs)
/// - When expanded: each clip gets its own row below the header
pub fn flatten_tracks_to_rows<'a>(
    project: &'a Project,
    root_track_ids: &[Uuid],
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
        hide_header: bool,
    ) {
        let Some(track) = project.get_track(track_id) else {
            return;
        };

        // If header is hidden (root track), always treat as expanded to show children
        let is_expanded = if hide_header {
            true
        } else {
            expanded_tracks.contains(&track_id)
        };

        // Count clips and sub-tracks among children
        let mut has_clips = false;
        let mut has_sub_tracks = false;
        for child_id in &track.child_ids {
            match project.get_node(*child_id) {
                Some(Node::Clip(_)) => has_clips = true,
                Some(Node::Track(_)) => has_sub_tracks = true,
                None => {}
            }
        }

        if !hide_header {
            rows.push(DisplayRow::TrackHeader {
                track,
                depth,
                is_expanded,
                visible_row_index: *current_row_index,
                has_clips,
                has_sub_tracks,
            });
            *current_row_index += 1;
        }

        if is_expanded {
            // Iterate in reverse: later children render on top, so show them first
            for (child_index, child_id) in track.child_ids.iter().enumerate().rev() {
                match project.get_node(*child_id) {
                    Some(Node::Clip(clip)) => {
                        rows.push(DisplayRow::ClipRow {
                            clip,
                            parent_track: track,
                            depth: if hide_header { depth } else { depth + 1 },
                            visible_row_index: *current_row_index,
                            child_index,
                        });
                        *current_row_index += 1;
                    }
                    Some(Node::Track(sub_track)) => {
                        process_track(
                            project,
                            sub_track.id,
                            expanded_tracks,
                            if hide_header { depth } else { depth + 1 },
                            rows,
                            current_row_index,
                            false,
                        );
                    }
                    None => {}
                }
            }
        }
    }

    // Process tracks - later tracks in the list render on top
    for track_id in root_track_ids {
        process_track(
            project,
            *track_id,
            expanded_tracks,
            0,
            &mut rows,
            &mut current_row_index,
            true, // Hide root track header
        );
    }

    rows
}

// Backward compatibility - DisplayTrack for track list panel
pub struct DisplayTrack<'a> {
    pub track: &'a TrackData,
    pub depth: usize,
    pub is_expanded: bool,
    pub visible_row_index: usize,
    pub is_folder: bool,
}

/// Flatten tracks for the track list panel (sidebar)
pub fn flatten_tracks<'a>(
    project: &'a Project,
    root_track_ids: &[Uuid],
    expanded_tracks: &HashSet<Uuid>,
) -> Vec<DisplayTrack<'a>> {
    let mut display_tracks = Vec::new();
    let mut current_row_index = 0;

    fn recurse<'a>(
        project: &'a Project,
        track_id: Uuid,
        expanded_tracks: &HashSet<Uuid>,
        depth: usize,
        display_tracks: &mut Vec<DisplayTrack<'a>>,
        current_row_index: &mut usize,
    ) {
        let Some(track) = project.get_track(track_id) else {
            return;
        };

        let is_expanded = expanded_tracks.contains(&track_id);

        // Check for children
        let mut clip_count = 0;
        let mut sub_track_ids = Vec::new();
        for child_id in &track.child_ids {
            match project.get_node(*child_id) {
                Some(Node::Clip(_)) => clip_count += 1,
                Some(Node::Track(_)) => sub_track_ids.push(*child_id),
                None => {}
            }
        }

        let is_folder = !sub_track_ids.is_empty() || clip_count > 0;

        display_tracks.push(DisplayTrack {
            track,
            depth,
            is_expanded,
            visible_row_index: *current_row_index,
            is_folder,
        });
        *current_row_index += 1;

        // Add rows for expanded clips
        if is_expanded && clip_count > 0 {
            *current_row_index += clip_count;
        }

        // Recurse into sub-tracks
        if is_expanded {
            for sub_id in sub_track_ids {
                recurse(
                    project,
                    sub_id,
                    expanded_tracks,
                    depth + 1,
                    display_tracks,
                    current_row_index,
                );
            }
        }
    }

    for track_id in root_track_ids {
        recurse(
            project,
            *track_id,
            expanded_tracks,
            0,
            &mut display_tracks,
            &mut current_row_index,
        );
    }

    display_tracks
}
