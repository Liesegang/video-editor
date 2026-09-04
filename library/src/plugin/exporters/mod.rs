mod export_frame;
mod ffmpeg_command;
mod ffmpeg_destination;
pub mod ffmpeg_export;
pub mod png_export;

pub use self::export_frame::{ExportColorAuthority, ExportFrame};
pub use self::ffmpeg_export::FfmpegExportPlugin;
pub use self::png_export::PngExportPlugin;

use crate::error::LibraryError;
use crate::model::authoring::{AuthoringProject, Timeline};
use crate::model::project::Composition;
use crate::model::project::{ExportConfig, Project};
use crate::model::property::PropertyDefinition;
use crate::plugin::{Plugin, PluginCategory};
use serde_json::Value;
use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use uuid::Uuid;

pub trait ExportPlugin: Plugin {
    /// Write one frame for `settings.job_id()`. Stateful implementations must
    /// reject another job that resolves to the same destination instead of
    /// sharing a writer or relying on destructive encoder overwrite flags.
    fn export_frame(
        &self,
        path: &str,
        frame: &ExportFrame,
        settings: &ExportSettings,
    ) -> Result<(), LibraryError>;

    /// Release all path-scoped exporter resources after the save queue has
    /// drained. `ExportService` calls this exactly once for every distinct
    /// output path whose `export_frame` call was attempted, including paths
    /// whose write failed. `settings.job_id()` identifies the owning job, so a
    /// stale caller must never finalize a newer job's destination. This is not
    /// called for paths that were only planned but never attempted.
    fn finish_export(&self, _path: &str, _settings: &ExportSettings) -> Result<(), LibraryError> {
        Ok(())
    }

    fn properties(&self) -> Vec<PropertyDefinition> {
        Vec::new()
    }

    fn plugin_type(&self) -> PluginCategory {
        PluginCategory::Export
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Png,
    Video,
}

/// Unique identity of one export service lifetime.
///
/// Stateful exporters use this together with the exact destination path so
/// concurrent callers cannot share or finalize one another's sessions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ExportJobId(Uuid);

impl ExportJobId {
    fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl std::fmt::Display for ExportJobId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

#[derive(Debug, Clone)]
pub struct ExportSettings {
    pub container: String,
    pub codec: String,
    pub pixel_format: String,
    trusted_ffmpeg_path: Option<String>,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub parameters: HashMap<String, Value>,
    color_authority: Option<ExportColorAuthority>,
    job_id: ExportJobId,
    runtime_audio: Option<RuntimeAudioSource>,
}

#[derive(Debug, Clone)]
struct RuntimeAudioSource {
    path: String,
    channels: u16,
    sample_rate: u32,
}

impl ExportSettings {
    pub fn from_project(
        project: &Project,
        composition: &Composition,
    ) -> Result<Self, LibraryError> {
        let mut settings = ExportSettings::for_dimensions(
            composition.width as u32,
            composition.height as u32,
            composition.fps,
        );
        settings.bind_project_color_authority(project)?;
        settings.apply_document_config(&project.export);
        Ok(settings)
    }

    /// Build encoder settings directly from a Timeline-first Project and one
    /// selected Timeline. No legacy Project or Composition is manufactured at
    /// this boundary.
    pub fn from_authoring_project(
        project: &AuthoringProject,
        timeline: &Timeline,
    ) -> Result<Self, LibraryError> {
        let width = u32::try_from(timeline.width).map_err(|_| {
            LibraryError::Render(format!(
                "Timeline width {} exceeds the encoder limit",
                timeline.width
            ))
        })?;
        let height = u32::try_from(timeline.height).map_err(|_| {
            LibraryError::Render(format!(
                "Timeline height {} exceeds the encoder limit",
                timeline.height
            ))
        })?;
        let mut settings = ExportSettings::for_dimensions(width, height, timeline.fps.to_f64());
        settings.bind_authoring_project_color_authority(project)?;
        settings.apply_document_config(&project.export);
        Ok(settings)
    }

    fn apply_document_config(&mut self, config: &ExportConfig) {
        if config.container.is_none()
            && config.codec.is_none()
            && config.pixel_format.is_none()
            && config.parameters.is_empty()
        {
            return;
        }

        if let Some(value) = &config.container {
            self.container = value.clone();
        }
        if let Some(value) = &config.codec {
            self.codec = value.clone();
        }
        if let Some(value) = &config.pixel_format {
            self.pixel_format = value.clone();
        }
        self.parameters = config.parameters.clone();
        for runtime_key in ["audio_source", "audio_channels", "audio_sample_rate"] {
            self.parameters.remove(runtime_key);
        }

        if matches!(self.export_format(), ExportFormat::Video) {
            if self.codec == "png" {
                self.codec = "libx264".into();
            }
            if self.pixel_format == "rgba" {
                self.pixel_format = "yuv420p".into();
            }
        }
    }

    pub fn for_dimensions(width: u32, height: u32, fps: f64) -> Self {
        Self {
            container: "png".into(),
            codec: "png".into(),
            pixel_format: "rgba".into(),
            trusted_ffmpeg_path: None,
            width,
            height,
            fps,
            parameters: HashMap::new(),
            color_authority: None,
            job_id: ExportJobId::new(),
            runtime_audio: None,
        }
    }

    /// Bind this encoder configuration to the exact output semantics of a
    /// validated Project. Settings created only for dimensions remain
    /// intentionally unusable for export until this succeeds.
    pub fn bind_project_color_authority(&mut self, project: &Project) -> Result<(), LibraryError> {
        self.color_authority = Some(ExportColorAuthority::from_project(project)?);
        Ok(())
    }

    /// Bind settings to a Timeline-first Project's exact export pipeline.
    pub fn bind_authoring_project_color_authority(
        &mut self,
        project: &AuthoringProject,
    ) -> Result<(), LibraryError> {
        self.color_authority = Some(ExportColorAuthority::from_authoring_project(project)?);
        Ok(())
    }

    pub fn color_authority(&self) -> Option<&ExportColorAuthority> {
        self.color_authority.as_ref()
    }

    pub const fn job_id(&self) -> ExportJobId {
        self.job_id
    }

    pub(crate) fn begin_new_job(&mut self) {
        self.job_id = ExportJobId::new();
    }

    /// Select an executable only from trusted application configuration.
    /// Project documents are intentionally unable to populate this field.
    pub fn set_trusted_ffmpeg_path(&mut self, path: Option<String>) {
        self.trusted_ffmpeg_path = path;
    }

    pub(crate) fn trusted_ffmpeg_path(&self) -> Option<&str> {
        self.trusted_ffmpeg_path.as_deref()
    }

    /// Bind a runtime-created raw audio file. Project JSON cannot populate
    /// this private channel or inject a local path into FFmpeg.
    pub fn bind_runtime_audio_source(
        &mut self,
        path: String,
        channels: u16,
        sample_rate: u32,
    ) -> Result<(), LibraryError> {
        if path.is_empty() || channels == 0 || sample_rate == 0 {
            return Err(LibraryError::Render(
                "runtime audio source requires a path, channels, and sample rate".to_string(),
            ));
        }
        self.runtime_audio = Some(RuntimeAudioSource {
            path,
            channels,
            sample_rate,
        });
        Ok(())
    }

    /// Runtime-only audio input exposed to exporter plugins after the host has
    /// created it. This cannot be hydrated from Project serialization.
    pub fn runtime_audio_source(&self) -> Option<(&str, u16, u32)> {
        self.runtime_audio
            .as_ref()
            .map(|audio| (audio.path.as_str(), audio.channels, audio.sample_rate))
    }

    pub(crate) fn require_matching_color_authority<'a>(
        &self,
        frame: &'a ExportFrame,
    ) -> Result<&'a ExportColorAuthority, LibraryError> {
        let image = frame.image();
        if (image.width, image.height) != (self.width, self.height) {
            return Err(LibraryError::Render(format!(
                "export frame is {}x{}, but settings require {}x{}; implicit resizing is forbidden",
                image.width, image.height, self.width, self.height
            )));
        }
        let expected = self.color_authority.as_ref().ok_or_else(|| {
            LibraryError::Render(
                "export settings have no Project-derived color authority; use ExportSettings::from_project or bind_project_color_authority"
                    .to_string(),
            )
        })?;
        let actual = frame.color_authority();
        if actual != expected {
            return Err(LibraryError::Render(format!(
                "export frame color authority '{}' does not match settings authority '{}'",
                actual.description(),
                expected.description()
            )));
        }
        Ok(actual)
    }

    pub fn export_format(&self) -> ExportFormat {
        match self.container.as_str() {
            "png" | "apng" => ExportFormat::Png,
            _ => ExportFormat::Video,
        }
    }

    /// Number of samples required to preserve a timeline duration at the
    /// configured output frame rate.
    pub fn frame_count_for_duration(&self, duration: f64) -> Result<u64, LibraryError> {
        validate_fps(self.fps)?;
        if !duration.is_finite() || duration < 0.0 {
            return Err(LibraryError::Render(format!(
                "export duration must be finite and non-negative, not {duration}"
            )));
        }
        checked_ceil_u64(duration * self.fps, "export frame count")
    }

    pub fn frame_time(&self, frame_index: u64) -> Result<f64, LibraryError> {
        validate_fps(self.fps)?;
        Ok(frame_index as f64 / self.fps)
    }

    /// Convert an exclusive frame range authored at the composition rate into
    /// an exclusive range at the output rate while preserving its time span.
    pub fn resample_timeline_frame_range(
        &self,
        range: Range<u64>,
        timeline_fps: f64,
    ) -> Result<Range<u64>, LibraryError> {
        validate_fps(self.fps)?;
        validate_fps(timeline_fps)?;
        if range.end < range.start {
            return Err(LibraryError::Render(
                "export timeline frame range is reversed".to_string(),
            ));
        }
        let scale = self.fps / timeline_fps;
        Ok(
            checked_ceil_u64(range.start as f64 * scale, "export range start")?
                ..checked_ceil_u64(range.end as f64 * scale, "export range end")?,
        )
    }

    /// Clamp an output-frame range to the composition duration. An empty
    /// result is always an error: a successful export must produce output.
    pub fn frame_range_within_duration(
        &self,
        range: Range<u64>,
        duration: f64,
    ) -> Result<Range<u64>, LibraryError> {
        let total_frames = self.frame_count_for_duration(duration)?;
        let end = range.end.min(total_frames);
        if range.start >= end {
            return Err(LibraryError::Render(format!(
                "export frame range {}..{} selects no frames from a {total_frames}-frame composition",
                range.start, range.end
            )));
        }
        Ok(range.start..end)
    }

    pub fn parameter_string(&self, key: &str) -> Option<String> {
        match self.parameters.get(key)? {
            Value::String(value) => Some(value.clone()),
            Value::Number(value) => Some(value.to_string()),
            Value::Bool(value) => Some(value.to_string()),
            _ => None,
        }
    }

    pub fn parameter_u64(&self, key: &str) -> Option<u64> {
        match self.parameters.get(key)? {
            Value::Number(value) => {
                if value.is_u64() {
                    value.as_u64()
                } else if value.is_i64() {
                    value
                        .as_i64()
                        .and_then(|v| if v >= 0 { Some(v as u64) } else { None })
                } else {
                    value.as_f64().map(|v| v.max(0.0).round() as u64)
                }
            }
            Value::String(value) => value.parse::<u64>().ok(),
            _ => None,
        }
    }

    pub fn parameter_f64(&self, key: &str) -> Option<f64> {
        match self.parameters.get(key)? {
            Value::Number(value) => value.as_f64(),
            Value::String(value) => value.parse::<f64>().ok(),
            _ => None,
        }
    }
}

fn validate_fps(fps: f64) -> Result<(), LibraryError> {
    if fps.is_finite() && fps > 0.0 {
        Ok(())
    } else {
        Err(LibraryError::Render(format!(
            "export fps must be finite and positive, not {fps}"
        )))
    }
}

fn checked_ceil_u64(value: f64, label: &str) -> Result<u64, LibraryError> {
    if !value.is_finite() || value < 0.0 || value > u64::MAX as f64 {
        return Err(LibraryError::Render(format!(
            "{label} is outside the supported u64 range: {value}"
        )));
    }
    Ok(value.ceil() as u64)
}

#[derive(Default)]
pub struct ExportRepository {
    pub plugins: HashMap<String, Arc<dyn ExportPlugin>>,
}

impl ExportRepository {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, plugin: Arc<dyn ExportPlugin>) {
        self.plugins.insert(plugin.id().to_string(), plugin);
    }

    pub fn get(&self, id: &str) -> Option<&Arc<dyn ExportPlugin>> {
        self.plugins.get(id)
    }
}

#[cfg(test)]
mod tests {
    use super::ExportSettings;
    use crate::model::project::{Composition, Project};
    use serde_json::json;

    #[test]
    fn output_fps_preserves_duration_when_resampling_timeline_ranges() {
        let sixty = ExportSettings::for_dimensions(1920, 1080, 60.0);
        assert_eq!(sixty.frame_count_for_duration(2.0).unwrap(), 120);
        assert_eq!(
            sixty.resample_timeline_frame_range(30..60, 30.0).unwrap(),
            60..120
        );
        assert_eq!(sixty.frame_time(119).unwrap(), 119.0 / 60.0);

        let twenty_four = ExportSettings::for_dimensions(1920, 1080, 24.0);
        assert_eq!(twenty_four.frame_count_for_duration(2.0).unwrap(), 48);
        assert_eq!(
            twenty_four
                .resample_timeline_frame_range(60..120, 60.0)
                .unwrap(),
            24..48
        );
        assert_eq!(twenty_four.frame_time(47).unwrap(), 47.0 / 24.0);

        assert_eq!(
            sixty.frame_range_within_duration(90..180, 2.0).unwrap(),
            90..120,
            "audio, video, and progress must share the clamped output range"
        );
        assert!(
            sixty
                .frame_range_within_duration(120..180, 2.0)
                .unwrap_err()
                .to_string()
                .contains("selects no frames")
        );
        assert!(
            sixty
                .frame_range_within_duration(0..0, 0.0)
                .unwrap_err()
                .to_string()
                .contains("selects no frames")
        );
    }

    #[test]
    fn project_data_cannot_select_an_executable_or_runtime_audio_path() {
        let mut project = Project::new("untrusted export document");
        project.export.ffmpeg_path = Some("/tmp/document-controlled-executable".to_string());
        project.export.parameters.insert(
            "audio_source".to_string(),
            json!("/tmp/document-controlled-audio.raw"),
        );
        project
            .export
            .parameters
            .insert("audio_channels".to_string(), json!(8));
        let (composition, _track) = Composition::new("main", 16, 16, 24.0, 1.0);

        let settings = ExportSettings::from_project(&project, &composition).unwrap();

        assert!(settings.trusted_ffmpeg_path().is_none());
        assert!(settings.runtime_audio_source().is_none());
        assert!(!settings.parameters.contains_key("audio_source"));
        assert!(!settings.parameters.contains_key("audio_channels"));
    }
}
