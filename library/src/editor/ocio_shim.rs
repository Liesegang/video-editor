use libloading::{Library, Symbol};
use log::{error, info};
use std::ffi::{CStr, CString};
use std::sync::{Arc, OnceLock};

static OCIO_LIB: OnceLock<Option<Arc<Library>>> = OnceLock::new();

// Opaque structs
#[repr(C)]
pub struct OcioContext {
    _private: [u8; 0],
}
#[repr(C)]
pub struct OcioProcessor {
    _private: [u8; 0],
}

type FnCreateContext = unsafe extern "C" fn() -> *mut OcioContext;
type FnDestroyContext = unsafe extern "C" fn(*mut OcioContext);
type FnGetNumColorspaces = unsafe extern "C" fn(*mut OcioContext) -> i32;
type FnGetColorspaceName = unsafe extern "C" fn(*mut OcioContext, i32) -> *const i8;
type FnCreateProcessor =
    unsafe extern "C" fn(*mut OcioContext, *const i8, *const i8) -> *mut OcioProcessor;
type FnDestroyProcessor = unsafe extern "C" fn(*mut OcioProcessor);
type FnApplyTransform = unsafe extern "C" fn(*mut OcioProcessor, *mut f32, i32);

pub struct OcioWrapper {
    lib: Arc<Library>,
}

impl OcioWrapper {
    pub fn get() -> Option<Arc<OcioWrapper>> {
        let lib_opt = OCIO_LIB.get_or_init(|| {
            // SAFETY: the library is retained in a process-wide OnceLock for
            // at least as long as every symbol resolved from it.
            unsafe {
                match Library::new("shim.dll") {
                    Ok(lib) => {
                        info!("Loaded shim.dll successfully");
                        Some(Arc::new(lib))
                    }
                    Err(e) => {
                        error!("Failed to load shim.dll: {}", e);
                        None
                    }
                }
            }
        });

        lib_opt
            .as_ref()
            .map(|lib| Arc::new(OcioWrapper { lib: lib.clone() }))
    }

    /// Creates a new context owned by the caller.
    ///
    /// # Safety
    ///
    /// The returned pointer must be passed exactly once to `destroy_context`
    /// from this same wrapper and must not be used afterward.
    pub unsafe fn create_context(&self) -> Option<*mut OcioContext> {
        // SAFETY: the symbol type matches external/shim/shim.cpp, and the
        // backing Library is retained by self for the complete call.
        unsafe {
            let func: Symbol<FnCreateContext> = self.lib.get(b"ocio_create_context").ok()?;
            let ptr = func();
            if ptr.is_null() { None } else { Some(ptr) }
        }
    }

    /// Destroys a context previously created by this wrapper.
    ///
    /// # Safety
    ///
    /// `ctx` must be a live pointer returned by `create_context` from this
    /// wrapper and must not be used or destroyed again after this call.
    pub unsafe fn destroy_context(&self, ctx: *mut OcioContext) {
        // SAFETY: the caller guarantees ownership and validity of ctx; the
        // loaded symbol has the matching C ABI signature.
        unsafe {
            if let Ok(func) = self.lib.get::<FnDestroyContext>(b"ocio_destroy_context") {
                func(ctx);
            }
        }
    }

    /// Returns the number of color spaces in a live context.
    ///
    /// # Safety
    ///
    /// `ctx` must remain a live context pointer for the duration of the call,
    /// with concurrent access synchronized by the caller.
    pub unsafe fn get_num_colorspaces(&self, ctx: *mut OcioContext) -> i32 {
        // SAFETY: the caller guarantees ctx validity and synchronization; the
        // symbol type matches the shim's exported signature.
        unsafe {
            if let Ok(func) = self
                .lib
                .get::<FnGetNumColorspaces>(b"ocio_get_num_colorspaces")
            {
                func(ctx)
            } else {
                0
            }
        }
    }

    /// Copies one color-space name from a live context.
    ///
    /// # Safety
    ///
    /// `ctx` must be live and synchronized, and `index` must be in the range
    /// returned by `get_num_colorspaces` for the same context.
    pub unsafe fn get_colorspace_name(&self, ctx: *mut OcioContext, index: i32) -> Option<String> {
        // SAFETY: the caller guarantees the context and index contract. The
        // returned C string is checked for null and copied before returning.
        unsafe {
            let func: Symbol<FnGetColorspaceName> =
                self.lib.get(b"ocio_get_colorspace_name").ok()?;
            let ptr = func(ctx, index);
            if ptr.is_null() {
                None
            } else {
                CStr::from_ptr(ptr).to_str().ok().map(|s| s.to_string())
            }
        }
    }

    /// Creates a processor owned by the caller.
    ///
    /// # Safety
    ///
    /// `ctx` must be a live, synchronized context. A returned pointer must be
    /// destroyed exactly once with `destroy_processor` from this wrapper.
    pub unsafe fn create_processor(
        &self,
        ctx: *mut OcioContext,
        src: &str,
        dst: &str,
    ) -> Option<*mut OcioProcessor> {
        // SAFETY: ctx validity is guaranteed by the caller, CString keeps both
        // input pointers alive, and the symbol matches the shim C ABI.
        unsafe {
            let func: Symbol<FnCreateProcessor> = self.lib.get(b"ocio_create_processor").ok()?;
            let c_src = CString::new(src).ok()?;
            let c_dst = CString::new(dst).ok()?;
            let ptr = func(ctx, c_src.as_ptr(), c_dst.as_ptr());
            if ptr.is_null() { None } else { Some(ptr) }
        }
    }

    /// Destroys a processor previously created by this wrapper.
    ///
    /// # Safety
    ///
    /// `proc` must be a live pointer returned by `create_processor` from this
    /// wrapper and must not be used or destroyed again afterward.
    pub unsafe fn destroy_processor(&self, proc: *mut OcioProcessor) {
        // SAFETY: the caller guarantees ownership and validity of proc; the
        // resolved symbol has the matching C ABI signature.
        unsafe {
            if let Ok(func) = self
                .lib
                .get::<FnDestroyProcessor>(b"ocio_destroy_processor")
            {
                func(proc);
            }
        }
    }

    /// Applies a processor to a contiguous RGBA-f32 pixel buffer in place.
    ///
    /// # Safety
    ///
    /// `proc` must be live and exclusively synchronized for this call. The
    /// wrapper derives the pointer and bounded pixel count from `pixels`.
    pub unsafe fn apply_transform(&self, proc: *mut OcioProcessor, pixels: &mut [f32]) {
        let pixel_count = pixels.len() / 4;
        let Ok(pixel_count) = i32::try_from(pixel_count) else {
            return;
        };
        // SAFETY: proc validity and synchronization are guaranteed by the
        // caller; pixels supplies a valid writable pointer for pixel_count RGBA
        // elements, and the symbol signature matches the shim.
        unsafe {
            if let Ok(func) = self.lib.get::<FnApplyTransform>(b"ocio_apply_transform") {
                func(proc, pixels.as_mut_ptr(), pixel_count);
            }
        }
    }
}
