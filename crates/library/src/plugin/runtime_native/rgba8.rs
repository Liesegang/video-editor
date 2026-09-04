use std::mem::size_of;

use ruvie_plugin_api::{
    ALPHA_MODE_STRAIGHT_V1, COLOR_PROFILE_SRGB_V1, MAX_CPU_RGBA8_DIMENSION_V1,
    MAX_CPU_RGBA8_FRAME_BYTES_V1, RuvieBytesView, RuvieOwnedRgba8FrameV1, RuvieRgba8FrameViewV1,
};

use crate::error::LibraryError;
pub(super) fn rgba8_view(
    image: &crate::model::frame::Image,
) -> Result<RuvieRgba8FrameViewV1, LibraryError> {
    let stride = usize::try_from(image.width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .ok_or_else(|| LibraryError::Plugin("RGBA8 input stride overflow".to_string()))?;
    validate_rgba8_layout(image.width, image.height, stride, image.data.len())?;
    Ok(RuvieRgba8FrameViewV1 {
        struct_size: size_of::<RuvieRgba8FrameViewV1>(),
        width: image.width,
        height: image.height,
        stride_bytes: stride,
        alpha_mode: ALPHA_MODE_STRAIGHT_V1,
        color_profile: COLOR_PROFILE_SRGB_V1,
        pixels: RuvieBytesView::from_slice(&image.data),
    })
}

pub(super) fn validate_rgba8_layout(
    width: u32,
    height: u32,
    stride: usize,
    length: usize,
) -> Result<(), LibraryError> {
    if width == 0
        || height == 0
        || width > MAX_CPU_RGBA8_DIMENSION_V1
        || height > MAX_CPU_RGBA8_DIMENSION_V1
    {
        return Err(LibraryError::Plugin(format!(
            "RGBA8 dimensions {width}x{height} are outside ABI-v1 bounds"
        )));
    }
    let row_bytes = usize::try_from(width)
        .ok()
        .and_then(|value| value.checked_mul(4))
        .ok_or_else(|| LibraryError::Plugin("RGBA8 row-byte overflow".to_string()))?;
    if stride < row_bytes {
        return Err(LibraryError::Plugin(format!(
            "RGBA8 stride {stride} is smaller than row width {row_bytes}"
        )));
    }
    let expected = stride
        .checked_mul(usize::try_from(height).unwrap_or(usize::MAX))
        .ok_or_else(|| LibraryError::Plugin("RGBA8 frame-byte overflow".to_string()))?;
    if expected != length || expected > MAX_CPU_RGBA8_FRAME_BYTES_V1 {
        return Err(LibraryError::Plugin(format!(
            "RGBA8 buffer length {length} does not match bounded layout {expected}"
        )));
    }
    Ok(())
}

fn frame_is_reclaimable(frame: RuvieOwnedRgba8FrameV1) -> bool {
    let pixels = frame.pixels;
    (!pixels.ptr.is_null() && pixels.capacity >= pixels.len)
        || (pixels.ptr.is_null() && pixels.len == 0 && pixels.capacity == 0)
}

pub(super) fn reclaim_owned_frame(
    context: *mut std::ffi::c_void,
    free: Option<unsafe extern "C" fn(*mut std::ffi::c_void, RuvieOwnedRgba8FrameV1)>,
    frame: RuvieOwnedRgba8FrameV1,
) {
    if frame_is_reclaimable(frame)
        && let Some(free) = free
    {
        // SAFETY: Structural pointer/len/capacity invariants permit ownership
        // to return to the same extension exactly once.
        unsafe { free(context, frame) };
    }
}

pub(super) fn copy_owned_frame(
    context: *mut std::ffi::c_void,
    free: Option<unsafe extern "C" fn(*mut std::ffi::c_void, RuvieOwnedRgba8FrameV1)>,
    frame: RuvieOwnedRgba8FrameV1,
) -> Result<crate::model::frame::Image, LibraryError> {
    if !frame_is_reclaimable(frame) {
        return Err(LibraryError::Plugin(format!(
            "Runtime plugin returned an unreclaimable RGBA8 buffer (len={}, capacity={})",
            frame.pixels.len, frame.pixels.capacity
        )));
    }
    let result = (|| {
        if frame.struct_size < size_of::<RuvieOwnedRgba8FrameV1>() {
            return Err(LibraryError::Plugin(format!(
                "Runtime plugin returned a truncated RGBA8 frame table ({} bytes)",
                frame.struct_size
            )));
        }
        if frame.alpha_mode != ALPHA_MODE_STRAIGHT_V1 {
            return Err(LibraryError::Plugin(format!(
                "Runtime plugin returned unsupported alpha mode {}",
                frame.alpha_mode
            )));
        }
        if frame.color_profile != COLOR_PROFILE_SRGB_V1 {
            return Err(LibraryError::Plugin(format!(
                "Runtime plugin returned unsupported color profile {}",
                frame.color_profile
            )));
        }
        validate_rgba8_layout(
            frame.width,
            frame.height,
            frame.stride_bytes,
            frame.pixels.len,
        )?;
        let row_bytes = usize::try_from(frame.width)
            .ok()
            .and_then(|width| width.checked_mul(4))
            .ok_or_else(|| LibraryError::Plugin("RGBA8 row-byte overflow".to_string()))?;
        let tight_len = row_bytes
            .checked_mul(usize::try_from(frame.height).unwrap_or(usize::MAX))
            .ok_or_else(|| LibraryError::Plugin("RGBA8 tight-buffer overflow".to_string()))?;
        let pixels = if frame.pixels.len == 0 {
            &[][..]
        } else {
            // SAFETY: Non-null, len/capacity, total layout, and maximum byte
            // count were validated before borrowing plugin-owned memory.
            unsafe { std::slice::from_raw_parts(frame.pixels.ptr.cast_const(), frame.pixels.len) }
        };
        let mut tight = Vec::with_capacity(tight_len);
        for row in pixels.chunks_exact(frame.stride_bytes) {
            tight.extend_from_slice(&row[..row_bytes]);
        }
        Ok(crate::model::frame::Image::new(
            frame.width,
            frame.height,
            tight,
        ))
    })();
    let free = free.ok_or_else(|| {
        LibraryError::Plugin("Runtime plugin RGBA8 free callback is missing".to_string())
    })?;
    // SAFETY: Structural invariants were checked before the copy, and the
    // exact frame is returned once even when semantic validation failed.
    unsafe { free(context, frame) };
    result
}
