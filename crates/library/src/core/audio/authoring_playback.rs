//! Realtime output for the authoritative Timeline audio schedule.
//!
//! The Timeline remains the source of truth. This runtime only derives and
//! queues short device-format windows from [`AuthoringAudioMixer`]; it never
//! creates an editable audio graph or a second playback model.

use std::sync::Arc;

use thiserror::Error;

use super::authoring::{AuthoringAudioError, AuthoringAudioMixer};
use super::engine::AudioEngine;
use crate::core::cache::CacheManager;
use crate::model::authoring::{AuthoringProject, ProjectRevision, TimelineId};

mod render_worker;

use render_worker::{AudioRenderJob, AudioRenderWorkerPoll, AuthoringAudioRenderWorker};

const PUMP_WINDOW_DIVISOR: u32 = 4;
const RUNNING_REFILL_DIVISOR: u32 = 10;
const MIN_SEEK_DISCONTINUITY_SECONDS: f64 = 0.05;

#[derive(Debug, Error)]
pub enum AuthoringAudioPlaybackError {
    #[error("cannot initialize the audio output device: {0}")]
    Output(#[source] anyhow::Error),
    #[error("audio render worker failed: {0}")]
    Worker(String),
    #[error(transparent)]
    Render(#[from] AuthoringAudioError),
}

/// Observable state for transport synchronization and native UI QA.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AuthoringAudioPlaybackStatus {
    pub has_audio_routes: bool,
    pub output_active: bool,
    pub current_time: f64,
    pub queued_frames: u64,
    pub underrun_callbacks: u64,
    pub rendered_non_silent_frames: u64,
    pub sample_rate: u32,
    pub channels: u16,
}

/// Owns the one system output stream used by Timeline Preview playback.
pub struct AuthoringAudioPlayback {
    engine: AudioEngine,
    cache: Arc<CacheManager>,
    worker: AuthoringAudioRenderWorker,
    project: Option<Arc<AuthoringProject>>,
    timeline_id: Option<TimelineId>,
    revision: Option<ProjectRevision>,
    requested_playing: bool,
    has_audio_routes: bool,
    output_active: bool,
    producer_frame: u64,
    requested_time: f64,
    render_generation: u64,
    render_pending: bool,
}

impl AuthoringAudioPlayback {
    pub fn new(cache: Arc<CacheManager>) -> Result<Self, AuthoringAudioPlaybackError> {
        let engine = AudioEngine::new().map_err(AuthoringAudioPlaybackError::Output)?;
        let worker = AuthoringAudioRenderWorker::new(Arc::clone(&cache))
            .map_err(|error| AuthoringAudioPlaybackError::Worker(error.to_string()))?;
        Ok(Self {
            engine,
            cache,
            worker,
            project: None,
            timeline_id: None,
            revision: None,
            requested_playing: false,
            has_audio_routes: false,
            output_active: false,
            producer_frame: 0,
            requested_time: 0.0,
            render_generation: 0,
            render_pending: false,
        })
    }

    /// Reconcile one UI transport frame with the system output stream.
    ///
    /// `transport_time` is authoritative when stopped or after a Project /
    /// Timeline change. While output is active, callers use the returned
    /// device clock as the visual playhead clock.
    pub fn synchronize(
        &mut self,
        project: Arc<AuthoringProject>,
        revision: ProjectRevision,
        timeline_id: TimelineId,
        transport_time: f64,
        playing: bool,
    ) -> Result<AuthoringAudioPlaybackStatus, AuthoringAudioPlaybackError> {
        let transport_time = finite_nonnegative(transport_time);
        let identity_changed =
            self.timeline_id != Some(timeline_id) || self.revision != Some(revision);
        let seek_requested = playing
            && self.requested_playing
            && !identity_changed
            && self.transport_discontinuity(&project, timeline_id, transport_time);

        if !playing {
            if self.requested_playing || self.output_active {
                self.engine
                    .pause()
                    .map_err(AuthoringAudioPlaybackError::Output)?;
                self.engine.flush();
                self.cancel_pending_render();
            }
            self.timeline_id = Some(timeline_id);
            self.revision = Some(revision);
            self.project = Some(project);
            self.requested_playing = false;
            self.output_active = false;
            self.requested_time = transport_time;
            return Ok(self.status_at(transport_time));
        }

        if !self.requested_playing || identity_changed || seek_requested {
            self.prepare(project, revision, timeline_id, transport_time)?;
        }
        if self.has_audio_routes {
            self.pump()?;
        }

        let current_time = if self.output_active {
            self.engine.get_current_time()
        } else {
            self.requested_time
        };
        Ok(self.status_at(current_time))
    }

    pub fn stop(&mut self) -> Result<(), AuthoringAudioPlaybackError> {
        self.engine
            .pause()
            .map_err(AuthoringAudioPlaybackError::Output)?;
        self.engine.flush();
        self.requested_playing = false;
        self.output_active = false;
        self.has_audio_routes = false;
        self.timeline_id = None;
        self.revision = None;
        self.project = None;
        self.cancel_pending_render();
        Ok(())
    }

    fn prepare(
        &mut self,
        project: Arc<AuthoringProject>,
        revision: ProjectRevision,
        timeline_id: TimelineId,
        transport_time: f64,
    ) -> Result<(), AuthoringAudioPlaybackError> {
        self.engine
            .pause()
            .map_err(AuthoringAudioPlaybackError::Output)?;
        let sample_rate = self.engine.get_sample_rate();
        let channels = self.engine.get_channels();
        self.cancel_pending_render();
        self.producer_frame = seconds_to_frame(transport_time, sample_rate);
        // Publish the new generation before any schedule traversal. The
        // always-running device callback then discards old queued audio while
        // this potentially non-trivial preparation runs.
        self.engine.set_time(transport_time);
        let mixer = AuthoringAudioMixer::new_with_format(
            project.as_ref(),
            self.cache.as_ref(),
            timeline_id,
            sample_rate,
            channels,
        )?;
        let has_audio_routes = mixer.has_audio_routes();
        drop(mixer);
        self.timeline_id = Some(timeline_id);
        self.revision = Some(revision);
        self.project = Some(project);
        self.requested_playing = true;
        self.has_audio_routes = has_audio_routes;
        self.output_active = false;
        self.requested_time = transport_time;
        Ok(())
    }

    fn pump(&mut self) -> Result<(), AuthoringAudioPlaybackError> {
        self.accept_ready_window()?;
        if self.engine.flush_pending() {
            return Ok(());
        }
        if self.render_pending {
            return Ok(());
        }
        let Some(project) = self.project.as_ref().cloned() else {
            return Ok(());
        };
        let Some(timeline_id) = self.timeline_id else {
            return Ok(());
        };
        let sample_rate = self.engine.get_sample_rate();
        let channels = self.engine.get_channels();
        let channel_count = usize::from(channels);
        let available_frames = self.engine.available_slots() / channel_count;
        let running_refill =
            usize::try_from((sample_rate / RUNNING_REFILL_DIVISOR).max(1)).unwrap_or(usize::MAX);
        if self.output_active && available_frames < running_refill {
            return Ok(());
        }
        let pump_limit =
            usize::try_from((sample_rate / PUMP_WINDOW_DIVISOR).max(1)).unwrap_or(usize::MAX);
        let frame_count = available_frames.min(pump_limit);
        if frame_count == 0 {
            return Ok(());
        }

        match self.worker.request(AudioRenderJob {
            generation: self.render_generation,
            project,
            timeline_id,
            sample_rate,
            channels,
            start_frame: self.producer_frame,
            frame_count,
        }) {
            Ok(queued) => self.render_pending = queued,
            Err(()) => {
                return Err(AuthoringAudioPlaybackError::Worker(
                    "request channel disconnected".to_string(),
                ));
            }
        }
        Ok(())
    }

    fn accept_ready_window(&mut self) -> Result<(), AuthoringAudioPlaybackError> {
        loop {
            let result = match self.worker.poll() {
                AudioRenderWorkerPoll::Pending => break,
                AudioRenderWorkerPoll::Ready(result) => result,
                AudioRenderWorkerPoll::Disconnected => {
                    return Err(AuthoringAudioPlaybackError::Worker(
                        "result channel disconnected".to_string(),
                    ));
                }
            };
            if result.generation != self.render_generation {
                continue;
            }
            self.render_pending = false;
            if result.start_frame != self.producer_frame {
                continue;
            }
            let samples = result.samples?;
            let Some(output_generation) = self.engine.output_generation() else {
                return Ok(());
            };
            let channel_count = usize::from(self.engine.get_channels());
            self.engine
                .play()
                .map_err(AuthoringAudioPlaybackError::Output)?;
            let written_samples = self.engine.push_samples(&samples, output_generation);
            let written_frames = written_samples / channel_count;
            self.producer_frame = self.producer_frame.saturating_add(written_frames as u64);
            if written_frames > 0 && !self.output_active {
                self.output_active = true;
            } else if written_frames == 0 && !self.output_active {
                self.engine
                    .pause()
                    .map_err(AuthoringAudioPlaybackError::Output)?;
            }
        }
        Ok(())
    }

    fn cancel_pending_render(&mut self) {
        self.render_generation = self.render_generation.wrapping_add(1);
        self.render_pending = false;
    }

    fn transport_discontinuity(
        &self,
        project: &AuthoringProject,
        timeline_id: TimelineId,
        transport_time: f64,
    ) -> bool {
        if !self.output_active {
            return false;
        }
        let fps = project
            .timelines
            .get(&timeline_id)
            .map(|timeline| timeline.fps.to_f64())
            .unwrap_or(0.0);
        is_transport_discontinuity(transport_time, self.engine.get_current_time(), fps)
    }

    fn status_at(&self, current_time: f64) -> AuthoringAudioPlaybackStatus {
        AuthoringAudioPlaybackStatus {
            has_audio_routes: self.has_audio_routes,
            output_active: self.output_active,
            current_time: finite_nonnegative(current_time),
            queued_frames: self.engine.queued_frames(),
            underrun_callbacks: self.engine.underrun_callbacks(),
            rendered_non_silent_frames: self.engine.non_silent_output_frames(),
            sample_rate: self.engine.get_sample_rate(),
            channels: self.engine.get_channels(),
        }
    }
}

fn finite_nonnegative(value: f64) -> f64 {
    if value.is_finite() && value >= 0.0 {
        value
    } else {
        0.0
    }
}

fn seconds_to_frame(seconds: f64, sample_rate: u32) -> u64 {
    (finite_nonnegative(seconds) * f64::from(sample_rate))
        .round()
        .clamp(0.0, u64::MAX as f64) as u64
}

fn is_transport_discontinuity(transport_time: f64, output_time: f64, fps: f64) -> bool {
    let two_frame_tolerance = if fps.is_finite() && fps > 0.0 {
        2.0 / fps
    } else {
        0.0
    };
    let tolerance = two_frame_tolerance.max(MIN_SEEK_DISCONTINUITY_SECONDS);
    (finite_nonnegative(transport_time) - finite_nonnegative(output_time)).abs() > tolerance
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_transport_times_are_bounded_before_sample_conversion() {
        assert_eq!(finite_nonnegative(f64::NAN), 0.0);
        assert_eq!(finite_nonnegative(-1.0), 0.0);
        assert_eq!(seconds_to_frame(0.5, 48_000), 24_000);
    }

    #[test]
    fn normal_ui_frame_lag_is_not_a_seek_but_a_transport_jump_is() {
        assert!(!is_transport_discontinuity(1.0, 1.05, 30.0));
        assert!(is_transport_discontinuity(4.0, 1.05, 30.0));
        assert!(!is_transport_discontinuity(1.0, 1.04, 0.0));
    }
}
