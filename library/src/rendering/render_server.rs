use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::thread;
use std::num::NonZeroUsize;
use log::{debug, info, error};
use lru::LruCache;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use std::sync::Arc;

use crate::model::frame::frame::FrameInfo;
use crate::loader::image::Image;
use crate::rendering::skia_renderer::SkiaRenderer;
use crate::service::RenderService;
use crate::plugin::PluginManager;
use crate::cache::SharedCacheManager;
use crate::framing::entity_converters::EntityConverterRegistry;

pub struct RenderServer {
    tx: Sender<RenderRequest>,
    rx_result: Receiver<RenderResult>,
    handle: Option<thread::JoinHandle<()>>,
}

enum RenderRequest {
    Render(FrameInfo),
    Shutdown,
}

pub struct RenderResult {
    pub frame_hash: u64,
    pub image: Image,
    pub frame_info: FrameInfo, // Return frame info to verify content if needed, though hash is mostly enough
}

struct CacheEntry {
    frame_info: FrameInfo,
    image: Image,
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
            let mut cache: LruCache<u64, Vec<CacheEntry>> = LruCache::new(NonZeroUsize::new(50).unwrap());
            
            // Initial renderer
            let mut current_background_color = crate::model::frame::color::Color { r: 0, g: 0, b: 0, a: 0 };
            let renderer = SkiaRenderer::new(1920, 1080, current_background_color.clone());
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
                        let hash = calculate_hash(&frame_info);
                        
                        // Check cache
                        let mut found = None;
                        if let Some(entries) = cache.get(&hash) {
                            for entry in entries {
                                if entry.frame_info == frame_info {
                                    found = Some(entry.image.clone());
                                    break;
                                }
                            }
                        }

                        if let Some(image) = found {
                            let _ = tx_result.send(RenderResult {
                                frame_hash: hash,
                                image,
                                frame_info,
                            });
                            continue;
                        }

                        // Render
                        // Check if renderer size or background color matches
                        if current_width != frame_info.width as u32 || 
                           current_height != frame_info.height as u32 ||
                           current_background_color != frame_info.background_color
                        {
                             current_width = frame_info.width as u32;
                             current_height = frame_info.height as u32;
                             current_background_color = frame_info.background_color.clone();
                             render_service.renderer = SkiaRenderer::new(current_width, current_height, current_background_color.clone());
                        }

                        match render_service.render_from_frame_info(&frame_info) {
                            Ok(image) => {
                                // Cache
                                if !cache.contains(&hash) {
                                    cache.put(hash, Vec::new());
                                }
                                if let Some(bucket) = cache.get_mut(&hash) {
                                    bucket.push(CacheEntry {
                                        frame_info: frame_info.clone(),
                                        image: image.clone(),
                                    });
                                }

                                let _ = tx_result.send(RenderResult {
                                    frame_hash: hash,
                                    image,
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

fn calculate_hash(frame_info: &FrameInfo) -> u64 {
    let mut hasher = DefaultHasher::new();
    // Serialize to bytes using bincode
    let bytes = bincode::serialize(frame_info).unwrap_or_default();
    bytes.hash(&mut hasher);
    hasher.finish()
}

