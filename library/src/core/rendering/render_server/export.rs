use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender};

use crate::cache::SharedCacheManager;
use crate::core::audio::authoring::{
    AUTHORING_AUDIO_CHANNELS, AUTHORING_AUDIO_SAMPLE_RATE, AuthoringAudioMixer,
    MAX_AUTHORING_AUDIO_WINDOW_FRAMES,
};
use crate::core::render_plan::{RenderPlan, evaluate_timeline_render_plan_frame_at_instance};
use crate::editor::RenderService;
use crate::error::LibraryError;
use crate::model::authoring::{AuthoringProject, InstancePath, TimelineId};
use crate::model::frame::frame::FrameInfo;
use crate::plugin::{ExportFormat, ExportSettings, PluginManager};
use crate::rendering::skia_renderer::SkiaRenderer;
use crate::util::output_path_identity::output_path_identity;

use super::{AuthoringExportResult, RenderRequestId, authoring_error_frame_info};

pub(super) struct AuthoringPngExportRequest {
    pub(super) request_id: RenderRequestId,
    pub(super) project: Arc<AuthoringProject>,
    pub(super) plan: Arc<RenderPlan>,
    pub(super) timeline_id: TimelineId,
    pub(super) instance_path: Option<InstancePath>,
    pub(super) frame_number: i64,
    pub(super) output_path: String,
}

pub(super) struct AuthoringVideoExportRequest {
    pub(super) request_id: RenderRequestId,
    pub(super) project: Arc<AuthoringProject>,
    pub(super) plan: Arc<RenderPlan>,
    pub(super) timeline_id: TimelineId,
    pub(super) instance_path: Option<InstancePath>,
    pub(super) output_path: String,
}

pub(super) enum AuthoringExportRequest {
    Png(AuthoringPngExportRequest),
    Video(AuthoringVideoExportRequest),
    Shutdown,
}

struct AuthoringExportRenderer {
    service: RenderService<SkiaRenderer>,
    width: u32,
    height: u32,
    background_color: crate::model::frame::color::Color,
}

impl AuthoringExportRenderer {
    fn new(
        frame_info: &FrameInfo,
        plugin_manager: Arc<PluginManager>,
        cache_manager: SharedCacheManager,
    ) -> Result<Self, LibraryError> {
        let (width, height) = authoring_export_dimensions(frame_info)?;
        let renderer = SkiaRenderer::new(
            width,
            height,
            frame_info.background_color.clone(),
            false,
            None,
            Some(Arc::clone(&cache_manager)),
        )?;
        Ok(Self {
            service: RenderService::new(renderer, plugin_manager, cache_manager),
            width,
            height,
            background_color: frame_info.background_color.clone(),
        })
    }

    fn prepare(&mut self, frame_info: &FrameInfo) -> Result<(), LibraryError> {
        let (width, height) = authoring_export_dimensions(frame_info)?;
        if self.width != width
            || self.height != height
            || self.background_color != frame_info.background_color
        {
            self.service.renderer.resize_render_target(
                width,
                height,
                frame_info.background_color.clone(),
            )?;
            self.width = width;
            self.height = height;
            self.background_color = frame_info.background_color.clone();
        }
        Ok(())
    }
}

fn authoring_export_dimensions(frame_info: &FrameInfo) -> Result<(u32, u32), LibraryError> {
    if frame_info.region.is_some() || frame_info.render_scale.into_inner() != 1.0 {
        return Err(LibraryError::Render(
            "authoring export requires an uncropped full frame at scale 1".to_string(),
        ));
    }
    let width = u32::try_from(frame_info.width).map_err(|_| {
        LibraryError::Render(format!(
            "authoring export width {} exceeds the renderer limit",
            frame_info.width
        ))
    })?;
    let height = u32::try_from(frame_info.height).map_err(|_| {
        LibraryError::Render(format!(
            "authoring export height {} exceeds the renderer limit",
            frame_info.height
        ))
    })?;
    if width == 0 || height == 0 {
        return Err(LibraryError::Render(
            "authoring export dimensions must be non-zero".to_string(),
        ));
    }
    Ok((width, height))
}

fn require_safe_authoring_output(
    project: &AuthoringProject,
    output_path: &str,
) -> Result<(), LibraryError> {
    let output = output_path_identity(output_path)?;
    for asset in &project.assets {
        if asset.path.trim().is_empty() {
            continue;
        }
        let source = output_path_identity(&asset.path).map_err(|error| {
            LibraryError::Render(format!(
                "cannot establish export-safe identity for Asset '{}' at '{}': {error}",
                asset.name, asset.path
            ))
        })?;
        if source.aliases(&output) {
            return Err(LibraryError::Render(format!(
                "authoring export destination '{output_path}' aliases Asset '{}' at '{}'; refusing to overwrite an input source",
                asset.name, asset.path
            )));
        }
    }
    Ok(())
}

fn write_authoring_png(
    plugin_manager: &PluginManager,
    output_path: &str,
    frame: &crate::plugin::ExportFrame,
    settings: &ExportSettings,
) -> Result<(), LibraryError> {
    let write = plugin_manager.export_frame("png_export", output_path, frame, settings);
    let finish = plugin_manager.finish_export("png_export", output_path, settings);
    match (write, finish) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(write_error), Ok(())) => Err(write_error),
        (Ok(()), Err(finish_error)) => Err(finish_error),
        (Err(write_error), Err(finish_error)) => Err(LibraryError::Render(format!(
            "PNG write failed: {write_error}; exporter finalization also failed: {finish_error}"
        ))),
    }
}

fn authoring_video_settings(
    project: &AuthoringProject,
    timeline_id: TimelineId,
    output_path: &str,
) -> Result<ExportSettings, LibraryError> {
    let timeline = project
        .timelines
        .get(&timeline_id)
        .ok_or_else(|| LibraryError::Render("export Timeline is missing".to_string()))?;
    let mut settings = ExportSettings::from_authoring_project(project, timeline)?;

    // A new pre-v1 document has no exporter selection yet. File > Export is
    // nevertheless a video operation, so it uses one explicit delivery preset
    // instead of manufacturing a legacy Composition merely to obtain defaults.
    if settings.export_format() == ExportFormat::Png {
        settings.container = "mp4".to_string();
        settings.codec = "libx264".to_string();
        settings.pixel_format = "yuv420p".to_string();
        settings.parameters.remove("compression");
    }
    if let Some(extension) = Path::new(output_path)
        .extension()
        .and_then(|value| value.to_str())
    {
        match extension.to_ascii_lowercase().as_str() {
            "mp4" => settings.container = "mp4".to_string(),
            "mkv" => settings.container = "mkv".to_string(),
            extension => {
                return Err(LibraryError::Validation(format!(
                    "Timeline video export supports .mp4 or .mkv, not .{extension}"
                )));
            }
        }
    } else {
        return Err(LibraryError::Validation(
            "Timeline video export path must end in .mp4 or .mkv".to_string(),
        ));
    }
    if settings.codec == "png" {
        settings.codec = "libx264".to_string();
    }
    if settings.pixel_format == "rgba" {
        settings.pixel_format = "yuv420p".to_string();
    }
    Ok(settings)
}

pub(super) fn authoring_video_frame_count(
    project: &AuthoringProject,
    timeline_id: TimelineId,
) -> Result<u64, LibraryError> {
    let timeline = project
        .timelines
        .get(&timeline_id)
        .ok_or_else(|| LibraryError::Render("export Timeline is missing".to_string()))?;
    if !timeline.fps.is_positive() || timeline.duration.is_negative() {
        return Err(LibraryError::Validation(
            "export Timeline must have a positive frame rate and non-negative duration".to_string(),
        ));
    }
    let numerator = i128::from(timeline.duration.value())
        .checked_mul(i128::from(timeline.fps.numerator()))
        .ok_or_else(|| LibraryError::Render("export frame count overflowed".to_string()))?;
    let denominator = i128::from(timeline.duration.timescale())
        .checked_mul(i128::from(timeline.fps.denominator()))
        .ok_or_else(|| LibraryError::Render("export frame count overflowed".to_string()))?;
    let frames = numerator
        .checked_add(denominator - 1)
        .ok_or_else(|| LibraryError::Render("export frame count overflowed".to_string()))?
        / denominator;
    let frames = u64::try_from(frames)
        .map_err(|_| LibraryError::Render("export frame count exceeds u64".to_string()))?;
    if frames == 0 {
        return Err(LibraryError::Validation(
            "export Timeline duration selects no video frames".to_string(),
        ));
    }
    Ok(frames)
}

fn combine_export_and_finish(
    export: Result<(), LibraryError>,
    finish: Result<(), LibraryError>,
) -> Result<(), LibraryError> {
    match (export, finish) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(export_error), Ok(())) => Err(export_error),
        (Ok(()), Err(finish_error)) => Err(finish_error),
        (Err(export_error), Err(finish_error)) => Err(LibraryError::Render(format!(
            "video export failed: {export_error}; exporter finalization also failed: {finish_error}"
        ))),
    }
}

struct TemporaryAuthoringAudio {
    path: Option<PathBuf>,
    writer: Option<BufWriter<File>>,
}

impl TemporaryAuthoringAudio {
    fn create() -> Result<Self, LibraryError> {
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
        Ok(Self {
            path: Some(path),
            writer: Some(BufWriter::new(file)),
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

    fn cleanup(&mut self) -> Result<(), LibraryError> {
        self.writer.take();
        let Some(path) = self.path.as_ref() else {
            return Ok(());
        };
        match std::fs::remove_file(path) {
            Ok(()) => {
                self.path = None;
                Ok(())
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.path = None;
                Ok(())
            }
            Err(error) => Err(LibraryError::Render(format!(
                "cannot remove temporary authoring audio '{}': {error}",
                path.display()
            ))),
        }
    }
}

impl Drop for TemporaryAuthoringAudio {
    fn drop(&mut self) {
        self.writer.take();
        if let Some(path) = self.path.take()
            && let Err(error) = std::fs::remove_file(&path)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            log::error!(
                "failed to remove temporary authoring audio '{}': {error}",
                path.display()
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

fn prepare_authoring_audio(
    project: &AuthoringProject,
    timeline_id: TimelineId,
    cache_manager: &SharedCacheManager,
    settings: &mut ExportSettings,
) -> Result<Option<TemporaryAuthoringAudio>, LibraryError> {
    let mut mixer = AuthoringAudioMixer::new(project, cache_manager.as_ref(), timeline_id)
        .map_err(|error| {
            LibraryError::Render(format!("authoring audio schedule failed: {error}"))
        })?;
    if !mixer.unsupported_video_assets().is_empty() {
        log::warn!(
            "Timeline-first export does not infer embedded audio for {} Video Asset(s); separately imported Audio Assets remain authoritative",
            mixer.unsupported_video_assets().len()
        );
    }
    if !mixer.has_audio_routes() {
        return Ok(None);
    }

    let frame_count = authoring_audio_frame_count(project, timeline_id)?;
    let mut temporary = TemporaryAuthoringAudio::create()?;
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
    settings.bind_runtime_audio_source(
        path,
        AUTHORING_AUDIO_CHANNELS,
        AUTHORING_AUDIO_SAMPLE_RATE,
    )?;
    Ok(Some(temporary))
}

fn combine_export_and_cleanup(
    export: Result<(), LibraryError>,
    cleanup: Result<(), LibraryError>,
) -> Result<(), LibraryError> {
    match (export, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(export_error), Ok(())) => Err(export_error),
        (Ok(()), Err(cleanup_error)) => Err(cleanup_error),
        (Err(export_error), Err(cleanup_error)) => Err(LibraryError::Render(format!(
            "video export failed: {export_error}; temporary audio cleanup also failed: {cleanup_error}"
        ))),
    }
}

fn run_authoring_png_export(
    request: AuthoringPngExportRequest,
    renderer: &mut Option<AuthoringExportRenderer>,
    plugin_manager: &Arc<PluginManager>,
    cache_manager: &SharedCacheManager,
) -> AuthoringExportResult {
    let frame_info = match evaluate_timeline_render_plan_frame_at_instance(
        request.project.as_ref(),
        request.plan.as_ref(),
        request.timeline_id,
        request.frame_number,
        1.0,
        None,
        request.instance_path.as_ref(),
    ) {
        Ok(frame_info) => frame_info,
        Err(error) => {
            return AuthoringExportResult {
                request_id: request.request_id,
                timeline_id: request.timeline_id,
                frame_number: request.frame_number,
                output_path: request.output_path,
                output: Err(error),
                frame_info: authoring_error_frame_info(
                    request.project.as_ref(),
                    request.timeline_id,
                    request.frame_number,
                    1.0,
                    None,
                ),
                frames_exported: 0,
                frame_count: 1,
            };
        }
    };

    let output = (|| {
        let timeline = request
            .project
            .timelines
            .get(&request.timeline_id)
            .ok_or_else(|| LibraryError::Render("export Timeline is missing".to_string()))?;
        let mut settings =
            ExportSettings::from_authoring_project(request.project.as_ref(), timeline)?;
        settings.container = "png".to_string();
        settings.codec = "png".to_string();
        settings.pixel_format = "rgba".to_string();
        settings.parameters.retain(|name, _| name == "compression");

        if renderer.is_none() {
            *renderer = Some(AuthoringExportRenderer::new(
                &frame_info,
                Arc::clone(plugin_manager),
                Arc::clone(cache_manager),
            )?);
        }
        let renderer = renderer.as_mut().ok_or_else(|| {
            LibraryError::Render("authoring export renderer did not initialize".to_string())
        })?;
        renderer.prepare(&frame_info)?;
        let frame = renderer
            .service
            .render_authoring_export_frame(request.project.as_ref(), &frame_info)?;

        // Re-resolve filesystem identities immediately before the plugin is
        // allowed to create or truncate the destination.
        require_safe_authoring_output(request.project.as_ref(), &request.output_path)?;
        write_authoring_png(
            plugin_manager.as_ref(),
            &request.output_path,
            &frame,
            &settings,
        )
    })();
    let frames_exported = u64::from(output.is_ok());
    AuthoringExportResult {
        request_id: request.request_id,
        timeline_id: request.timeline_id,
        frame_number: request.frame_number,
        output_path: request.output_path,
        output,
        frame_info,
        frames_exported,
        frame_count: 1,
    }
}

fn run_authoring_video_export(
    request: AuthoringVideoExportRequest,
    renderer: &mut Option<AuthoringExportRenderer>,
    plugin_manager: &Arc<PluginManager>,
    cache_manager: &SharedCacheManager,
) -> AuthoringExportResult {
    let mut frame_info =
        authoring_error_frame_info(request.project.as_ref(), request.timeline_id, 0, 1.0, None);
    let frame_count =
        match authoring_video_frame_count(request.project.as_ref(), request.timeline_id) {
            Ok(frame_count) => frame_count,
            Err(error) => {
                return AuthoringExportResult {
                    request_id: request.request_id,
                    timeline_id: request.timeline_id,
                    frame_number: 0,
                    output_path: request.output_path,
                    output: Err(error),
                    frame_info,
                    frames_exported: 0,
                    frame_count: 0,
                };
            }
        };
    let mut settings = match authoring_video_settings(
        request.project.as_ref(),
        request.timeline_id,
        &request.output_path,
    ) {
        Ok(settings) => settings,
        Err(error) => {
            return AuthoringExportResult {
                request_id: request.request_id,
                timeline_id: request.timeline_id,
                frame_number: 0,
                output_path: request.output_path,
                output: Err(error),
                frame_info,
                frames_exported: 0,
                frame_count,
            };
        }
    };

    let mut frames_exported = 0_u64;
    let mut exporter_attempted = false;
    let mut temporary_audio = None;
    let output = (|| {
        require_safe_authoring_output(request.project.as_ref(), &request.output_path)?;
        temporary_audio = prepare_authoring_audio(
            request.project.as_ref(),
            request.timeline_id,
            cache_manager,
            &mut settings,
        )?;
        for frame_index in 0..frame_count {
            let exact_frame = i64::try_from(frame_index)
                .map_err(|_| LibraryError::Render("video frame index exceeds i64".to_string()))?;
            frame_info = evaluate_timeline_render_plan_frame_at_instance(
                request.project.as_ref(),
                request.plan.as_ref(),
                request.timeline_id,
                exact_frame,
                1.0,
                None,
                request.instance_path.as_ref(),
            )?;
            if renderer.is_none() {
                *renderer = Some(AuthoringExportRenderer::new(
                    &frame_info,
                    Arc::clone(plugin_manager),
                    Arc::clone(cache_manager),
                )?);
            }
            let renderer = renderer.as_mut().ok_or_else(|| {
                LibraryError::Render("authoring export renderer did not initialize".to_string())
            })?;
            renderer.prepare(&frame_info)?;
            let frame = renderer
                .service
                .render_authoring_export_frame(request.project.as_ref(), &frame_info)?;

            // This check is deliberately repeated: an input symlink or output
            // alias can change while a long export is running.
            require_safe_authoring_output(request.project.as_ref(), &request.output_path)?;
            exporter_attempted = true;
            plugin_manager.export_frame(
                "ffmpeg_export",
                &request.output_path,
                &frame,
                &settings,
            )?;
            frames_exported = frames_exported.checked_add(1).ok_or_else(|| {
                LibraryError::Render("exported frame count overflowed".to_string())
            })?;
        }
        Ok(())
    })();
    let finish = if exporter_attempted {
        plugin_manager.finish_export("ffmpeg_export", &request.output_path, &settings)
    } else {
        Ok(())
    };
    let output = combine_export_and_finish(output, finish);
    let cleanup = temporary_audio
        .as_mut()
        .map_or(Ok(()), TemporaryAuthoringAudio::cleanup);
    let output = combine_export_and_cleanup(output, cleanup);
    let frame_number = i64::try_from(frames_exported.saturating_sub(1)).unwrap_or(i64::MAX);
    AuthoringExportResult {
        request_id: request.request_id,
        timeline_id: request.timeline_id,
        frame_number,
        output_path: request.output_path,
        output,
        frame_info,
        frames_exported,
        frame_count,
    }
}

pub(super) fn run_authoring_export_worker(
    receiver: Receiver<AuthoringExportRequest>,
    result_sender: Sender<AuthoringExportResult>,
    plugin_manager: Arc<PluginManager>,
    cache_manager: SharedCacheManager,
) {
    let mut renderer: Option<AuthoringExportRenderer> = None;
    while let Ok(request) = receiver.recv() {
        let result = match request {
            AuthoringExportRequest::Png(request) => {
                run_authoring_png_export(request, &mut renderer, &plugin_manager, &cache_manager)
            }
            AuthoringExportRequest::Video(request) => {
                run_authoring_video_export(request, &mut renderer, &plugin_manager, &cache_manager)
            }
            AuthoringExportRequest::Shutdown => break,
        };
        if result_sender.send(result).is_err() {
            break;
        }
    }
}
