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
        let lib_opt = OCIO_LIB.get_or_init(|| unsafe {
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
        });

        lib_opt
            .as_ref()
            .map(|lib| Arc::new(OcioWrapper { lib: lib.clone() }))
    }

    pub unsafe fn create_context(&self) -> Option<*mut OcioContext> {
        unsafe {
            let func: Symbol<FnCreateContext> = self.lib.get(b"ocio_create_context").ok()?;
            let ptr = func();
            if ptr.is_null() { None } else { Some(ptr) }
        }
    }

    pub unsafe fn destroy_context(&self, ctx: *mut OcioContext) {
        unsafe {
            if let Ok(func) = self.lib.get::<FnDestroyContext>(b"ocio_destroy_context") {
                func(ctx);
            }
        }
    }

    pub unsafe fn get_num_colorspaces(&self, ctx: *mut OcioContext) -> i32 {
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

    pub unsafe fn get_colorspace_name(&self, ctx: *mut OcioContext, index: i32) -> Option<String> {
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

    pub unsafe fn create_processor(
        &self,
        ctx: *mut OcioContext,
        src: &str,
        dst: &str,
    ) -> Option<*mut OcioProcessor> {
        unsafe {
            let func: Symbol<FnCreateProcessor> = self.lib.get(b"ocio_create_processor").ok()?;
            let c_src = CString::new(src).ok()?;
            let c_dst = CString::new(dst).ok()?;
            let ptr = func(ctx, c_src.as_ptr(), c_dst.as_ptr());
            if ptr.is_null() { None } else { Some(ptr) }
        }
    }

    pub unsafe fn destroy_processor(&self, proc: *mut OcioProcessor) {
        unsafe {
            if let Ok(func) = self
                .lib
                .get::<FnDestroyProcessor>(b"ocio_destroy_processor")
            {
                func(proc);
            }
        }
    }

    pub unsafe fn apply_transform(&self, proc: *mut OcioProcessor, pixels: &mut [f32]) {
        unsafe {
            if let Ok(func) = self.lib.get::<FnApplyTransform>(b"ocio_apply_transform") {
                let count = pixels.len() as i32 / 4;
                func(proc, pixels.as_mut_ptr(), count);
            }
        }
    }
}
