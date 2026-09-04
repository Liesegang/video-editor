//! Shared safety boundary helpers for the plugin's C ABI callbacks.

use std::ffi::c_void;
use std::panic::{catch_unwind, AssertUnwindSafe};

use ruvie_plugin_api::{
    RuvieBuffer, RuvieBytesView, RuvieCallResult, RuvieExtensionResultV1, RuvieOwnedRgba8FrameV1,
    STATUS_INVALID_REQUEST, STATUS_PANIC,
};

pub(super) fn call_guard(action: impl FnOnce() -> RuvieCallResult) -> RuvieCallResult {
    match catch_unwind(AssertUnwindSafe(action)) {
        Ok(result) => result,
        Err(_) => RuvieCallResult::error(STATUS_PANIC, "plugin callback panicked"),
    }
}

pub(super) fn extension_guard(
    action: impl FnOnce() -> RuvieExtensionResultV1,
) -> RuvieExtensionResultV1 {
    match catch_unwind(AssertUnwindSafe(action)) {
        Ok(result) => result,
        Err(_) => RuvieExtensionResultV1::error(STATUS_PANIC, "plugin callback panicked"),
    }
}

pub(super) unsafe fn bytes_from_view<'a>(view: RuvieBytesView) -> Result<&'a [u8], &'static str> {
    if view.len == 0 {
        return Ok(&[]);
    }
    if view.ptr.is_null() {
        return Err("non-empty byte view has a null pointer");
    }
    // SAFETY: The caller is inside the ABI callback for which the host keeps
    // this immutable borrowed byte view alive.
    Ok(unsafe { std::slice::from_raw_parts(view.ptr, view.len) })
}

pub(super) unsafe fn utf8_from_view<'a>(view: RuvieBytesView) -> Result<&'a str, &'static str> {
    // SAFETY: The same callback-scoped borrowed-view contract applies.
    std::str::from_utf8(unsafe { bytes_from_view(view)? }).map_err(|_| "byte view is not UTF-8")
}

pub(super) fn invalid_extension(detail: impl Into<String>) -> RuvieExtensionResultV1 {
    RuvieExtensionResultV1::error(STATUS_INVALID_REQUEST, detail)
}

pub(super) unsafe extern "C" fn free_frame(_context: *mut c_void, frame: RuvieOwnedRgba8FrameV1) {
    // SAFETY: The host returns a structurally reclaimable frame exactly once
    // to the extension table that allocated it.
    unsafe { ruvie_plugin_api::free_owned_buffer(frame.pixels) };
}

pub(super) unsafe extern "C" fn free_buffer(_context: *mut c_void, buffer: RuvieBuffer) {
    // SAFETY: The host returns every plugin-owned buffer exactly once to the
    // same dynamic library that allocated it.
    unsafe { ruvie_plugin_api::free_owned_buffer(buffer) };
}
