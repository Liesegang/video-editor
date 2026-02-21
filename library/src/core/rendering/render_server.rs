use log::error;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread;
use uuid::Uuid;

use super::render_service::RenderService;
use crate::cache::SharedCacheManager;
use crate::core::evaluation::engine::EvalEngine;
use crate::model::frame::Image;
use crate::model::frame::frame::FrameInfo;
use crate::model::project::project::Project;
use crate::plugin::PluginManager;
use crate::rendering::skia_renderer::SkiaRenderer;

pub struct RenderServer {
    tx: Sender<RenderRequest>,
    rx_result: Receiver<RenderResult>,
    #[allow(dead_code)]
    handle: Option<thread::JoinHandle<()>>,
}

/// Parameters for composition-based rendering via EvalEngine.
pub struct CompositionRenderParams {
    pub project: Project,
    pub composition_id: Uuid,
    pub frame_number: u64,
    pub render_scale: f64,
    pub region: Option<crate::model::frame::frame::Region>,
}

enum RenderRequest {
    Render(FrameInfo),
    RenderComposition(CompositionRenderParams),
    SetSharingContext(usize, Option<isize>),
    #[allow(dead_code)]
    Shutdown,
}

use crate::rendering::renderer::RenderOutput;
use crate::rendering::renderer::Renderer;

pub struct RenderResult {
    pub(crate) frame_hash: u64,
    pub output: RenderOutput,
    pub frame_info: FrameInfo, // Return frame info to verify content if needed, though hash is mostly enough
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
            let renderer =
                SkiaRenderer::new(1920, 1080, current_background_color.clone(), true, None);
            let mut current_width = 1920;
            let mut current_height = 1080;

            let mut render_service =
                RenderService::new(renderer, plugin_manager.clone(), cache_manager.clone());

            // Create the pull-based evaluation engine
            let eval_engine = EvalEngine::with_default_evaluators();

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
                            if let RenderRequest::SetSharingContext(_, _) = req {
                                req = RenderRequest::SetSharingContext(handle, hwnd);
                            }
                        }
                        RenderRequest::Render(_) | RenderRequest::RenderComposition(_) => {
                            if let RenderRequest::SetSharingContext(h, w) = req {
                                render_service.renderer.set_sharing_context(h, w);
                                req = next_req;
                            } else {
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
                                frame_hash: 0,
                                output: RenderOutput::Image(Image::new(
                                    target_width,
                                    target_height,
                                    cached_image_data.clone(),
                                )),
                                frame_info,
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
                                    frame_info,
                                });
                            }
                            Err(e) => {
                                error!("Failed to render frame: {}", e);
                            }
                        }
                    }
                    RenderRequest::RenderComposition(params) => {
                        let composition =
                            match params.project.get_composition(params.composition_id) {
                                Some(comp) => comp,
                                None => {
                                    error!("Composition {} not found", params.composition_id);
                                    continue;
                                }
                            };

                        let render_scale = params.render_scale;
                        let (target_width, target_height) = if let Some(region) = &params.region {
                            (
                                (region.width * render_scale).round() as u32,
                                (region.height * render_scale).round() as u32,
                            )
                        } else {
                            (
                                (composition.width as f64 * render_scale).round() as u32,
                                (composition.height as f64 * render_scale).round() as u32,
                            )
                        };

                        let bg_color = composition.background_color.clone();

                        if current_width != target_width
                            || current_height != target_height
                            || current_background_color != bg_color
                        {
                            current_width = target_width;
                            current_height = target_height;
                            current_background_color = bg_color.clone();

                            let old_context = render_service.renderer.take_context();
                            render_service.renderer = SkiaRenderer::new(
                                current_width,
                                current_height,
                                current_background_color.clone(),
                                true,
                                old_context,
                            );
                        }

                        render_service.renderer.clear().ok();

                        let property_evaluators = plugin_manager.get_property_evaluators();

                        match eval_engine.evaluate_composition(
                            &params.project,
                            composition,
                            &plugin_manager,
                            &mut render_service.renderer,
                            &cache_manager,
                            property_evaluators,
                            params.frame_number,
                            params.render_scale,
                            params.region.clone(),
                        ) {
                            Ok(output) => {
                                // Build a minimal FrameInfo for the result
                                let frame_info = FrameInfo {
                                    width: composition.width,
                                    height: composition.height,
                                    background_color: bg_color,
                                    color_profile: String::new(),
                                    render_scale: ordered_float::OrderedFloat(params.render_scale),
                                    now_time: ordered_float::OrderedFloat(
                                        params.frame_number as f64 / composition.fps,
                                    ),
                                    region: params.region,
                                    objects: vec![],
                                };

                                let _ = tx_result.send(RenderResult {
                                    frame_hash: 0,
                                    output,
                                    frame_info,
                                });
                            }
                            Err(e) => {
                                error!("EvalEngine render failed: {}", e);
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

    /// Send a composition render request using the pull-based EvalEngine.
    pub fn send_composition_request(&self, params: CompositionRenderParams) {
        let _ = self.tx.send(RenderRequest::RenderComposition(params));
    }

    pub fn poll_result(&self) -> Result<RenderResult, TryRecvError> {
        self.rx_result.try_recv()
    }

    pub fn set_sharing_context(&self, handle: usize, hwnd: Option<isize>) {
        let _ = self.tx.send(RenderRequest::SetSharingContext(handle, hwnd));
    }
}
