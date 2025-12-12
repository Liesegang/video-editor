use log::error;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread;

use crate::cache::SharedCacheManager;
use crate::framing::entity_converters::EntityConverterRegistry;
use crate::loader::image::Image;
use crate::model::frame::frame::FrameInfo;
use crate::plugin::PluginManager;
use crate::rendering::skia_renderer::SkiaRenderer;
use crate::service::RenderService;

pub struct RenderServer {
    tx: Sender<RenderRequest>,
    rx_result: Receiver<RenderResult>,
    #[allow(dead_code)]
    handle: Option<thread::JoinHandle<()>>,
}

enum RenderRequest {
    Render(FrameInfo),
    #[allow(dead_code)]
    Shutdown,
}

use crate::rendering::renderer::RenderOutput;

pub struct RenderResult {
    pub frame_hash: u64,
    pub output: RenderOutput,
    pub frame_info: FrameInfo, // Return frame info to verify content if needed, though hash is mostly enough
}

impl RenderServer {
    pub fn new(
        plugin_manager: Arc<PluginManager>,
        cache_manager: SharedCacheManager,
        entity_converter_registry: Arc<EntityConverterRegistry>,
    ) -> Self {
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
            let renderer = SkiaRenderer::new(1920, 1080, current_background_color.clone(), true, None);
            let mut current_width = 1920;
            let mut current_height = 1080;

            let mut render_service = RenderService::new(
                renderer,
                plugin_manager,
                cache_manager,
                entity_converter_registry,
            );

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
                        RenderRequest::Render(_) => {
                            req = next_req;
                        }
                    }
                }

                match req {
                    RenderRequest::Render(frame_info) => {
                        // Check cache
                        if let Some(cached_image_data) = cache.get(&frame_info) {
                            let _ = tx_result.send(RenderResult {
                                frame_hash: 0, // Hash is no longer used/needed for identification in the same way, or we can compute a cheap hash if needed for Result
                                output: RenderOutput::Image(Image::new(
                                    frame_info.width as u32,
                                    frame_info.height as u32,
                                    cached_image_data.clone(),
                                )),
                                frame_info,
                            });
                            continue;
                        }

                        // Render
                        // Check if renderer size or background color matches
                        if current_width != frame_info.width as u32
                            || current_height != frame_info.height as u32
                            || current_background_color != frame_info.background_color
                        {
                            current_width = frame_info.width as u32;
                            current_height = frame_info.height as u32;
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

    pub fn poll_result(&self) -> Result<RenderResult, TryRecvError> {
        self.rx_result.try_recv()
    }
}
