//! Authoring Timeline audio materialization for video export.

use std::fs::{File, OpenOptions};
use std::io::{self, BufWriter, Write};
#[cfg(test)]
use std::path::Path;
use std::path::PathBuf;
#[cfg(test)]
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use crate::cache::SharedCacheManager;
use crate::core::audio::authoring::{
    AUTHORING_AUDIO_CHANNELS, AUTHORING_AUDIO_SAMPLE_RATE, AuthoringAudioMixer,
    MAX_AUTHORING_AUDIO_WINDOW_FRAMES,
};
use crate::error::LibraryError;
use crate::model::authoring::{AuthoringProject, TimelineId};
use crate::plugin::ExportSettings;

#[cfg(test)]
#[derive(Default)]
pub(in crate::core::rendering::render_server) struct TemporaryAudioTestControl {
    state: Mutex<TemporaryAudioTestState>,
}

#[cfg(test)]
#[derive(Default)]
struct TemporaryAudioTestState {
    created_paths: Vec<PathBuf>,
    explicit_cleanup_attempts: usize,
    drop_cleanup_attempts: usize,
    injected_cleanup_failures: usize,
    cleanup_failures_remaining: usize,
}

#[derive(Clone, Copy)]
enum CleanupOrigin {
    Explicit,
    Drop,
}

#[cfg(test)]
impl TemporaryAudioTestControl {
    fn test_state(&self) -> MutexGuard<'_, TemporaryAudioTestState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn record_creation(&self, path: &Path) {
        self.test_state().created_paths.push(path.to_path_buf());
    }

    fn before_remove(&self, path: &Path, origin: CleanupOrigin) -> io::Result<()> {
        let should_fail = {
            let mut state = self.test_state();
            match origin {
                CleanupOrigin::Explicit => state.explicit_cleanup_attempts += 1,
                CleanupOrigin::Drop => state.drop_cleanup_attempts += 1,
            }
            if state.cleanup_failures_remaining > 0 {
                state.cleanup_failures_remaining -= 1;
                state.injected_cleanup_failures += 1;
                true
            } else {
                false
            }
        };
        if should_fail {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "injected temporary authoring audio cleanup failure for '{}'",
                    path.display()
                ),
            ));
        }
        Ok(())
    }

    pub(in crate::core::rendering::render_server) fn fail_cleanup_attempts(
        &self,
        attempts: usize,
    ) -> Result<(), LibraryError> {
        if attempts == 0 {
            return Err(LibraryError::Validation(
                "temporary audio cleanup failure count must be positive".to_string(),
            ));
        }
        let mut state = self.test_state();
        if state.cleanup_failures_remaining > 0 {
            return Err(LibraryError::Runtime(
                "a temporary authoring audio cleanup failure is already armed".to_string(),
            ));
        }
        state.cleanup_failures_remaining = attempts;
        Ok(())
    }

    pub(in crate::core::rendering::render_server) fn fail_explicit_cleanup(
        &self,
    ) -> Result<(), LibraryError> {
        self.fail_cleanup_attempts(TEMPORARY_AUDIO_CLEANUP_ATTEMPTS)
    }

    pub(in crate::core::rendering::render_server) fn observation(
        &self,
    ) -> (Vec<PathBuf>, usize, usize, usize) {
        let state = self.test_state();
        (
            state.created_paths.clone(),
            state.explicit_cleanup_attempts,
            state.drop_cleanup_attempts,
            state.injected_cleanup_failures,
        )
    }
}

const TEMPORARY_AUDIO_CLEANUP_ATTEMPTS: usize = 4;
const TEMPORARY_AUDIO_CLEANUP_RETRY_DELAY: Duration = Duration::from_millis(5);

fn cleanup_error_is_retryable(error: &io::Error) -> bool {
    matches!(
        error.kind(),
        io::ErrorKind::Interrupted | io::ErrorKind::PermissionDenied | io::ErrorKind::WouldBlock
    )
}

fn remove_with_retry(mut remove: impl FnMut() -> io::Result<()>) -> io::Result<()> {
    let mut attempt = 0;
    loop {
        attempt += 1;
        match remove() {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error)
                if attempt < TEMPORARY_AUDIO_CLEANUP_ATTEMPTS
                    && cleanup_error_is_retryable(&error) =>
            {
                std::thread::sleep(TEMPORARY_AUDIO_CLEANUP_RETRY_DELAY);
            }
            Err(error) => return Err(error),
        }
    }
}

pub(super) struct TemporaryAuthoringAudio {
    path: Option<PathBuf>,
    writer: Option<BufWriter<File>>,
    #[cfg(test)]
    test_control: Arc<TemporaryAudioTestControl>,
}

impl TemporaryAuthoringAudio {
    fn create(
        #[cfg(test)] test_control: Arc<TemporaryAudioTestControl>,
    ) -> Result<Self, LibraryError> {
        let path = std::env::temp_dir().join(format!(
            "ruvie-authoring-audio-{}.f32le",
            uuid::Uuid::new_v4()
        ));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|error| {
                LibraryError::Render(format!(
                    "cannot create temporary authoring audio '{}': {error}",
                    path.display()
                ))
            })?;
        #[cfg(test)]
        test_control.record_creation(&path);
        Ok(Self {
            path: Some(path),
            writer: Some(BufWriter::new(file)),
            #[cfg(test)]
            test_control,
        })
    }

    fn write_samples(&mut self, samples: &[f32]) -> Result<(), LibraryError> {
        let writer = self.writer.as_mut().ok_or_else(|| {
            LibraryError::Render("temporary authoring audio is already closed".to_string())
        })?;
        for sample in samples {
            writer.write_all(&sample.to_le_bytes()).map_err(|error| {
                LibraryError::Render(format!("cannot write temporary authoring audio: {error}"))
            })?;
        }
        Ok(())
    }

    fn finish_writing(&mut self) -> Result<String, LibraryError> {
        if let Some(mut writer) = self.writer.take() {
            writer.flush().map_err(|error| {
                LibraryError::Render(format!("cannot flush temporary authoring audio: {error}"))
            })?;
        }
        let path = self.path.as_ref().ok_or_else(|| {
            LibraryError::Render("temporary authoring audio path is unavailable".to_string())
        })?;
        path.to_str().map(str::to_owned).ok_or_else(|| {
            LibraryError::Render(format!(
                "temporary authoring audio path is not valid UTF-8: {}",
                path.display()
            ))
        })
    }

    pub(super) fn cleanup(&mut self) -> Result<(), LibraryError> {
        self.writer.take();
        let removal = self.remove_owned_path(CleanupOrigin::Explicit);
        removal.map_err(|error| {
            let path = self.path.as_deref().map_or_else(
                || "<removed>".to_string(),
                |path| path.display().to_string(),
            );
            LibraryError::Render(format!(
                "cannot remove temporary authoring audio '{}': {error}",
                path
            ))
        })
    }

    fn remove_owned_path(&mut self, origin: CleanupOrigin) -> io::Result<()> {
        let Some(path) = self.path.as_ref() else {
            return Ok(());
        };
        let removal = remove_with_retry(|| {
            #[cfg(test)]
            self.test_control.before_remove(path, origin)?;
            let _ = origin;
            std::fs::remove_file(path)
        });
        if removal.is_ok() {
            self.path = None;
        }
        removal
    }
}

impl Drop for TemporaryAuthoringAudio {
    fn drop(&mut self) {
        self.writer.take();
        let removal = self.remove_owned_path(CleanupOrigin::Drop);
        if let Err(error) = removal {
            let path = self.path.as_deref().map_or_else(
                || "<removed>".to_string(),
                |path| path.display().to_string(),
            );
            log::error!(
                "failed to remove temporary authoring audio '{}': {error}",
                path
            );
        }
    }
}

fn authoring_audio_frame_count(
    project: &AuthoringProject,
    timeline_id: TimelineId,
) -> Result<u64, LibraryError> {
    let duration = project
        .timelines
        .get(&timeline_id)
        .ok_or_else(|| LibraryError::Render("export Timeline is missing".to_string()))?
        .duration;
    if duration.is_negative() {
        return Err(LibraryError::Validation(
            "export Timeline duration must be non-negative".to_string(),
        ));
    }
    let numerator = i128::from(duration.value())
        .checked_mul(i128::from(AUTHORING_AUDIO_SAMPLE_RATE))
        .ok_or_else(|| LibraryError::Render("export audio frame count overflowed".to_string()))?;
    let denominator = i128::from(duration.timescale());
    let frames = if numerator == 0 {
        0
    } else {
        numerator.checked_add(denominator - 1).ok_or_else(|| {
            LibraryError::Render("export audio frame count overflowed".to_string())
        })? / denominator
    };
    u64::try_from(frames)
        .map_err(|_| LibraryError::Render("export audio frame count exceeds u64".to_string()))
}

pub(super) fn prepare_authoring_audio(
    project: &AuthoringProject,
    timeline_id: TimelineId,
    cache_manager: &SharedCacheManager,
    settings: &mut ExportSettings,
    owner: &mut Option<TemporaryAuthoringAudio>,
    #[cfg(test)] test_control: Arc<TemporaryAudioTestControl>,
) -> Result<(), LibraryError> {
    let mut mixer = AuthoringAudioMixer::new(project, cache_manager.as_ref(), timeline_id)
        .map_err(|error| {
            LibraryError::Render(format!("authoring audio schedule failed: {error}"))
        })?;
    if !mixer.unsupported_video_assets().is_empty() {
        log::warn!(
            "Authoring export does not infer embedded audio for {} Video Asset(s); separately imported Audio Assets remain authoritative",
            mixer.unsupported_video_assets().len()
        );
    }
    if !mixer.has_audio_routes() {
        return Ok(());
    }

    let frame_count = authoring_audio_frame_count(project, timeline_id)?;
    let temporary = TemporaryAuthoringAudio::create(
        #[cfg(test)]
        test_control,
    )?;
    *owner = Some(temporary);
    let temporary = owner.as_mut().ok_or_else(|| {
        LibraryError::Runtime("temporary authoring audio owner was not installed".to_string())
    })?;
    let mut start_frame = 0_u64;
    while start_frame < frame_count {
        let remaining = frame_count - start_frame;
        let window_frames = usize::try_from(
            remaining.min(MAX_AUTHORING_AUDIO_WINDOW_FRAMES as u64),
        )
        .map_err(|_| LibraryError::Render("authoring audio window exceeds usize".to_string()))?;
        let samples = mixer
            .render_window(start_frame, window_frames)
            .map_err(|error| {
                LibraryError::Render(format!("authoring audio render failed: {error}"))
            })?;
        temporary.write_samples(&samples)?;
        start_frame = start_frame
            .checked_add(window_frames as u64)
            .ok_or_else(|| LibraryError::Render("authoring audio range overflowed".to_string()))?;
    }
    let path = temporary.finish_writing()?;
    settings.bind_runtime_audio_source(path, AUTHORING_AUDIO_CHANNELS, AUTHORING_AUDIO_SAMPLE_RATE)
}
