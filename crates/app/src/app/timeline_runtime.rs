use library::model::authoring::{AuthoringProject, Timeline};

use crate::state::authoring::{AuthoringSelection, AuthoringTimelineView, AuthoringUiState};

pub(super) fn timeline_end_frame(timeline: &Timeline) -> i64 {
    (timeline.duration.to_seconds_f64() * timeline.fps.to_f64())
        .ceil()
        .clamp(0.0, i64::MAX as f64) as i64
}

pub(super) fn finish_playback_at_timeline_end(
    timeline_view: &mut AuthoringTimelineView,
    timeline: &Timeline,
) {
    let end_frame = timeline_end_frame(timeline);
    if timeline_view.current_frame >= end_frame {
        timeline_view.seek_frame(end_frame.saturating_sub(1));
        timeline_view.set_playing(false);
    }
}

pub(super) fn initialize_timeline_view(project: &AuthoringProject, state: &mut AuthoringUiState) {
    if let Some(timeline) = project.timelines.get(&state.active_timeline_id) {
        state
            .timeline
            .expanded_tracks
            .extend(timeline.track_order.iter().copied());
    }
    if let Some(item_id) = project
        .items
        .values()
        .filter(|item| {
            project
                .tracks
                .get(&item.track_id)
                .is_some_and(|track| track.timeline_id == state.active_timeline_id)
        })
        .min_by_key(|item| (item.interval.start, item.layer, item.id))
        .map(|item| item.id)
    {
        state.selection.replace(AuthoringSelection::Item(item_id));
    }
}
