use log::error;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread;

use crate::cache::SharedCacheManager;
use crate::editor::RenderService;
use crate::model::frame::Image;
use crate::model::frame::frame::FrameInfo;
use crate::plugin::PluginManager;
use crate::rendering::skia_renderer::SkiaRenderer;

pub struct RenderServer {
    tx: Sender<RenderRequest>,
    rx_result: Receiver<RenderResult>,
    #[allow(dead_code)]
    handle: Option<thread::JoinHandle<()>>,
}

enum RenderRequest {
    Render(FrameInfo),
    RenderTrinity {
        project: Box<crate::model::Project>,
        composition_id: uuid::Uuid,
        time: f64,
        render_scale: f64,
        region: Option<crate::model::frame::frame::Region>,
    },
    SetSharingContext(usize, Option<isize>),
    #[allow(dead_code)]
    Shutdown,
}

use crate::rendering::renderer::RenderOutput;
use crate::rendering::renderer::Renderer;

pub struct RenderResult {
    pub frame_hash: u64,
    pub output: RenderOutput,
    pub frame_info: Option<FrameInfo>, // Made Option to handle Trinity case where we don't have FrameInfo initially
}

impl RenderServer {
    pub fn new(plugin_manager: Arc<PluginManager>, cache_manager: SharedCacheManager) -> Self {
        let (tx, rx) = channel::<RenderRequest>();
        let (tx_result, rx_result) = channel::<RenderResult>();

        let handle = thread::spawn(move || {
            let mut cache: LruCache<FrameInfo, Vec<u8>> =
                LruCache::new(NonZeroUsize::new(50).unwrap());

            // Initial renderer
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
            let mut current_width = 1920;
            let mut current_height = 1080;

            let mut last_trinity_cache: Option<(
                Box<crate::model::Project>,
                uuid::Uuid,
                f64,
                f64,
                Option<crate::model::frame::frame::Region>,
                RenderOutput,
            )> = None;

            let mut render_service = RenderService::new(renderer, plugin_manager, cache_manager);

            loop {
                // Get the next request (blocking)
                let mut req = match rx.recv() {
                    Ok(r) => r,
                    Err(_) => break,
                };

                // Drain any accumulated requests to jump to the latest state
                // This prevents the renderer from falling behind during rapid updates (e.g. dragging sliders)
                while let Ok(next_req) = rx.try_recv() {
                    match next_req {
                        RenderRequest::Shutdown => {
                            req = RenderRequest::Shutdown;
                            break;
                        }
                        RenderRequest::SetSharingContext(handle, hwnd) => {
                            render_service.renderer.set_sharing_context(handle, hwnd);
                            // We handled it immediately.
                            // We do NOT update `req` because `req` might be a pending Render request we still want to process.
                            // If `req` was also SetSharing, it's fine, we processed it (or will process it if we don't handle `req` variants differently).
                            // Actually, if `req` matches variants in the main loop, we should check `req`'s type.
                            // But simplify: just apply sharing here.
                            // NOTE: If `req` was SetSharingContext(old_handle), and we just set(new_handle), then `req` is outdated.
                            // But executing `req` (SetSharing) again with old handle is bad.
                            // So if `req` is SetSharing, we shound discard/update it?
                            if let RenderRequest::SetSharingContext(_, _) = req {
                                // req is an old sharing request, superseded or just done.
                                // Since we processed next_req (new handle), we should drop old req?
                                // But what if `req` was Render? We keep it.
                                // So:
                                // If req is SetSharing, update it to something innocuous? Or just let it run?
                                // Running SetSharing(old) after SetSharing(new) reverts the change!
                                // So we MUST NOT let `req` run if it is an old SetSharing.
                                // We can update `req` to `SetSharingContext(handle)` (the new one)?
                                // Then main loop runs it again? That's wasteful but safe-ish (idempotent check inside).
                                req = RenderRequest::SetSharingContext(handle, hwnd);
                            }
                        }
                        RenderRequest::RenderTrinity { .. } => {
                            // Trinity Request supersedes previous render requests
                            if let RenderRequest::SetSharingContext(h, w) = req {
                                render_service.renderer.set_sharing_context(h, w);
                                req = next_req;
                            } else {
                                req = next_req;
                            }
                        }
                        RenderRequest::Render(_) => {
                            // New render request.
                            if let RenderRequest::SetSharingContext(h, w) = req {
                                // Previous `req` was sharing. We MUST execute it before switching to new Render.
                                // But we can't execute it here easily without potentially executing it TWICE if `SetSharingContext` case above also ran?
                                // Wait, `SetSharingContext` case only runs if `next_req` is SetSharing.
                                // Here `next_req` is Render.
                                // `req` is SetSharing.
                                // So `SetSharing` is pending.
                                // We should apply it now?
                                render_service.renderer.set_sharing_context(h, w);
                                req = next_req; // Now we can switch to new Render.
                            } else {
                                // req was Render or Shutdown.
                                req = next_req;
                            }
                        }
                    }
                }

                match req {
                    RenderRequest::Render(frame_info) => {
                        let render_scale = frame_info.render_scale.into_inner();

                        let (target_width, target_height) = if let Some(region) = &frame_info.region
                        {
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

                        // Check cache
                        if let Some(cached_image_data) = cache.get(&frame_info) {
                            let _ = tx_result.send(RenderResult {
                                frame_hash: 0, // Hash is no longer used/needed for identification in the same way, or we can compute a cheap hash if needed for Result
                                output: RenderOutput::Image(Image::new(
                                    target_width,
                                    target_height,
                                    cached_image_data.clone(),
                                )),
                                frame_info: Some(frame_info),
                            });
                            continue;
                        }

                        // Render
                        // Check if renderer size or background color matches

                        if current_width != target_width
                            || current_height != target_height
                            || current_background_color != frame_info.background_color
                        {
                            current_width = target_width;
                            current_height = target_height;
                            current_background_color = frame_info.background_color.clone();

                            // Reuse existing context to avoid EventLoop creation issues
                            let old_context = render_service.renderer.take_context();

                            render_service.renderer = SkiaRenderer::new(
                                current_width,
                                current_height,
                                current_background_color.clone(),
                                true,
                                old_context,
                                None, // Added 6th argument
                            );
                        }

                        match render_service.render_from_frame_info(&frame_info) {
                            Ok(output) => {
                                // Cache if image
                                if let RenderOutput::Image(ref img) = output {
                                    cache.put(frame_info.clone(), img.data.clone());
                                }

                                let _ = tx_result.send(RenderResult {
                                    frame_hash: 0,
                                    output,
                                    frame_info: Some(frame_info),
                                });
                            }
                            Err(e) => {
                                error!("Failed to render frame: {}", e);
                            }
                        }
                    }
                    RenderRequest::RenderTrinity {
                        project,
                        composition_id,
                        time,
                        render_scale,
                        region,
                    } => {
                        log::debug!("RenderServer: Handling Trinity Request for time={}", time);
                        // Check Cache
                        if let Some((c_proj, c_cid, c_time, c_scale, c_region, c_out)) =
                            &last_trinity_cache
                        {
                            if *c_proj == project
                                && *c_cid == composition_id
                                && (*c_time - time).abs() < f64::EPSILON
                                && (*c_scale - render_scale).abs() < f64::EPSILON
                                && *c_region == region
                            {
                                let _ = tx_result.send(RenderResult {
                                    frame_hash: 0,
                                    output: c_out.clone(), // RenderOutput needs to be Clone (it is)
                                    frame_info: None,
                                });
                                continue;
                            }
                        }

                        // Trinity Rendering path
                        if let Some(comp) =
                            project.compositions.iter().find(|c| c.id == composition_id)
                        {
                            let target_width = (comp.width as f64 * render_scale).round() as u32;
                            let target_height = (comp.height as f64 * render_scale).round() as u32;

                            // Ensure Context Size
                            if current_width != target_width || current_height != target_height {
                                current_width = target_width;
                                current_height = target_height;

                                let old_context = render_service.renderer.take_context();
                                render_service.renderer = SkiaRenderer::new(
                                    current_width,
                                    current_height,
                                    crate::model::frame::color::Color {
                                        r: 0,
                                        g: 0,
                                        b: 0,
                                        a: 0,
                                    },
                                    true,
                                    old_context,
                                    None,
                                );
                            }

                            match render_service
                                .renderer
                                .render_composite(&project, comp, time)
                            {
                                Ok(output) => {
                                    // Update Cache
                                    last_trinity_cache = Some((
                                        project,
                                        composition_id,
                                        time,
                                        render_scale,
                                        region,
                                        output.clone(),
                                    ));

                                    let _ = tx_result.send(RenderResult {
                                        frame_hash: 0,
                                        output,
                                        frame_info: None,
                                    });
                                }
                                Err(e) => {
                                    error!("Trinity Render Failed: {}", e);
                                }
                            }
                        }
                    }
                    RenderRequest::SetSharingContext(handle, hwnd) => {
                        render_service.renderer.set_sharing_context(handle, hwnd);
                    }
                    RenderRequest::Shutdown => break,
                }
            }
        });

        RenderServer {
            tx,
            rx_result,
            handle: Some(handle),
        }
    }

    pub fn send_request(&self, frame_info: FrameInfo) {
        let _ = self.tx.send(RenderRequest::Render(frame_info));
    }

    pub fn send_trinity_request(
        &self,
        project: crate::model::Project,
        composition_id: uuid::Uuid,
        time: f64,
        render_scale: f64,
        region: Option<crate::model::frame::frame::Region>,
    ) {
        let _ = self.tx.send(RenderRequest::RenderTrinity {
            project: Box::new(project),
            composition_id,
            time,
            render_scale,
            region,
        });
    }

    pub fn poll_result(&self) -> Result<RenderResult, TryRecvError> {
        self.rx_result.try_recv()
    }

    pub fn set_sharing_context(&self, handle: usize, hwnd: Option<isize>) {
        let _ = self.tx.send(RenderRequest::SetSharingContext(handle, hwnd));
    }
}
