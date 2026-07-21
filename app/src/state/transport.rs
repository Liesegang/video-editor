use library::EditorService;

use crate::state::context::EditorContext;

trait AudioSeekTarget {
    fn reset_after_seek(&self, time: f64);
}

impl AudioSeekTarget for EditorService {
    fn reset_after_seek(&self, time: f64) {
        self.reset_audio_pump(time);
    }
}

/// Move the UI transport and audio master clock through one transaction.
///
/// Directly assigning `TimelineState::current_time` while playback is active
/// is ineffective: the next app frame samples the still-old audio clock and
/// overwrites it. Every interactive ruler/time-field seek goes through this
/// boundary so the Preview seek revision and audio pump adopt the same time.
pub(crate) fn seek_transport(
    editor_context: &mut EditorContext,
    project_service: &EditorService,
    time: f32,
) -> bool {
    seek_transport_with(editor_context, project_service, time)
}

fn seek_transport_with(
    editor_context: &mut EditorContext,
    audio: &impl AudioSeekTarget,
    time: f32,
) -> bool {
    if !time.is_finite() || time < 0.0 {
        return false;
    }
    editor_context.timeline.seek_to(time);
    audio.reset_after_seek(f64::from(time));
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::cache::CacheManager;
    use library::model::project::Project;
    use library::plugin::PluginManager;
    use std::cell::Cell;
    use std::sync::{Arc, RwLock};
    use uuid::Uuid;

    #[derive(Default)]
    struct RecordingAudioSeek {
        reset_time: Cell<Option<f64>>,
    }

    impl AudioSeekTarget for RecordingAudioSeek {
        fn reset_after_seek(&self, time: f64) {
            self.reset_time.set(Some(time));
        }
    }

    #[test]
    fn playing_seek_updates_timeline_revision_and_audio_clock_together() {
        let mut context = EditorContext::new(Uuid::new_v4());
        context.timeline.current_time = 1.0;
        context.timeline.is_playing = true;
        let revision = context.timeline.transport_seek_revision;
        let audio = RecordingAudioSeek::default();

        assert!(seek_transport_with(&mut context, &audio, 7.5));
        assert_eq!(context.timeline.current_time, 7.5);
        assert_eq!(
            context.timeline.transport_seek_revision,
            revision.wrapping_add(1)
        );
        assert_eq!(audio.reset_time.get(), Some(7.5));
    }

    #[test]
    fn playing_seek_moves_the_concrete_audio_master_clock_before_next_sample(
    ) -> Result<(), Box<dyn std::error::Error>> {
        let project = Arc::new(RwLock::new(Project::new("transport seek")));
        let service = EditorService::new(
            project,
            Arc::new(PluginManager::default()),
            Arc::new(CacheManager::new()),
        )?;
        let mut context = EditorContext::new(Uuid::new_v4());
        context.timeline.is_playing = true;
        service.get_audio_service().set_playing(true);
        service.reset_audio_pump(0.25);

        assert!(seek_transport(&mut context, &service, 2.0));
        let audio_time = service.get_audio_engine().get_current_time();
        assert!(audio_time >= 2.0, "audio clock remained at {audio_time}");

        // This mirrors the app's next playback sample. It must preserve the
        // seek instead of restoring the pre-seek 0.25 second clock.
        context.timeline.current_time = audio_time as f32;
        assert!(context.timeline.current_time >= 2.0);
        Ok(())
    }

    #[test]
    fn invalid_seek_does_not_split_timeline_and_audio_state() {
        let mut context = EditorContext::new(Uuid::new_v4());
        context.timeline.current_time = 1.0;
        let revision = context.timeline.transport_seek_revision;
        let audio = RecordingAudioSeek::default();

        assert!(!seek_transport_with(&mut context, &audio, f32::NAN));
        assert_eq!(context.timeline.current_time, 1.0);
        assert_eq!(context.timeline.transport_seek_revision, revision);
        assert_eq!(audio.reset_time.get(), None);
    }
}
