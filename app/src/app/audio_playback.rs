//! Application lifecycle around the library's authoring-native audio output.

use std::sync::Arc;

use library::audio::authoring_playback::{AuthoringAudioPlayback, AuthoringAudioPlaybackStatus};
use library::cache::CacheManager;
use library::model::authoring::{AuthoringProject, ProjectRevision, TimelineId};

pub(super) enum PlaybackClock {
    Wall,
    AwaitingAudio,
    Audio(f64),
}

pub(super) struct TimelineAudioRuntime {
    playback: Option<AuthoringAudioPlayback>,
    status: Option<AuthoringAudioPlaybackStatus>,
    startup_error: Option<String>,
}

impl TimelineAudioRuntime {
    pub(super) fn new(cache: Arc<CacheManager>) -> Self {
        match AuthoringAudioPlayback::new(cache) {
            Ok(playback) => Self {
                playback: Some(playback),
                status: None,
                startup_error: None,
            },
            Err(error) => {
                log::warn!("Timeline audio output is unavailable: {error}");
                Self {
                    playback: None,
                    status: None,
                    startup_error: Some(error.to_string()),
                }
            }
        }
    }

    pub(super) fn synchronize(
        &mut self,
        project: Arc<AuthoringProject>,
        revision: ProjectRevision,
        timeline_id: TimelineId,
        transport_time: f64,
        playing: bool,
    ) -> Result<PlaybackClock, String> {
        let Some(playback) = self.playback.as_mut() else {
            return Ok(PlaybackClock::Wall);
        };
        let status = playback
            .synchronize(project, revision, timeline_id, transport_time, playing)
            .map_err(|error| error.to_string())?;
        self.status = Some(status);
        if !playing || !status.has_audio_routes {
            Ok(PlaybackClock::Wall)
        } else if status.output_active {
            Ok(PlaybackClock::Audio(status.current_time))
        } else {
            Ok(PlaybackClock::AwaitingAudio)
        }
    }

    pub(super) fn stop(&mut self) -> Result<(), String> {
        if let Some(playback) = self.playback.as_mut() {
            playback.stop().map_err(|error| error.to_string())?;
        }
        self.status = None;
        Ok(())
    }

    pub(super) fn status(&self) -> Option<AuthoringAudioPlaybackStatus> {
        self.status
    }

    pub(super) fn startup_error(&self) -> Option<&str> {
        self.startup_error.as_deref()
    }
}
