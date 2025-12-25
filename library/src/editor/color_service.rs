use crate::editor::ocio_shim::{OcioContext, OcioProcessor as ShimProcessor, OcioWrapper};
use log::{error, info, warn};
use std::sync::{Arc, OnceLock};

// Global singleton for the OCIO context to avoid reloading config repeatedly
static OCIO_CONTEXT: OnceLock<Option<GlobalContext>> = OnceLock::new();

struct GlobalContext {
    wrapper: Arc<OcioWrapper>,
    context: *mut OcioContext,
}

unsafe impl Send for GlobalContext {}
unsafe impl Sync for GlobalContext {}

impl Drop for GlobalContext {
    fn drop(&mut self) {
        unsafe {
            self.wrapper.destroy_context(self.context);
        }
    }
}

pub struct ColorSpaceManager;

impl ColorSpaceManager {
    fn get_context() -> Option<&'static GlobalContext> {
        OCIO_CONTEXT
            .get_or_init(|| {
                if let Some(wrapper) = OcioWrapper::get() {
                    unsafe {
                        if let Some(ctx) = wrapper.create_context() {
                            info!("OCIO Context created successfully.");
                            return Some(GlobalContext {
                                wrapper,
                                context: ctx,
                            });
                        } else {
                            error!("Failed to create OCIO Context.");
                        }
                    }
                } else {
                    warn!("OCIO Wrapper not available (shim.dll missing?).");
                }
                None
            })
            .as_ref()
    }

    pub fn get_available_colorspaces() -> Vec<String> {
        let mut names = Vec::new();
        if let Some(gctx) = Self::get_context() {
            unsafe {
                let count = gctx.wrapper.get_num_colorspaces(gctx.context);
                for i in 0..count {
                    if let Some(name) = gctx.wrapper.get_colorspace_name(gctx.context, i) {
                        names.push(name);
                    }
                }
            }
        }
        names
    }

    pub fn create_processor(src: &str, dst: &str) -> Option<OcioProcessor> {
        let gctx = Self::get_context()?;
        unsafe {
            let ptr = gctx.wrapper.create_processor(gctx.context, src, dst);
            if let Some(p) = ptr {
                Some(OcioProcessor {
                    ptr: p,
                    wrapper: gctx.wrapper.clone(),
                })
            } else {
                error!("Failed to create processor for {} -> {}", src, dst);
                None
            }
        }
    }
}

pub struct OcioProcessor {
    ptr: *mut ShimProcessor,
    wrapper: Arc<OcioWrapper>,
}

unsafe impl Send for OcioProcessor {}
unsafe impl Sync for OcioProcessor {}

impl Drop for OcioProcessor {
    fn drop(&mut self) {
        unsafe {
            self.wrapper.destroy_processor(self.ptr);
        }
    }
}

impl OcioProcessor {
    pub fn apply_rgba(&self, pixels: &[u8]) -> Vec<u8> {
        // Convert u8 to f32 (0.0-1.0)
        // Optimized for performance?
        // We could maybe use SIMD or parallel iterators, but for now simple loop.
        let _pixel_count = pixels.len() / 4;
        let mut floats: Vec<f32> = Vec::with_capacity(pixels.len());

        for &b in pixels {
            floats.push(b as f32 / 255.0);
        }

        // Apply transform in place
        unsafe {
            self.wrapper.apply_transform(self.ptr, &mut floats);
        }

        // Convert back to u8 (clamp and scale)
        let mut out_pixels = Vec::with_capacity(pixels.len());
        for f in floats {
            let val = (f * 255.0).round().clamp(0.0, 255.0) as u8;
            out_pixels.push(val);
        }

        out_pixels
    }
}
