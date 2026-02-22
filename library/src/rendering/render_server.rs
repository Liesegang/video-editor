use log::error;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread;
use uuid::Uuid;

use crate::cache::SharedCacheManager;
use crate::pipeline::engine::EvalEngine;
use crate::plugin::PluginManager;
use crate::rendering::renderer::{RenderOutput, Renderer};
use crate::rendering::skia_renderer::SkiaRenderer;
use crate::runtime::frame::Region;

pub struct RenderServer {
    tx: Sender<RenderRequest>,
    rx_result: Receiver<RenderResult>,
    #[allow(dead_code)]
    handle: Option<thread::JoinHandle<()>>,
}

/// Parameters for composition-based rendering via EvalEngine.
pub struct CompositionRenderParams {
    pub project: crate::project::project::Project,
    pub composition_id: Uuid,
    pub frame_number: u64,
    pub render_scale: f64,
    pub region: Option<Region>,
}

enum RenderRequest {
    RenderComposition(CompositionRenderParams),
    SetSharingContext(usize, Option<isize>),
    #[allow(dead_code)]
    Shutdown,
}

pub struct RenderResult {
    pub(crate) frame_hash: u64,
    pub output: RenderOutput,
    pub region: Option<Region>,
}

impl RenderServer {
    pub fn new(plugin_manager: Arc<PluginManager>, cache_manager: SharedCacheManager) -> Self {
        let (tx, rx) = channel::<RenderRequest>();
        let (tx_result, rx_result) = channel::<RenderResult>();

        let handle = thread::spawn(move || {
            let mut current_background_color = crate::runtime::color::Color {
                r: 0,
                g: 0,
                b: 0,
                a: 0,
            };
            let mut renderer =
                SkiaRenderer::new(1920, 1080, current_background_color.clone(), true, None);
            let mut current_width: u32 = 1920;
            let mut current_height: u32 = 1080;

            let eval_engine = EvalEngine::with_default_evaluators();

            loop {
                let mut req = match rx.recv() {
                    Ok(r) => r,
                    Err(_) => break,
                };

                // Drain accumulated requests to jump to the latest state
                while let Ok(next_req) = rx.try_recv() {
                    match next_req {
                        RenderRequest::Shutdown => {
                            req = RenderRequest::Shutdown;
                            break;
                        }
                        RenderRequest::SetSharingContext(handle, hwnd) => {
                            renderer.set_sharing_context(handle, hwnd);
                            if let RenderRequest::SetSharingContext(_, _) = req {
                                req = RenderRequest::SetSharingContext(handle, hwnd);
                            }
                        }
                        RenderRequest::RenderComposition(_) => {
                            if let RenderRequest::SetSharingContext(h, w) = req {
                                renderer.set_sharing_context(h, w);
                            }
                            req = next_req;
                        }
                    }
                }

                match req {
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
                            current_background_color = bg_color;

                            let old_context = renderer.take_context();
                            renderer = SkiaRenderer::new(
                                current_width,
                                current_height,
                                current_background_color.clone(),
                                true,
                                old_context,
                            );
                        }

                        renderer.clear().ok();

                        let property_evaluators = plugin_manager.get_property_evaluators();

                        match eval_engine.evaluate_composition(
                            &params.project,
                            composition,
                            &plugin_manager,
                            &mut renderer,
                            &cache_manager,
                            property_evaluators,
                            params.frame_number,
                            params.render_scale,
                            params.region.clone(),
                        ) {
                            Ok(output) => {
                                let _ = tx_result.send(RenderResult {
                                    frame_hash: 0,
                                    output,
                                    region: params.region,
                                });
                            }
                            Err(e) => {
                                error!("EvalEngine render failed: {}", e);
                            }
                        }
                    }
                    RenderRequest::SetSharingContext(handle, hwnd) => {
                        renderer.set_sharing_context(handle, hwnd);
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
