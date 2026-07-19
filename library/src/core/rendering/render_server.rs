use log::error;
use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::thread;

use crate::cache::SharedCacheManager;
use crate::editor::RenderService;
use crate::error::LibraryError;
use crate::model::frame::Image;
use crate::model::frame::frame::FrameInfo;
use crate::plugin::PluginManager;
use crate::rendering::renderer::{RenderOutput, Renderer};
use crate::rendering::skia_renderer::SkiaRenderer;

pub struct RenderServer {
    tx: Sender<RenderRequest>,
    rx_result: Receiver<RenderResult>,
    handle: Option<thread::JoinHandle<()>>,
}

enum RenderRequest {
    Render(FrameInfo),
    SetSharingContext(usize, Option<isize>),
    Shutdown,
}

pub struct RenderResult {
    pub frame_hash: u64,
    pub output: Result<RenderOutput, LibraryError>,
    pub frame_info: FrameInfo,
}

impl RenderServer {
    pub fn new(plugin_manager: Arc<PluginManager>, cache_manager: SharedCacheManager) -> Self {
        let (tx, rx) = channel::<RenderRequest>();
        let (tx_result, rx_result) = channel::<RenderResult>();

        let handle = thread::spawn(move || {
            let cache_capacity = NonZeroUsize::new(50).unwrap_or(NonZeroUsize::MIN);
            let mut cache: LruCache<FrameInfo, Vec<u8>> = LruCache::new(cache_capacity);
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

            'server: while let Ok(first_request) = rx.recv() {
                let mut pending_render = None;

                for request in std::iter::once(first_request).chain(rx.try_iter()) {
                    match request {
                        RenderRequest::Render(frame_info) => pending_render = Some(frame_info),
                        RenderRequest::SetSharingContext(handle, hwnd) => {
                            if let Some(render_service) = render_service.as_mut()
                                && let Err(error) =
                                    render_service.renderer.set_sharing_context(handle, hwnd)
                            {
                                error!("Failed to set render sharing context: {error}");
                            }
                        }
                        RenderRequest::Shutdown => break 'server,
                    }
                }

                let Some(frame_info) = pending_render else {
                    continue;
                };
                let Some(render_service) = render_service.as_mut() else {
                    let error =
                        LibraryError::Render(initialization_error.clone().unwrap_or_else(|| {
                            "Preview renderer is unavailable without an error message".to_string()
                        }));
                    if tx_result
                        .send(RenderResult {
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

                if let Some(cached_image_data) = cache.get(&frame_info) {
                    if tx_result
                        .send(RenderResult {
                            frame_hash: 0,
                            output: Ok(RenderOutput::Image(Image::new(
                                target_width,
                                target_height,
                                cached_image_data.clone(),
                            ))),
                            frame_info,
                        })
                        .is_err()
                    {
                        break;
                    }
                    continue;
                }

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
                            if tx_result
                                .send(RenderResult {
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

                let output = render_service.render_from_frame_info(&frame_info);
                if let Ok(RenderOutput::Image(image)) = &output {
                    cache.put(frame_info.clone(), image.data.clone());
                }
                if let Err(error) = &output {
                    error!("Failed to render frame: {error}");
                }
                if tx_result
                    .send(RenderResult {
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
            tx,
            rx_result,
            handle: Some(handle),
        }
    }

    pub fn send_request(&self, frame_info: FrameInfo) {
        if let Err(error) = self.tx.send(RenderRequest::Render(frame_info)) {
            log::debug!("Render server is unavailable: {error}");
        }
    }

    pub fn poll_result(&self) -> Result<RenderResult, TryRecvError> {
        self.rx_result.try_recv()
    }

    pub fn set_sharing_context(&self, handle: usize, hwnd: Option<isize>) {
        if let Err(error) = self.tx.send(RenderRequest::SetSharingContext(handle, hwnd)) {
            log::debug!("Render server is unavailable: {error}");
        }
    }
}

impl Drop for RenderServer {
    fn drop(&mut self) {
        drop(self.tx.send(RenderRequest::Shutdown));
        if let Some(handle) = self.handle.take()
            && handle.join().is_err()
        {
            error!("Render server thread panicked during shutdown");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RenderServer;
    use crate::cache::CacheManager;
    use crate::model::frame::color::Color;
    use crate::model::frame::frame::FrameInfo;
    use crate::plugin::PluginManager;
    use crate::rendering::renderer::RenderOutput;
    use ordered_float::OrderedFloat;
    use std::sync::Arc;
    use std::time::Duration;

    fn empty_frame(width: u64, height: u64) -> FrameInfo {
        FrameInfo {
            width,
            height,
            background_color: Color::black(),
            color_profile: "sRGB".to_string(),
            render_scale: OrderedFloat(1.0),
            now_time: OrderedFloat(0.0),
            region: None,
            items: Vec::new(),
        }
    }

    #[test]
    fn resize_error_is_returned_and_the_next_valid_frame_recovers() {
        let server = RenderServer::new(
            Arc::new(PluginManager::default()),
            Arc::new(CacheManager::new()),
        );
        server.send_request(empty_frame(0, 0));
        let failed = server
            .rx_result
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(failed.output.is_err());

        server.send_request(empty_frame(2, 2));
        let recovered = server
            .rx_result
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        let RenderOutput::Image(image) = recovered.output.unwrap() else {
            panic!("CPU fallback must return an image");
        };
        assert_eq!((image.width, image.height), (2, 2));
    }
}
