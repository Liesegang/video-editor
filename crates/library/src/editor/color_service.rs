use crate::editor::ocio_shim::{OcioContext, OcioProcessor as ShimProcessor, OcioWrapper};
use log::{error, info, warn};
use std::sync::{Arc, Mutex, OnceLock};

// Global singleton for the OCIO context to avoid reloading config repeatedly
static OCIO_CONTEXT: OnceLock<Option<GlobalContext>> = OnceLock::new();

struct GlobalContext {
    wrapper: Arc<OcioWrapper>,
    context: Mutex<ContextHandle>,
}

struct ContextHandle(*mut OcioContext);

// SAFETY: ContextHandle owns one opaque shim allocation. Rust never
// dereferences the pointer, and GlobalContext serializes every FFI access and
// destruction through its Mutex.
unsafe impl Send for ContextHandle {}

impl Drop for GlobalContext {
    fn drop(&mut self) {
        let context = self
            .context
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: this handle was returned by create_context, remains uniquely
        // owned here, and Drop invokes its matching destructor exactly once.
        unsafe {
            self.wrapper.destroy_context(context.0);
        }
    }
}

pub struct ColorSpaceManager;

impl ColorSpaceManager {
    fn get_context() -> Option<&'static GlobalContext> {
        OCIO_CONTEXT
            .get_or_init(|| {
                if let Some(wrapper) = OcioWrapper::get() {
                    // SAFETY: OcioWrapper resolves the matching shim symbol and
                    // returns ownership only for a non-null context pointer.
                    unsafe {
                        if let Some(ctx) = wrapper.create_context() {
                            info!("OCIO Context created successfully.");
                            return Some(GlobalContext {
                                wrapper,
                                context: Mutex::new(ContextHandle(ctx)),
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
            let context = gctx
                .context
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            // SAFETY: context is a live shim handle and its Mutex is held for
            // the complete sequence of calls, including C string copying.
            unsafe {
                let count = gctx.wrapper.get_num_colorspaces(context.0);
                for i in 0..count {
                    if let Some(name) = gctx.wrapper.get_colorspace_name(context.0, i) {
                        names.push(name);
                    }
                }
            }
        }
        names
    }

    pub fn create_processor(src: &str, dst: &str) -> Option<OcioProcessor> {
        let gctx = Self::get_context()?;
        let context = gctx
            .context
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: context is live and locked; OcioWrapper converts both Rust
        // strings to NUL-terminated inputs for the duration of the shim call.
        unsafe {
            let ptr = gctx.wrapper.create_processor(context.0, src, dst);
            if let Some(p) = ptr {
                Some(OcioProcessor {
                    handle: Mutex::new(ProcessorHandle(p)),
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
    handle: Mutex<ProcessorHandle>,
    wrapper: Arc<OcioWrapper>,
}

struct ProcessorHandle(*mut ShimProcessor);

// SAFETY: ProcessorHandle owns one opaque shim allocation. The containing
// OcioProcessor serializes all transform calls and destruction through its
// Mutex, and Rust never dereferences the pointer.
unsafe impl Send for ProcessorHandle {}

impl Drop for OcioProcessor {
    fn drop(&mut self) {
        let handle = self
            .handle
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: this is the live pointer returned by create_processor and its
        // matching destructor is called exactly once while exclusively owned.
        unsafe {
            self.wrapper.destroy_processor(handle.0);
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
        let handle = self
            .handle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: handle is live and locked for the whole call; `floats` owns a
        // writable contiguous buffer whose length is supplied to the wrapper.
        unsafe {
            self.wrapper.apply_transform(handle.0, &mut floats);
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
