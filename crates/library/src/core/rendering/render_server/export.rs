mod authoring_audio;
pub(super) mod cancellation;
mod panic_guard;
mod video_output;
mod worker;

use std::collections::BTreeSet;
use std::path::Path;
use std::sync::Arc;

use crate::cache::SharedCacheManager;
use crate::core::render_plan::{
    RenderCapability, RenderPlan, evaluate_timeline_render_plan_frame_at_instance,
};
use crate::editor::{RenderDestination, RenderService};
use crate::error::LibraryError;
use crate::model::authoring::{AuthoringProject, InstancePath, TimelineId};
use crate::model::frame::entity::{FrameContent, FrameGroupKind, FrameItem};
use crate::model::frame::frame::FrameInfo;
use crate::plugin::{
    ExportDestination, ExportFormat, ExportFrame, ExportPlugin, ExportSettings, PluginManager,
};
use crate::rendering::renderer::Renderer;
use crate::rendering::skia_renderer::SkiaRenderer;
#[cfg(test)]
pub(super) use crate::util::atomic_file::AtomicSyncTestControl;
use crate::util::output_path_identity::output_path_identity;

use super::{AuthoringExportResult, RenderRequestId, authoring_error_frame_info};
#[cfg(test)]
pub(super) use authoring_audio::TemporaryAudioTestControl;
use authoring_audio::{TemporaryAuthoringAudio, prepare_authoring_audio};
use cancellation::ExportCancellation;
#[cfg(test)]
use cancellation::ExportCheckpoint;
use panic_guard::catch_export_panic;
use video_output::AuthoringVideoOutput;
pub(super) use worker::run_authoring_export_worker;

pub(super) struct AuthoringPngExportRequest {
    pub(super) cancellation: Arc<ExportCancellation>,
    pub(super) request_id: RenderRequestId,
    pub(super) project: Arc<AuthoringProject>,
    pub(super) plan: Arc<RenderPlan>,
    pub(super) timeline_id: TimelineId,
    pub(super) instance_path: Option<InstancePath>,
    pub(super) frame_number: i64,
    pub(super) output_path: String,
}

pub(super) struct AuthoringVideoExportRequest {
    pub(super) cancellation: Arc<ExportCancellation>,
    pub(super) request_id: RenderRequestId,
    pub(super) project: Arc<AuthoringProject>,
    pub(super) plan: Arc<RenderPlan>,
    pub(super) timeline_id: TimelineId,
    pub(super) instance_path: Option<InstancePath>,
    pub(super) output_path: String,
    #[cfg(test)]
    pub(super) temporary_audio_test_control: Arc<TemporaryAudioTestControl>,
    #[cfg(test)]
    pub(super) atomic_sync_test_control: Arc<AtomicSyncTestControl>,
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
    requires_gpu: bool,
}

#[derive(Default)]
struct AuthoringParticlePreflight {
    target_sizes: BTreeSet<(u32, u32)>,
}

impl AuthoringParticlePreflight {
    fn requires_gpu(&self) -> bool {
        !self.target_sizes.is_empty()
    }

    fn target_sizes(&self) -> Vec<(u32, u32)> {
        self.target_sizes.iter().copied().collect()
    }

    fn include_frame(&mut self, frame_info: &FrameInfo) -> Result<(), LibraryError> {
        let root = authoring_export_dimensions(frame_info)?;
        collect_particle_target_sizes(&frame_info.items, root, &mut self.target_sizes)
    }
}

impl AuthoringExportRenderer {
    fn new(
        frame_info: &FrameInfo,
        requires_gpu: bool,
        plugin_manager: Arc<PluginManager>,
        cache_manager: SharedCacheManager,
    ) -> Result<Self, LibraryError> {
        let (width, height) = authoring_export_dimensions(frame_info)?;
        let mut renderer = SkiaRenderer::new(
            width,
            height,
            frame_info.background_color.clone(),
            requires_gpu,
            None,
            Some(Arc::clone(&cache_manager)),
        )?;
        if requires_gpu && renderer.get_gpu_context().is_none() {
            return Err(particle_gpu_unavailable());
        }
        Ok(Self {
            service: RenderService::new(renderer, plugin_manager, cache_manager),
            width,
            height,
            background_color: frame_info.background_color.clone(),
            requires_gpu,
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

    fn render_frame(
        &mut self,
        project: &AuthoringProject,
        frame_info: &FrameInfo,
    ) -> Result<ExportFrame, LibraryError> {
        if frame_uses_gpu_particle(&frame_info.items)
            && self.service.renderer.get_gpu_context().is_none()
        {
            return Err(particle_gpu_unavailable());
        }
        self.service
            .render_authoring_export_frame(project, frame_info)
    }
}

fn particle_gpu_unavailable() -> LibraryError {
    LibraryError::Render(
        "GPU Particle export is unavailable: the export worker could not create an OpenGL GPU session; desktop OpenGL 4.3 compute/SSBO support is required"
            .to_string(),
    )
}

fn ensure_authoring_export_renderer(
    renderer: &mut Option<AuthoringExportRenderer>,
    project: &AuthoringProject,
    frame_info: &FrameInfo,
    particle_preflight: &AuthoringParticlePreflight,
    plugin_manager: &Arc<PluginManager>,
    cache_manager: &SharedCacheManager,
) -> Result<(), LibraryError> {
    let requires_gpu = particle_preflight.requires_gpu();
    if renderer
        .as_ref()
        .is_none_or(|renderer| renderer.requires_gpu != requires_gpu)
    {
        let replacement = AuthoringExportRenderer::new(
            frame_info,
            requires_gpu,
            Arc::clone(plugin_manager),
            Arc::clone(cache_manager),
        )?;
        *renderer = Some(replacement);
    }
    let renderer = renderer.as_mut().ok_or_else(|| {
        LibraryError::Render("authoring export renderer did not initialize".to_string())
    })?;
    renderer.prepare(frame_info)?;
    if requires_gpu {
        renderer.service.preflight_authoring_particle_backend(
            project,
            RenderDestination::Export,
            &particle_preflight.target_sizes(),
        )?;
    }
    Ok(())
}

fn frame_uses_gpu_particle(items: &[FrameItem]) -> bool {
    items.iter().any(|item| match item {
        FrameItem::Object(object) => matches!(&object.content, FrameContent::ParticleScene { .. }),
        FrameItem::Group(group) => frame_uses_gpu_particle(&group.items),
        FrameItem::Transition(transition) => {
            frame_uses_gpu_particle(std::slice::from_ref(&transition.from.item))
                || frame_uses_gpu_particle(std::slice::from_ref(&transition.to.item))
        }
    })
}

fn collect_particle_target_sizes(
    items: &[FrameItem],
    current_target: (u32, u32),
    targets: &mut BTreeSet<(u32, u32)>,
) -> Result<(), LibraryError> {
    for item in items {
        match item {
            FrameItem::Object(object) => {
                if matches!(&object.content, FrameContent::ParticleScene { .. }) {
                    targets.insert(current_target);
                }
            }
            FrameItem::Group(group) => {
                let child_target = if matches!(
                    group.kind,
                    FrameGroupKind::Composition | FrameGroupKind::ImageTransform
                ) {
                    (
                        preflight_dimension(group.width, "width")?,
                        preflight_dimension(group.height, "height")?,
                    )
                } else {
                    current_target
                };
                collect_particle_target_sizes(&group.items, child_target, targets)?;
            }
            FrameItem::Transition(transition) => {
                collect_particle_target_sizes(
                    std::slice::from_ref(&transition.from.item),
                    current_target,
                    targets,
                )?;
                collect_particle_target_sizes(
                    std::slice::from_ref(&transition.to.item),
                    current_target,
                    targets,
                )?;
            }
        }
    }
    Ok(())
}

fn preflight_dimension(value: u64, axis: &str) -> Result<u32, LibraryError> {
    u32::try_from(value.max(1)).map_err(|_| {
        LibraryError::Render(format!(
            "GPU Particle nested target {axis} {value} exceeds the renderer limit"
        ))
    })
}

fn preflight_authoring_video_particle_targets(
    project: &AuthoringProject,
    plan: &RenderPlan,
    plugin_manager: &PluginManager,
    timeline_id: TimelineId,
    instance_path: Option<&InstancePath>,
    frame_count: u64,
    cancellation: &ExportCancellation,
) -> Result<AuthoringParticlePreflight, LibraryError> {
    cancellation.check()?;
    if !plan.timeline_may_require_capability(
        project,
        timeline_id,
        instance_path,
        RenderCapability::Gpu,
    )? {
        return Ok(AuthoringParticlePreflight::default());
    }

    let mut preflight = AuthoringParticlePreflight::default();
    // The hierarchical query is intentionally conservative around nested
    // TimeMap policies. Resolve exact reachability and target dimensions
    // through the production evaluator before any export side effect.
    for frame_index in 0..frame_count {
        cancellation.check()?;
        let exact_frame = i64::try_from(frame_index)
            .map_err(|_| LibraryError::Render("video frame index exceeds i64".to_string()))?;
        let frame_info = evaluate_timeline_render_plan_frame_at_instance(
            project,
            plan,
            plugin_manager,
            timeline_id,
            exact_frame,
            1.0,
            None,
            instance_path,
        )?;
        preflight.include_frame(&frame_info)?;
    }
    Ok(preflight)
}

#[cfg(test)]
pub(super) fn preflight_authoring_video_requires_gpu(
    project: &AuthoringProject,
    plan: &RenderPlan,
    plugin_manager: &PluginManager,
    timeline_id: TimelineId,
    instance_path: Option<&InstancePath>,
    frame_count: u64,
) -> Result<bool, LibraryError> {
    Ok(preflight_authoring_video_particle_targets(
        project,
        plan,
        plugin_manager,
        timeline_id,
        instance_path,
        frame_count,
        &ExportCancellation::default(),
    )?
    .requires_gpu())
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

pub(super) fn require_safe_authoring_output(
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
    let exporter = plugin_manager.require_export_plugin("png_export")?;
    let destination = ExportDestination::staged(output_path, output_path);
    let (write, _) = catch_export_panic("authoring PNG exporter write", || {
        exporter.export_frame(&destination, frame, settings)
    });
    let (finish, _) = catch_export_panic("authoring PNG exporter finalization", || {
        exporter.finish_export(&destination, settings)
    });
    combine_operation_and_cleanup(
        write,
        finish,
        "PNG write failed",
        "exporter finalization also failed",
    )
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

pub(super) fn combine_operation_and_cleanup(
    operation: Result<(), LibraryError>,
    cleanup: Result<(), LibraryError>,
    operation_failure: &'static str,
    cleanup_failure: &'static str,
) -> Result<(), LibraryError> {
    match (operation, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(operation_error), Ok(())) => Err(operation_error),
        (Ok(()), Err(cleanup_error)) => Err(cleanup_error),
        (Err(operation_error), Err(cleanup_error)) => Err(LibraryError::OperationAndCleanup {
            operation_phase: operation_failure,
            operation: Box::new(operation_error),
            cleanup_phase: cleanup_failure,
            cleanup: Box::new(cleanup_error),
        }),
    }
}

fn run_authoring_png_export(
    request: AuthoringPngExportRequest,
    renderer: &mut Option<AuthoringExportRenderer>,
    plugin_manager: &Arc<PluginManager>,
    cache_manager: &SharedCacheManager,
) -> AuthoringExportResult {
    let frame_info = match request.cancellation.check().and_then(|()| {
        evaluate_timeline_render_plan_frame_at_instance(
            request.project.as_ref(),
            request.plan.as_ref(),
            plugin_manager.as_ref(),
            request.timeline_id,
            request.frame_number,
            1.0,
            None,
            request.instance_path.as_ref(),
        )
    }) {
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
                published: false,
                frame_count: 1,
            };
        }
    };
    let mut particle_preflight = AuthoringParticlePreflight::default();
    let preflight_result = particle_preflight.include_frame(&frame_info);

    let output = (|| {
        request.cancellation.check()?;
        preflight_result?;
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

        ensure_authoring_export_renderer(
            renderer,
            request.project.as_ref(),
            &frame_info,
            &particle_preflight,
            plugin_manager,
            cache_manager,
        )?;
        let renderer = renderer.as_mut().ok_or_else(|| {
            LibraryError::Render("authoring export renderer did not initialize".to_string())
        })?;
        let frame = renderer.render_frame(request.project.as_ref(), &frame_info)?;

        // Re-resolve filesystem identities immediately before the plugin is
        // allowed to create or truncate the destination.
        require_safe_authoring_output(request.project.as_ref(), &request.output_path)?;
        // A PNG write publishes directly; once it starts, cancellation is too
        // late. The request remains tracked until write and finish complete.
        #[cfg(test)]
        request
            .cancellation
            .pause_at(ExportCheckpoint::BeforePublication)?;
        request.cancellation.begin_publication()?;
        #[cfg(test)]
        request
            .cancellation
            .pause_at(ExportCheckpoint::PublicationStarted)?;
        write_authoring_png(
            plugin_manager.as_ref(),
            &request.output_path,
            &frame,
            &settings,
        )
    })();
    let frames_exported = u64::from(output.is_ok());
    let published = output.is_ok();
    AuthoringExportResult {
        request_id: request.request_id,
        timeline_id: request.timeline_id,
        frame_number: request.frame_number,
        output_path: request.output_path,
        output,
        frame_info,
        frames_exported,
        published,
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
                    published: false,
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
                published: false,
                frame_count,
            };
        }
    };
    let particle_preflight = match preflight_authoring_video_particle_targets(
        request.project.as_ref(),
        request.plan.as_ref(),
        plugin_manager.as_ref(),
        request.timeline_id,
        request.instance_path.as_ref(),
        frame_count,
        &request.cancellation,
    ) {
        Ok(preflight) => preflight,
        Err(error) => {
            return AuthoringExportResult {
                request_id: request.request_id,
                timeline_id: request.timeline_id,
                frame_number: 0,
                output_path: request.output_path,
                output: Err(error),
                frame_info,
                frames_exported: 0,
                published: false,
                frame_count,
            };
        }
    };

    let mut frames_exported = 0_u64;
    let mut exporter_attempted = false;
    let mut temporary_audio = None;
    let mut video_output = None;
    let mut destination_lease = None;
    let mut exporter: Option<Arc<dyn ExportPlugin>> = None;
    let (output, job_panicked) = catch_export_panic("authoring video export job", || {
        request.cancellation.check()?;
        require_safe_authoring_output(request.project.as_ref(), &request.output_path)?;
        // Establish the complete export's renderer capability before creating
        // temporary audio or giving the exporter a chance to open the output.
        // A Particle that starts late therefore cannot leave a partial video.
        ensure_authoring_export_renderer(
            renderer,
            request.project.as_ref(),
            &frame_info,
            &particle_preflight,
            plugin_manager,
            cache_manager,
        )?;
        request.cancellation.check()?;
        exporter = Some(plugin_manager.require_export_plugin("ffmpeg_export")?);
        // Exporter sessions end before host-side cleanup and publication.
        // Keep this logical destination reserved across that entire gap so a
        // second coordinator cannot publish over the same user-selected path.
        destination_lease = Some(plugin_manager.reserve_export_destination(&request.output_path)?);
        let output = AuthoringVideoOutput::begin(&request.output_path)?;
        #[cfg(test)]
        let output =
            output.with_sync_test_control(Arc::clone(&request.atomic_sync_test_control))?;
        video_output = Some(output);
        let audio_preparation = prepare_authoring_audio(
            request.project.as_ref(),
            request.timeline_id,
            cache_manager,
            &mut settings,
            &mut temporary_audio,
            &request.cancellation,
            #[cfg(test)]
            Arc::clone(&request.temporary_audio_test_control),
        );
        audio_preparation?;
        for frame_index in 0..frame_count {
            #[cfg(test)]
            request
                .cancellation
                .pause_at(ExportCheckpoint::BeforeFrame(frame_index))?;
            request.cancellation.check()?;
            let exact_frame = i64::try_from(frame_index)
                .map_err(|_| LibraryError::Render("video frame index exceeds i64".to_string()))?;
            frame_info = evaluate_timeline_render_plan_frame_at_instance(
                request.project.as_ref(),
                request.plan.as_ref(),
                plugin_manager.as_ref(),
                request.timeline_id,
                exact_frame,
                1.0,
                None,
                request.instance_path.as_ref(),
            )?;
            request.cancellation.check()?;
            let renderer = renderer.as_mut().ok_or_else(|| {
                LibraryError::Render("authoring export renderer did not initialize".to_string())
            })?;
            renderer.prepare(&frame_info)?;
            let frame = renderer.render_frame(request.project.as_ref(), &frame_info)?;

            #[cfg(test)]
            request
                .cancellation
                .pause_at(ExportCheckpoint::FrameRendered(frame_index))?;

            // This check is deliberately repeated: an input symlink or output
            // alias can change while a long export is running.
            require_safe_authoring_output(request.project.as_ref(), &request.output_path)?;
            request.cancellation.check()?;
            exporter_attempted = true;
            let destination = video_output.as_ref().ok_or_else(|| {
                LibraryError::Render("video export destination is unavailable".to_string())
            })?;
            let exporter = exporter.as_ref().ok_or_else(|| {
                LibraryError::Render("video exporter endpoint is unavailable".to_string())
            })?;
            exporter.export_frame(destination.destination(), &frame, &settings)?;
            frames_exported = frames_exported.checked_add(1).ok_or_else(|| {
                LibraryError::Render("exported frame count overflowed".to_string())
            })?;
        }
        Ok(())
    });
    let (finish, finish_panicked) =
        match (exporter_attempted, video_output.as_ref(), exporter.as_ref()) {
            (true, Some(destination), Some(exporter)) => {
                catch_export_panic("authoring video exporter finalization", || {
                    exporter.finish_export(destination.destination(), &settings)
                })
            }
            (true, _, _) => (
                Err(LibraryError::Render(
                    "video exporter was started without its pinned endpoint or destination"
                        .to_string(),
                )),
                false,
            ),
            (false, _, _) => (Ok(()), false),
        };
    let output = combine_operation_and_cleanup(
        output.and_then(|()| request.cancellation.check()),
        finish,
        "video export failed",
        "exporter finalization also failed",
    );
    let cleanup = temporary_audio
        .as_mut()
        .map_or(Ok(()), TemporaryAuthoringAudio::cleanup);
    let mut output = combine_operation_and_cleanup(
        output.and_then(|()| request.cancellation.check()),
        cleanup,
        "video export failed",
        "temporary audio cleanup also failed",
    );
    #[cfg(test)]
    if output.is_ok() {
        output = request
            .cancellation
            .pause_at(ExportCheckpoint::BeforePublication);
    }
    if output.is_ok() {
        // Linearize cancellation against publication after encoder shutdown
        // and Audio cleanup. An accepted cancel can never reach atomic replace.
        // Once publication starts, callers must await its terminal result.
        output = request.cancellation.begin_publication();
    }
    #[cfg(test)]
    if output.is_ok() {
        output = request
            .cancellation
            .pause_at(ExportCheckpoint::PublicationStarted);
    }
    if output.is_ok() {
        output = video_output
            .take()
            .ok_or_else(|| {
                LibraryError::Render("video export staging transaction is unavailable".to_string())
            })
            .and_then(|video_output| {
                video_output.publish(|| {
                    require_safe_authoring_output(request.project.as_ref(), &request.output_path)
                })
            });
    }
    if output.is_err()
        && let Some(video_output) = video_output.take()
    {
        let cleanup = video_output.abort();
        output = combine_operation_and_cleanup(
            output,
            cleanup,
            "video export failed",
            "staging cleanup also failed",
        );
    }
    if job_panicked || finish_panicked {
        *renderer = None;
    }
    let published = output.is_ok();
    drop(destination_lease);
    let frame_number = i64::try_from(frames_exported.saturating_sub(1)).unwrap_or(i64::MAX);
    AuthoringExportResult {
        request_id: request.request_id,
        timeline_id: request.timeline_id,
        frame_number,
        output_path: request.output_path,
        output,
        frame_info,
        frames_exported,
        published,
        frame_count,
    }
}
