use library::model::project::Track;
use std::collections::HashSet;
use uuid::Uuid;

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
            let is_folder = !track.children.is_empty();

            display_tracks.push(DisplayTrack {
                track,
                depth,
                is_expanded,
                visible_row_index: *current_row_index,
                is_folder,
            });

            *current_row_index += 1;

            if is_expanded && is_folder {
                recurse(
                    &track.children,
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
