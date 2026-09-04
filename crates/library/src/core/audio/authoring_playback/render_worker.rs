use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::thread::JoinHandle;

use crate::core::audio::authoring::{AuthoringAudioError, AuthoringAudioMixer};
use crate::core::cache::CacheManager;
use crate::model::authoring::{AuthoringProject, TimelineId};

pub(super) struct AudioRenderJob {
    pub generation: u64,
    pub project: Arc<AuthoringProject>,
    pub timeline_id: TimelineId,
    pub sample_rate: u32,
    pub channels: u16,
    pub start_frame: u64,
    pub frame_count: usize,
}

pub(super) struct AudioRenderResult {
    pub generation: u64,
    pub start_frame: u64,
    pub samples: Result<Vec<f32>, AuthoringAudioError>,
}

pub(super) enum AudioRenderWorkerPoll {
    Pending,
    Ready(AudioRenderResult),
    Disconnected,
}

/// One persistent decoder/mixer worker for Preview playback.
///
/// Media IO and decode never run on the UI thread. The worker accepts at most
/// one queued window in addition to the window it is currently rendering, so
/// rapid seeks cannot grow an unbounded backlog.
pub(super) struct AuthoringAudioRenderWorker {
    jobs: Option<SyncSender<AudioRenderJob>>,
    results: Receiver<AudioRenderResult>,
    thread: Option<JoinHandle<()>>,
}

impl AuthoringAudioRenderWorker {
    pub(super) fn new(cache: Arc<CacheManager>) -> Result<Self, std::io::Error> {
        let (job_sender, job_receiver) = mpsc::sync_channel::<AudioRenderJob>(1);
        let (result_sender, result_receiver) = mpsc::channel::<AudioRenderResult>();
        let thread = std::thread::Builder::new()
            .name("authoring-audio-render".to_string())
            .spawn(move || {
                while let Ok(job) = job_receiver.recv() {
                    let samples = AuthoringAudioMixer::new_with_format(
                        job.project.as_ref(),
                        cache.as_ref(),
                        job.timeline_id,
                        job.sample_rate,
                        job.channels,
                    )
                    .and_then(|mut mixer| mixer.render_window(job.start_frame, job.frame_count));
                    if result_sender
                        .send(AudioRenderResult {
                            generation: job.generation,
                            start_frame: job.start_frame,
                            samples,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            })?;
        Ok(Self {
            jobs: Some(job_sender),
            results: result_receiver,
            thread: Some(thread),
        })
    }

    pub(super) fn request(&self, job: AudioRenderJob) -> Result<bool, ()> {
        let Some(sender) = self.jobs.as_ref() else {
            return Err(());
        };
        match sender.try_send(job) {
            Ok(()) => Ok(true),
            Err(TrySendError::Full(_)) => Ok(false),
            Err(TrySendError::Disconnected(_)) => Err(()),
        }
    }

    pub(super) fn poll(&self) -> AudioRenderWorkerPoll {
        match self.results.try_recv() {
            Ok(result) => AudioRenderWorkerPoll::Ready(result),
            Err(TryRecvError::Empty) => AudioRenderWorkerPoll::Pending,
            Err(TryRecvError::Disconnected) => AudioRenderWorkerPoll::Disconnected,
        }
    }
}

impl Drop for AuthoringAudioRenderWorker {
    fn drop(&mut self) {
        self.jobs.take();
        if let Some(thread) = self.thread.take() {
            crate::util::thread::join_in_background(
                "audio-shutdown-reaper",
                vec![("Authoring audio render worker", thread)],
            );
        }
    }
}
