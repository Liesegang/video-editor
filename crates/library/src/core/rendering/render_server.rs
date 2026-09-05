use log::error;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError, channel, sync_channel};
use std::thread;

use crate::RenderDestination;
use crate::cache::SharedCacheManager;
use crate::core::render_plan::{RenderPlan, evaluate_timeline_render_plan_frame_at_instance};
use crate::editor::RenderService;
use crate::error::LibraryError;
use crate::model::authoring::{AuthoringProject, InstancePath, TimelineId};
use crate::model::frame::frame::{FrameInfo, Region};
use crate::plugin::PluginManager;
use crate::rendering::renderer::{RenderOutput, Renderer};
use crate::rendering::skia_renderer::SkiaRenderer;

mod export;
mod preview_mailbox;

#[cfg(test)]
use export::{AtomicSyncTestControl, TemporaryAudioTestControl};
use export::{
    AuthoringExportRequest, AuthoringPngExportRequest, AuthoringVideoExportRequest,
    run_authoring_export_worker,
};
use preview_mailbox::{PreviewMailbox, PreviewSubmission, PreviewWorkerMessage};

pub struct RenderServer {
    preview_mailbox: Arc<PreviewMailbox<AuthoringRenderRequest>>,
    rx_authoring_result: Receiver<RenderResult>,
    tx_authoring_export: SyncSender<AuthoringExportRequest>,
    rx_authoring_export_result: Receiver<AuthoringExportResult>,
    #[cfg(test)]
    temporary_audio_test_control: Arc<TemporaryAudioTestControl>,
    #[cfg(test)]
    atomic_sync_test_control: Arc<AtomicSyncTestControl>,
    handle: Option<thread::JoinHandle<()>>,
    export_handle: Option<thread::JoinHandle<()>>,
}

struct AuthoringRenderRequest {
    request_id: RenderRequestId,
    project: Arc<AuthoringProject>,
    plan: Arc<RenderPlan>,
    timeline_id: TimelineId,
    instance_path: Option<InstancePath>,
    frame_number: i64,
    render_scale: f64,
    region: Option<Region>,
}

/// Opaque identity assigned by the caller to one asynchronous render request.
///
/// The renderer deliberately does not attach timeline semantics to this value.
/// It only returns the identity with the result so the caller can validate the
/// result against its own Project/navigation generation before applying it.
/// This identity does not manage the lifetime or ownership of shared GPU
/// textures; that remains a separate renderer/UI context contract.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RenderRequestId(u64);

impl RenderRequestId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

pub struct RenderResult {
    pub request_id: RenderRequestId,
    pub frame_hash: u64,
    pub output: Result<RenderOutput, LibraryError>,
    pub frame_info: FrameInfo,
}

/// Result of a full-frame authoring export request.
///
/// Export has a dedicated queue and worker so Preview coalescing can neither
/// discard an export request nor consume its completion.
pub struct AuthoringExportResult {
    pub request_id: RenderRequestId,
    pub timeline_id: TimelineId,
    pub frame_number: i64,
    pub output_path: String,
    pub output: Result<(), LibraryError>,
    pub frame_info: FrameInfo,
    /// Frames accepted by the exporter before the request completed.
    pub frames_exported: u64,
    /// Whether the completed artifact was published at `output_path`.
    ///
    /// Video export sets this only after atomic publication. It remains false
    /// when frames reached the exporter but rendering, finalization, cleanup,
    /// validation, sync, or publication later failed.
    pub published: bool,
    /// Total frames selected by the immutable export request.
    pub frame_count: u64,
}

fn authoring_error_frame_info(
    project: &AuthoringProject,
    timeline_id: TimelineId,
    frame_number: i64,
    render_scale: f64,
    region: Option<Region>,
) -> FrameInfo {
    let Some(timeline) = project.timelines.get(&timeline_id) else {
        return FrameInfo {
            width: 1,
            height: 1,
            background_color: crate::model::frame::color::Color::black(),
            color_profile: "sRGB".to_string(),
            render_scale: ordered_float::OrderedFloat(render_scale),
            now_time: ordered_float::OrderedFloat(0.0),
            region,
            items: Vec::new(),
        };
    };
    let now_time = (frame_number >= 0)
        .then(|| crate::model::authoring::MediaTime::from_frame_index(frame_number, timeline.fps))
        .and_then(Result::ok)
        .map(crate::model::authoring::MediaTime::to_seconds_f64)
        .unwrap_or(0.0);
    FrameInfo {
        width: timeline.width,
        height: timeline.height,
        background_color: timeline.background_color.clone(),
        color_profile: timeline.color_profile.clone(),
        render_scale: ordered_float::OrderedFloat(render_scale),
        now_time: ordered_float::OrderedFloat(now_time),
        region,
        items: Vec::new(),
    }
}

impl RenderServer {
    pub fn new(plugin_manager: Arc<PluginManager>, cache_manager: SharedCacheManager) -> Self {
        let preview_mailbox = Arc::new(PreviewMailbox::new());
        let worker_mailbox = Arc::clone(&preview_mailbox);
        let (tx_authoring_result, rx_authoring_result) = channel::<RenderResult>();
        let (tx_authoring_export, rx_authoring_export) = sync_channel::<AuthoringExportRequest>(1);
        let (tx_authoring_export_result, rx_authoring_export_result) =
            channel::<AuthoringExportResult>();
        #[cfg(test)]
        let temporary_audio_test_control = Arc::new(TemporaryAudioTestControl::default());
        #[cfg(test)]
        let atomic_sync_test_control = Arc::new(AtomicSyncTestControl::default());
        let export_plugin_manager = Arc::clone(&plugin_manager);
        let export_cache_manager = Arc::clone(&cache_manager);
        let export_handle = thread::spawn(move || {
            run_authoring_export_worker(
                rx_authoring_export,
                tx_authoring_export_result,
                export_plugin_manager,
                export_cache_manager,
            );
        });

        let handle = thread::spawn(move || {
            let frame_plugin_manager = Arc::clone(&plugin_manager);
            let mut current_background_color = crate::model::frame::color::Color {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            };
            let renderer = SkiaRenderer::new(
                1920,
                1080,
                current_background_color.clone(),
                true,
                None,
                None,
            );
            let mut initialization_error = None;
            let mut render_service = match renderer {
                Ok(renderer) => Some(RenderService::new(renderer, plugin_manager, cache_manager)),
                Err(error) => {
                    error!("Failed to initialize render server: {error}");
                    initialization_error = Some(error.to_string());
                    None
                }
            };
            let mut current_width = 1920;
            let mut current_height = 1080;

            'server: loop {
                let mut pending_render = match worker_mailbox.recv() {
                    PreviewWorkerMessage::Render(request) => request,
                    PreviewWorkerMessage::SetSharingContext(handle, hwnd) => {
                        if let Some(render_service) = render_service.as_mut()
                            && let Err(error) =
                                render_service.renderer.set_sharing_context(handle, hwnd)
                        {
                            error!("Failed to set render sharing context: {error}");
                        }
                        continue;
                    }
                    PreviewWorkerMessage::Shutdown => break,
                };
                // A newer scrub/playback request may have arrived between the
                // wake-up and evaluation. Drop that stale snapshot before any
                // frame or raster work begins.
                if let Some(newer) = worker_mailbox.take_newer_render() {
                    pending_render = newer;
                }
                let AuthoringRenderRequest {
                    request_id,
                    project,
                    plan,
                    timeline_id,
                    instance_path,
                    frame_number,
                    render_scale,
                    region,
                } = pending_render;
                let frame_info = match evaluate_timeline_render_plan_frame_at_instance(
                    project.as_ref(),
                    plan.as_ref(),
                    frame_plugin_manager.as_ref(),
                    timeline_id,
                    frame_number,
                    render_scale,
                    region,
                    instance_path.as_ref(),
                ) {
                    Ok(frame_info) => frame_info,
                    Err(error) => {
                        let frame_info = authoring_error_frame_info(
                            project.as_ref(),
                            timeline_id,
                            frame_number,
                            render_scale,
                            region,
                        );
                        if tx_authoring_result
                            .send(RenderResult {
                                request_id,
                                frame_hash: 0,
                                output: Err(error),
                                frame_info,
                            })
                            .is_err()
                        {
                            break 'server;
                        }
                        continue 'server;
                    }
                };
                let Some(render_service) = render_service.as_mut() else {
                    let error =
                        LibraryError::Render(initialization_error.clone().unwrap_or_else(|| {
                            "Preview renderer is unavailable without an error message".to_string()
                        }));
                    if tx_authoring_result
                        .send(RenderResult {
                            request_id,
                            frame_hash: 0,
                            output: Err(error),
                            frame_info,
                        })
                        .is_err()
                    {
                        break;
                    }
                    continue;
                };
                let render_scale = frame_info.render_scale.into_inner();
                let (target_width, target_height) = if let Some(region) = &frame_info.region {
                    (
                        (region.width * render_scale).round() as u32,
                        (region.height * render_scale).round() as u32,
                    )
                } else {
                    (
                        (frame_info.width as f64 * render_scale).round() as u32,
                        (frame_info.height as f64 * render_scale).round() as u32,
                    )
                };

                if current_width != target_width
                    || current_height != target_height
                    || current_background_color != frame_info.background_color
                {
                    match render_service.renderer.resize_render_target(
                        target_width,
                        target_height,
                        frame_info.background_color.clone(),
                    ) {
                        Ok(()) => {
                            current_width = target_width;
                            current_height = target_height;
                            current_background_color = frame_info.background_color.clone();
                        }
                        Err(error) => {
                            error!("Failed to resize render target: {error}");
                            if tx_authoring_result
                                .send(RenderResult {
                                    request_id,
                                    frame_hash: 0,
                                    output: Err(error),
                                    frame_info,
                                })
                                .is_err()
                            {
                                break;
                            }
                            continue;
                        }
                    }
                }

                let output = render_service.render_authoring_frame(
                    project.as_ref(),
                    &frame_info,
                    RenderDestination::Preview,
                );
                if let Err(error) = &output {
                    error!("Failed to render frame: {error}");
                }
                if tx_authoring_result
                    .send(RenderResult {
                        request_id,
                        frame_hash: 0,
                        output,
                        frame_info,
                    })
                    .is_err()
                {
                    break;
                }
            }
        });

        Self {
            preview_mailbox,
            rx_authoring_result,
            tx_authoring_export,
            rx_authoring_export_result,
            #[cfg(test)]
            temporary_audio_test_control,
            #[cfg(test)]
            atomic_sync_test_control,
            handle: Some(handle),
            export_handle: Some(export_handle),
        }
    }

    /// Publish the latest desired authoring Preview frame. Evaluation and
    /// rasterization both happen on the render worker; Project ownership stays
    /// in `AuthoringProject` and the immutable derived plan is shared across
    /// requests. A successful submission may replace a pending older request;
    /// superseded Preview requests intentionally produce no completion.
    #[expect(
        clippy::too_many_arguments,
        reason = "the asynchronous boundary keeps exact frame, scale, and region explicit"
    )]
    pub fn send_authoring_request(
        &self,
        request_id: RenderRequestId,
        project: Arc<AuthoringProject>,
        plan: Arc<RenderPlan>,
        timeline_id: TimelineId,
        frame_number: i64,
        render_scale: f64,
        region: Option<Region>,
    ) -> bool {
        self.send_authoring_request_at_instance(
            request_id,
            project,
            plan,
            timeline_id,
            None,
            frame_number,
            render_scale,
            region,
        )
    }

    /// Queue an authoring render in a concrete nested placement context.
    #[expect(
        clippy::too_many_arguments,
        reason = "the asynchronous boundary keeps exact frame, viewport, and instance context explicit"
    )]
    pub fn send_authoring_request_at_instance(
        &self,
        request_id: RenderRequestId,
        project: Arc<AuthoringProject>,
        plan: Arc<RenderPlan>,
        timeline_id: TimelineId,
        instance_path: Option<InstancePath>,
        frame_number: i64,
        render_scale: f64,
        region: Option<Region>,
    ) -> bool {
        let submission = self.preview_mailbox.submit_render(AuthoringRenderRequest {
            request_id,
            project,
            plan,
            timeline_id,
            instance_path,
            frame_number,
            render_scale,
            region,
        });
        if submission == PreviewSubmission::Closed {
            log::debug!("Render server is unavailable");
            false
        } else {
            true
        }
    }

    /// Queue one full-resolution authoring frame for PNG export.
    ///
    /// The bounded dedicated worker is intentionally independent from Preview
    /// request coalescing. `false` means the single pending export slot is busy
    /// or the worker is unavailable; callers may retry without losing an
    /// accepted request.
    pub fn send_authoring_png_export_request(
        &self,
        request_id: RenderRequestId,
        project: Arc<AuthoringProject>,
        plan: Arc<RenderPlan>,
        timeline_id: TimelineId,
        frame_number: i64,
        output_path: String,
    ) -> bool {
        self.send_authoring_png_export_request_at_instance(
            request_id,
            project,
            plan,
            timeline_id,
            None,
            frame_number,
            output_path,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "export boundary keeps exact frame and instance context explicit"
    )]
    pub fn send_authoring_png_export_request_at_instance(
        &self,
        request_id: RenderRequestId,
        project: Arc<AuthoringProject>,
        plan: Arc<RenderPlan>,
        timeline_id: TimelineId,
        instance_path: Option<InstancePath>,
        frame_number: i64,
        output_path: String,
    ) -> bool {
        match self
            .tx_authoring_export
            .try_send(AuthoringExportRequest::Png(AuthoringPngExportRequest {
                request_id,
                project,
                plan,
                timeline_id,
                instance_path,
                frame_number,
                output_path,
            })) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                log::debug!("Authoring export worker already has a pending request");
                false
            }
            Err(TrySendError::Disconnected(_)) => {
                log::debug!("Authoring export worker is unavailable");
                false
            }
        }
    }

    /// Queue a full-duration authoring video export.
    ///
    /// Frames are rendered from the immutable Project/RenderPlan snapshot and
    /// streamed to the FFmpeg exporter on the dedicated export worker. Audio
    /// is intentionally absent until the authoring runtime has a real audio
    /// schedule and mixer; this API does not manufacture a legacy graph or a
    /// silent placeholder source.
    pub fn send_authoring_video_export_request(
        &self,
        request_id: RenderRequestId,
        project: Arc<AuthoringProject>,
        plan: Arc<RenderPlan>,
        timeline_id: TimelineId,
        output_path: String,
    ) -> bool {
        self.send_authoring_video_export_request_at_instance(
            request_id,
            project,
            plan,
            timeline_id,
            None,
            output_path,
        )
    }

    /// Queue a full-duration video export in a concrete nested placement.
    pub fn send_authoring_video_export_request_at_instance(
        &self,
        request_id: RenderRequestId,
        project: Arc<AuthoringProject>,
        plan: Arc<RenderPlan>,
        timeline_id: TimelineId,
        instance_path: Option<InstancePath>,
        output_path: String,
    ) -> bool {
        match self
            .tx_authoring_export
            .try_send(AuthoringExportRequest::Video(AuthoringVideoExportRequest {
                request_id,
                project,
                plan,
                timeline_id,
                instance_path,
                output_path,
                #[cfg(test)]
                temporary_audio_test_control: Arc::clone(&self.temporary_audio_test_control),
                #[cfg(test)]
                atomic_sync_test_control: Arc::clone(&self.atomic_sync_test_control),
            })) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                log::debug!("Authoring export worker already has a pending request");
                false
            }
            Err(TrySendError::Disconnected(_)) => {
                log::debug!("Authoring export worker is unavailable");
                false
            }
        }
    }

    /// Poll an authoring Preview completion.
    pub fn poll_authoring_result(&self) -> Result<RenderResult, TryRecvError> {
        self.rx_authoring_result.try_recv()
    }

    /// Poll only authoring export completions. Preview never observes or
    /// discards values from this receiver.
    pub fn poll_authoring_export_result(&self) -> Result<AuthoringExportResult, TryRecvError> {
        self.rx_authoring_export_result.try_recv()
    }

    pub fn set_sharing_context(&self, handle: usize, hwnd: Option<isize>) {
        if !self.preview_mailbox.set_sharing_context(handle, hwnd) {
            log::debug!("Render server is unavailable");
        }
    }

    #[cfg(test)]
    pub(crate) fn fail_temporary_audio_cleanup_attempts(
        &self,
        attempts: usize,
    ) -> Result<(), LibraryError> {
        self.temporary_audio_test_control
            .fail_cleanup_attempts(attempts)
    }

    #[cfg(test)]
    pub(crate) fn fail_temporary_audio_explicit_cleanup(&self) -> Result<(), LibraryError> {
        self.temporary_audio_test_control.fail_explicit_cleanup()
    }

    #[cfg(test)]
    pub(crate) fn temporary_audio_test_observation(
        &self,
    ) -> (Vec<std::path::PathBuf>, usize, usize, usize) {
        self.temporary_audio_test_control.observation()
    }

    #[cfg(test)]
    pub(crate) fn fail_next_atomic_file_sync(&self) -> Result<(), LibraryError> {
        self.atomic_sync_test_control
            .fail_next_sync()
            .map_err(|error| LibraryError::Runtime(error.to_string()))
    }

    #[cfg(test)]
    pub(crate) fn atomic_sync_test_observation(&self) -> (usize, usize) {
        self.atomic_sync_test_control.observation()
    }
}

impl Drop for RenderServer {
    fn drop(&mut self) {
        self.preview_mailbox.shutdown();
        match self
            .tx_authoring_export
            .try_send(AuthoringExportRequest::Shutdown)
        {
            Ok(()) | Err(TrySendError::Disconnected(_)) => {}
            Err(TrySendError::Full(_)) => {
                // Dropping the last sender after this method returns closes
                // the channel once the already queued export is received.
                log::debug!("Authoring export shutdown follows its queued request");
            }
        }
        let mut workers = Vec::with_capacity(2);
        if let Some(handle) = self.handle.take() {
            workers.push(("Render server thread", handle));
        }
        if let Some(handle) = self.export_handle.take() {
            workers.push(("Authoring export thread", handle));
        }
        crate::util::thread::join_in_background("render-shutdown-reaper", workers);
    }
}

#[cfg(test)]
mod export_atomic_tests;

#[cfg(test)]
mod export_source_alias_tests;

#[cfg(test)]
mod export_tests;

#[cfg(test)]
mod tests {
    use super::{RenderRequestId, RenderServer};
    use crate::cache::CacheManager;
    use crate::core::render_plan::RenderPlanCompiler;
    use crate::model::authoring::{
        AuthoringProject, MediaTime, RationalRate, Timeline, TimelineId, TimelineTrack,
        TimelineTrackId, TimelineTrackKind,
    };
    use crate::model::frame::color::Color;
    use crate::model::project::property::PropertyMap;
    use crate::plugin::PluginManager;
    use crate::rendering::renderer::RenderOutput;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn authoring_request_renders_the_selected_nested_timeline() {
        let mut project = AuthoringProject::new(
            "nested authoring Preview",
            8,
            8,
            RationalRate::new(30, 1).unwrap(),
            MediaTime::new(1, 1).unwrap(),
        )
        .unwrap();
        let timeline_id = TimelineId::new();
        let track_id = TimelineTrackId::new();
        project.timelines.insert(
            timeline_id,
            Timeline {
                id: timeline_id,
                name: "Opened nested Timeline".to_string(),
                width: 3,
                height: 2,
                fps: RationalRate::new(24, 1).unwrap(),
                duration: MediaTime::new(1, 1).unwrap(),
                background_color: Color {
                    r: 16,
                    g: 32,
                    b: 64,
                    a: 255,
                },
                color_profile: "sRGB".to_string(),
                track_order: vec![track_id],
                authored_properties: PropertyMap::new(),
                published_parameters: Vec::new(),
            },
        );
        project.tracks.insert(
            track_id,
            TimelineTrack {
                id: track_id,
                timeline_id,
                name: "Nested video".to_string(),
                kind: TimelineTrackKind::AudioVisual,
                authored_properties: PropertyMap::new(),
            },
        );
        let plan = RenderPlanCompiler::compile(&project).unwrap();
        let server = RenderServer::new(
            Arc::new(PluginManager::default()),
            Arc::new(CacheManager::new()),
        );

        assert!(server.send_authoring_request(
            RenderRequestId::new(43),
            Arc::new(project),
            Arc::new(plan),
            timeline_id,
            0,
            1.0,
            None,
        ));
        let rendered = server
            .rx_authoring_result
            .recv_timeout(Duration::from_secs(5))
            .unwrap();

        assert_eq!(rendered.request_id, RenderRequestId::new(43));
        assert_eq!(
            (rendered.frame_info.width, rendered.frame_info.height),
            (3, 2)
        );
        let RenderOutput::Image(image) = rendered.output.unwrap() else {
            panic!("CPU fallback must return an image");
        };
        assert_eq!((image.width, image.height), (3, 2));
    }
}
