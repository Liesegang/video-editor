//! FFmpeg AVFrame to typed, straight, full-range RGB pixel storage.
//!
//! Floating-point RGB frames are read directly from their
//! [`AVPixFmtDescriptor`](ffmpeg::ffi::AVPixFmtDescriptor) component layout so
//! negative and greater-than-one samples never cross a UNORM conversion.
//! Verified planar YUV is delegated to the descriptor-driven H.273 `f32`
//! converter. Integer RGB that needs more than the RGBA8 compatibility path is
//! also read directly from its component descriptor. Transfer and primaries
//! remain encoded.

use super::ffmpeg_color_metadata::{DecodeColorPlan, DecodePixelStorage};
use super::ffmpeg_yuv_decode::decode_planar_yuv_to_rgba32f;
use super::{DecodedPixelLayout, DecodedPixelStorage, validate_decoded_pixel_layout};
use crate::error::LibraryError;
use crate::model::frame::Image;
use crate::plugin::{DecodedPixelBuffer, DecodedStraightRgba8, DecodedStraightRgba32F};
use ffmpeg_next as ffmpeg;

const RGBA8_BYTES_PER_PIXEL: usize = 4;

pub(super) fn decode_frame_pixels(
    frame: &ffmpeg::util::frame::Video,
    color_plan: &DecodeColorPlan,
) -> Result<DecodedPixelBuffer, LibraryError> {
    let is_float = pixel_format_has_flag(frame.format(), ffmpeg::ffi::AV_PIX_FMT_FLAG_FLOAT);
    let planned = color_plan.pixel_storage();
    let target_storage = decoded_target_storage(is_float, planned);
    decode_after_layout_validation(frame.width(), frame.height(), target_storage, |layout| {
        match planned {
            DecodePixelStorage::Rejected => Err(LibraryError::FfmpegOther(format!(
                "refusing unverified FFmpeg pixel conversion: {}",
                color_plan
                    .rejection_reason()
                    .unwrap_or("missing verified conversion authority")
            ))),
            DecodePixelStorage::Rgba32FDirectYuv => decode_planar_yuv_to_rgba32f(
                frame,
                color_plan.verified_yuv_conversion().ok_or_else(|| {
                    LibraryError::FfmpegOther(
                        "verified YUV storage lacks H.273 conversion parameters".to_string(),
                    )
                })?,
                layout,
            ),
            DecodePixelStorage::Rgba8 | DecodePixelStorage::Rgba32FDirectRgb if is_float => {
                decode_float_rgb_direct(frame, layout)
            }
            DecodePixelStorage::Rgba8 => decode_rgba8(frame, layout),
            DecodePixelStorage::Rgba32FDirectRgb => decode_integer_rgb_direct(frame, layout),
        }
    })
}

fn decoded_target_storage(is_float: bool, planned: DecodePixelStorage) -> DecodedPixelStorage {
    if is_float {
        return DecodedPixelStorage::StraightRgba32F;
    }
    match planned {
        DecodePixelStorage::Rgba8 => DecodedPixelStorage::StraightRgba8,
        DecodePixelStorage::Rgba32FDirectRgb
        | DecodePixelStorage::Rgba32FDirectYuv
        | DecodePixelStorage::Rejected => DecodedPixelStorage::StraightRgba32F,
    }
}

fn decode_after_layout_validation<T>(
    width: u32,
    height: u32,
    storage: DecodedPixelStorage,
    decode: impl FnOnce(DecodedPixelLayout) -> Result<T, LibraryError>,
) -> Result<T, LibraryError> {
    let layout = validate_decoded_pixel_layout(width, height, storage)?;
    decode(layout)
}

#[derive(Clone, Copy, Debug)]
struct FloatComponentLayout<'a> {
    data: &'a [u8],
    stride: usize,
    step: usize,
    offset: usize,
    byte_width: usize,
    big_endian: bool,
}

fn decode_float_rgb_direct(
    frame: &ffmpeg::util::frame::Video,
    layout: DecodedPixelLayout,
) -> Result<DecodedPixelBuffer, LibraryError> {
    let format = frame.format();
    let descriptor = format.descriptor().ok_or_else(|| {
        LibraryError::FfmpegOther(format!(
            "floating-point pixel format '{}' has no component descriptor",
            pixel_format_name(format)
        ))
    })?;
    if !pixel_format_has_flag(format, ffmpeg::ffi::AV_PIX_FMT_FLAG_RGB) {
        return Err(LibraryError::FfmpegOther(format!(
            "unsupported floating-point pixel format '{}': only descriptor-backed RGB/RGBA layouts can preserve extended-range samples",
            descriptor.name()
        )));
    }

    let component_count = usize::from(descriptor.nb_components());
    if !(3..=4).contains(&component_count) {
        return Err(LibraryError::FfmpegOther(format!(
            "unsupported floating-point RGB layout '{}': expected 3 or 4 components, got {component_count}",
            descriptor.name()
        )));
    }
    let has_alpha = pixel_format_has_flag(format, ffmpeg::ffi::AV_PIX_FMT_FLAG_ALPHA);
    if has_alpha != (component_count == 4) {
        return Err(LibraryError::FfmpegOther(format!(
            "unsupported integer RGB layout '{}': alpha flag and component count disagree",
            descriptor.name()
        )));
    }
    let has_alpha = pixel_format_has_flag(format, ffmpeg::ffi::AV_PIX_FMT_FLAG_ALPHA);
    if has_alpha != (component_count == 4) {
        return Err(LibraryError::FfmpegOther(format!(
            "unsupported floating-point RGB layout '{}': alpha flag and component count disagree",
            descriptor.name()
        )));
    }

    let layouts = (0..component_count)
        .map(|index| float_component_layout(frame, index))
        .collect::<Result<Vec<_>, _>>()?;
    let width = usize::try_from(layout.width())
        .map_err(|_| LibraryError::FfmpegOther("decoded width exceeds usize".to_string()))?;
    let height = usize::try_from(layout.height())
        .map_err(|_| LibraryError::FfmpegOther("decoded height exceeds usize".to_string()))?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(layout.pixel_count())
        .map_err(|_| {
            LibraryError::FfmpegOther(format!(
                "cannot allocate {} floating-point decoded pixels",
                layout.pixel_count()
            ))
        })?;

    for row in 0..height {
        for column in 0..width {
            let mut pixel = [0.0, 0.0, 0.0, 1.0];
            for (component, layout) in pixel.iter_mut().zip(&layouts) {
                *component = read_float_frame_component(*layout, row, column)?;
            }
            pixels.push(pixel);
        }
    }

    Ok(DecodedPixelBuffer::StraightRgba32F(
        DecodedStraightRgba32F::new(frame.width(), frame.height(), pixels)?,
    ))
}

fn float_component_layout(
    frame: &ffmpeg::util::frame::Video,
    component_index: usize,
) -> Result<FloatComponentLayout<'_>, LibraryError> {
    let format = frame.format();
    let descriptor = format.descriptor().ok_or_else(|| {
        LibraryError::FfmpegOther("floating-point format descriptor disappeared".to_string())
    })?;
    if component_index >= usize::from(descriptor.nb_components()) {
        return Err(LibraryError::FfmpegOther(format!(
            "floating-point component index {component_index} is out of range for '{}'",
            descriptor.name()
        )));
    }
    let descriptor_ptr = descriptor.as_ptr();
    // SAFETY: FFmpeg pixel descriptors are immutable process-lifetime table
    // entries. `component_index` was checked against `nb_components`, which is
    // bounded by the fixed four-element `comp` array.
    let component = unsafe {
        std::ptr::addr_of!((*descriptor_ptr).comp)
            .cast::<ffmpeg::ffi::AVComponentDescriptor>()
            .add(component_index)
            .read()
    };
    let plane = usize::try_from(component.plane).map_err(|_| {
        LibraryError::FfmpegOther(format!(
            "floating-point component {component_index} has a negative plane index"
        ))
    })?;
    if plane >= frame.planes() {
        return Err(LibraryError::FfmpegOther(format!(
            "floating-point component {component_index} references missing plane {plane}"
        )));
    }
    let step = usize::try_from(component.step)
        .ok()
        .filter(|step| *step > 0)
        .ok_or_else(|| {
            LibraryError::FfmpegOther(format!(
                "floating-point component {component_index} has an invalid step"
            ))
        })?;
    let offset = usize::try_from(component.offset).map_err(|_| {
        LibraryError::FfmpegOther(format!(
            "floating-point component {component_index} has a negative offset"
        ))
    })?;
    if component.shift != 0 {
        return Err(LibraryError::FfmpegOther(format!(
            "unsupported floating-point component {component_index}: bit shift {} is not zero",
            component.shift
        )));
    }
    let byte_width = match component.depth {
        16 => 2,
        32 => 4,
        depth => {
            return Err(LibraryError::FfmpegOther(format!(
                "unsupported {depth}-bit floating-point RGB component {component_index}"
            )));
        }
    };
    if offset
        .checked_add(byte_width)
        .is_none_or(|component_end| component_end > step)
    {
        return Err(LibraryError::FfmpegOther(format!(
            "unsupported floating-point component {component_index} layout: offset={offset} bytes={byte_width} step={step}"
        )));
    }

    let width = usize::try_from(frame.width())
        .map_err(|_| LibraryError::FfmpegOther("decoded width exceeds usize".to_string()))?;
    let height = usize::try_from(frame.height())
        .map_err(|_| LibraryError::FfmpegOther("decoded height exceeds usize".to_string()))?;
    let row_bytes = width
        .checked_sub(1)
        .and_then(|last_column| last_column.checked_mul(step))
        .and_then(|last_start| last_start.checked_add(offset))
        .and_then(|last_start| last_start.checked_add(byte_width))
        .ok_or_else(|| {
            LibraryError::FfmpegOther(
                "floating-point component row layout overflowed or was empty".to_string(),
            )
        })?;
    let (data, stride) = validated_plane_bytes(
        frame,
        plane,
        height,
        row_bytes,
        &format!("floating-point component {component_index}"),
    )?;

    Ok(FloatComponentLayout {
        data,
        stride,
        step,
        offset,
        byte_width,
        big_endian: pixel_format_has_flag(format, ffmpeg::ffi::AV_PIX_FMT_FLAG_BE),
    })
}

fn read_float_frame_component(
    layout: FloatComponentLayout<'_>,
    row: usize,
    column: usize,
) -> Result<f32, LibraryError> {
    let start = row
        .checked_mul(layout.stride)
        .and_then(|row| {
            column
                .checked_mul(layout.step)
                .and_then(|column| row.checked_add(column))
        })
        .and_then(|pixel| pixel.checked_add(layout.offset))
        .ok_or_else(|| {
            LibraryError::FfmpegOther("floating-point component address overflowed".to_string())
        })?;
    let end = start.checked_add(layout.byte_width).ok_or_else(|| {
        LibraryError::FfmpegOther("floating-point component address overflowed".to_string())
    })?;
    let bytes = layout.data.get(start..end).ok_or_else(|| {
        LibraryError::FfmpegOther("floating-point component exceeds its source plane".to_string())
    })?;
    read_float_component(bytes, layout.big_endian)
}

fn pixel_format_has_flag(format: ffmpeg::format::Pixel, flag: i32) -> bool {
    let Some(descriptor) = format.descriptor() else {
        return false;
    };
    let Ok(flag) = u64::try_from(flag) else {
        return false;
    };
    let descriptor_ptr = descriptor.as_ptr();
    // SAFETY: FFmpeg returns immutable process-lifetime pixel descriptors. We
    // copy only the plain integer `flags` field while the descriptor is live.
    let flags = unsafe { std::ptr::addr_of!((*descriptor_ptr).flags).read() };
    flags & flag != 0
}

fn pixel_format_name(format: ffmpeg::format::Pixel) -> &'static str {
    format
        .descriptor()
        .map_or("unknown-pixel-format", |descriptor| descriptor.name())
}

fn validated_plane_bytes<'a>(
    frame: &'a ffmpeg::util::frame::Video,
    plane: usize,
    height: usize,
    row_bytes: usize,
    context: &str,
) -> Result<(&'a [u8], usize), LibraryError> {
    if plane >= frame.planes() || height == 0 || row_bytes == 0 {
        return Err(LibraryError::FfmpegOther(format!(
            "{context} references a missing or empty plane {plane}"
        )));
    }
    // SAFETY: the frame is live for `'a`; only scalar/pointer members in the
    // fixed AVFrame plane arrays are copied after the plane bound check.
    let frame_ptr = unsafe { frame.as_ptr() };
    // SAFETY: `plane < frame.planes() <= 8` bounds both fixed arrays.
    let (data_ptr, signed_stride) = unsafe {
        (
            std::ptr::addr_of!((*frame_ptr).data[plane]).read(),
            std::ptr::addr_of!((*frame_ptr).linesize[plane]).read(),
        )
    };
    if data_ptr.is_null() || signed_stride <= 0 {
        return Err(LibraryError::FfmpegOther(format!(
            "{context} has a null plane or non-positive stride {signed_stride}"
        )));
    }
    let stride = usize::try_from(signed_stride)
        .map_err(|_| LibraryError::FfmpegOther(format!("{context} stride overflow")))?;
    if stride < row_bytes {
        return Err(LibraryError::FfmpegOther(format!(
            "{context} stride {stride} is shorter than row size {row_bytes}"
        )));
    }
    let required = height
        .checked_sub(1)
        .and_then(|last| last.checked_mul(stride))
        .and_then(|last| last.checked_add(row_bytes))
        .ok_or_else(|| LibraryError::FfmpegOther(format!("{context} layout overflowed")))?;
    let plane_span = stride
        .checked_mul(height)
        .ok_or_else(|| LibraryError::FfmpegOther(format!("{context} plane span overflowed")))?;
    if plane_span < required || plane_span > isize::MAX as usize {
        return Err(LibraryError::FfmpegOther(format!(
            "{context} plane span {plane_span} cannot cover {required} bytes"
        )));
    }
    // SAFETY: FFmpeg-owned AVFrames guarantee a non-null plane covers
    // `linesize * plane_height`. Bounds and arithmetic were validated above.
    // As with all AVFrame wrappers, corrupt foreign allocation provenance
    // cannot be proven in Rust and is outside this loader boundary.
    let data = unsafe { std::slice::from_raw_parts(data_ptr, plane_span) };
    Ok((data, stride))
}

fn decode_rgba8(
    frame: &ffmpeg::util::frame::Video,
    layout: DecodedPixelLayout,
) -> Result<DecodedPixelBuffer, LibraryError> {
    let converted = scale_rgb(frame, ffmpeg::format::Pixel::RGBA)?;
    let data = copy_packed_rows(&converted, RGBA8_BYTES_PER_PIXEL, layout)?;
    let pixels =
        DecodedStraightRgba8::new(Image::new(converted.width(), converted.height(), data))?;
    Ok(DecodedPixelBuffer::StraightRgba8(pixels))
}

fn decode_integer_rgb_direct(
    frame: &ffmpeg::util::frame::Video,
    layout: DecodedPixelLayout,
) -> Result<DecodedPixelBuffer, LibraryError> {
    let format = frame.format();
    let descriptor = format.descriptor().ok_or_else(|| {
        LibraryError::FfmpegOther(format!(
            "integer RGB pixel format '{}' has no component descriptor",
            pixel_format_name(format)
        ))
    })?;
    if !pixel_format_has_flag(format, ffmpeg::ffi::AV_PIX_FMT_FLAG_RGB) {
        return Err(LibraryError::FfmpegOther(format!(
            "integer direct RGB path received non-RGB format '{}'",
            pixel_format_name(format)
        )));
    }
    let component_count = usize::from(descriptor.nb_components());
    if !(3..=4).contains(&component_count) {
        return Err(LibraryError::FfmpegOther(format!(
            "unsupported integer RGB layout '{}': expected 3 or 4 components, got {component_count}",
            descriptor.name()
        )));
    }
    let components = (0..component_count)
        .map(|index| integer_rgb_component_layout(frame, index))
        .collect::<Result<Vec<_>, _>>()?;
    let width = usize::try_from(layout.width())
        .map_err(|_| LibraryError::FfmpegOther("decoded width exceeds usize".to_string()))?;
    let height = usize::try_from(layout.height())
        .map_err(|_| LibraryError::FfmpegOther("decoded height exceeds usize".to_string()))?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(layout.pixel_count())
        .map_err(|_| {
            LibraryError::FfmpegOther(format!(
                "cannot allocate {} integer RGB decode pixels",
                layout.pixel_count()
            ))
        })?;
    for row in 0..height {
        for column in 0..width {
            let mut pixel = [0.0, 0.0, 0.0, 1.0];
            for (destination, component) in pixel.iter_mut().zip(&components) {
                let raw = read_integer_frame_component(*component, row, column)?;
                *destination = raw as f32 / component.maximum as f32;
            }
            pixels.push(pixel);
        }
    }
    Ok(DecodedPixelBuffer::StraightRgba32F(
        DecodedStraightRgba32F::new(frame.width(), frame.height(), pixels)?,
    ))
}

#[derive(Clone, Copy, Debug)]
struct IntegerComponentLayout<'a> {
    data: &'a [u8],
    stride: usize,
    step: usize,
    offset: usize,
    byte_width: usize,
    shift: u32,
    maximum: u64,
    big_endian: bool,
}

fn integer_rgb_component_layout(
    frame: &ffmpeg::util::frame::Video,
    component_index: usize,
) -> Result<IntegerComponentLayout<'_>, LibraryError> {
    let format = frame.format();
    let descriptor = format.descriptor().ok_or_else(|| {
        LibraryError::FfmpegOther("integer RGB format descriptor disappeared".to_string())
    })?;
    let descriptor_ptr = descriptor.as_ptr();
    if component_index >= usize::from(descriptor.nb_components()) {
        return Err(LibraryError::FfmpegOther(format!(
            "integer RGB component {component_index} is out of range"
        )));
    }
    // SAFETY: descriptor table entries have process lifetime and the checked
    // component index is within the fixed four-element component array.
    let component = unsafe {
        std::ptr::addr_of!((*descriptor_ptr).comp)
            .cast::<ffmpeg::ffi::AVComponentDescriptor>()
            .add(component_index)
            .read()
    };
    let plane = usize::try_from(component.plane)
        .map_err(|_| LibraryError::FfmpegOther("negative RGB plane index".to_string()))?;
    if plane >= frame.planes() {
        return Err(LibraryError::FfmpegOther(format!(
            "RGB component {component_index} references missing plane {plane}"
        )));
    }
    let step = usize::try_from(component.step)
        .ok()
        .filter(|step| *step > 0)
        .ok_or_else(|| LibraryError::FfmpegOther("invalid RGB component step".to_string()))?;
    let offset = usize::try_from(component.offset)
        .map_err(|_| LibraryError::FfmpegOther("negative RGB component offset".to_string()))?;
    let shift = u32::try_from(component.shift)
        .map_err(|_| LibraryError::FfmpegOther("negative RGB component shift".to_string()))?;
    let depth = u32::try_from(component.depth)
        .ok()
        .filter(|depth| *depth > 0)
        .ok_or_else(|| LibraryError::FfmpegOther("invalid RGB component depth".to_string()))?;
    let packed_bits = shift.checked_add(depth).ok_or_else(|| {
        LibraryError::FfmpegOther("RGB component bit layout overflowed".to_string())
    })?;
    let byte_width = usize::try_from(packed_bits.div_ceil(8)).map_err(|_| {
        LibraryError::FfmpegOther("RGB component byte width overflowed".to_string())
    })?;
    let component_end = offset
        .checked_add(byte_width)
        .ok_or_else(|| LibraryError::FfmpegOther("RGB component layout overflowed".to_string()))?;
    if byte_width == 0 || byte_width > 8 || component_end > step {
        return Err(LibraryError::FfmpegOther(format!(
            "unsupported RGB layout offset={offset} bytes={byte_width} step={step}"
        )));
    }
    let width = usize::try_from(frame.width())
        .map_err(|_| LibraryError::FfmpegOther("decoded width exceeds usize".to_string()))?;
    let height = usize::try_from(frame.height())
        .map_err(|_| LibraryError::FfmpegOther("decoded height exceeds usize".to_string()))?;
    let row_bytes = width
        .checked_sub(1)
        .and_then(|last| last.checked_mul(step))
        .and_then(|last| last.checked_add(component_end))
        .ok_or_else(|| LibraryError::FfmpegOther("RGB row layout overflowed".to_string()))?;
    let (data, stride) = validated_plane_bytes(
        frame,
        plane,
        height,
        row_bytes,
        &format!("integer RGB component {component_index}"),
    )?;
    let maximum = 1_u64
        .checked_shl(depth)
        .and_then(|limit| limit.checked_sub(1))
        .ok_or_else(|| LibraryError::FfmpegOther("RGB depth exceeds 63 bits".to_string()))?;
    Ok(IntegerComponentLayout {
        data,
        stride,
        step,
        offset,
        byte_width,
        shift,
        maximum,
        big_endian: pixel_format_has_flag(format, ffmpeg::ffi::AV_PIX_FMT_FLAG_BE),
    })
}

fn read_integer_frame_component(
    layout: IntegerComponentLayout<'_>,
    row: usize,
    column: usize,
) -> Result<u64, LibraryError> {
    let start = row * layout.stride + column * layout.step + layout.offset;
    let bytes = layout
        .data
        .get(start..start + layout.byte_width)
        .ok_or_else(|| LibraryError::FfmpegOther("RGB component exceeds plane".to_string()))?;
    Ok((read_unsigned_component(bytes, layout.big_endian) >> layout.shift) & layout.maximum)
}

fn read_unsigned_component(bytes: &[u8], big_endian: bool) -> u64 {
    if big_endian {
        bytes
            .iter()
            .fold(0_u64, |value, byte| (value << 8) | u64::from(*byte))
    } else {
        bytes
            .iter()
            .rev()
            .fold(0_u64, |value, byte| (value << 8) | u64::from(*byte))
    }
}

fn read_float_component(bytes: &[u8], big_endian: bool) -> Result<f32, LibraryError> {
    let bits = read_unsigned_component(bytes, big_endian);
    match bytes.len() {
        2 => Ok(half::f16::from_bits(bits as u16).to_f32()),
        4 => Ok(f32::from_bits(bits as u32)),
        width => Err(LibraryError::FfmpegOther(format!(
            "unsupported floating-point component width {width}"
        ))),
    }
}

fn scale_rgb(
    frame: &ffmpeg::util::frame::Video,
    output_format: ffmpeg::format::Pixel,
) -> Result<ffmpeg::util::frame::Video, LibraryError> {
    let mut scaler = ffmpeg::software::scaling::context::Context::get(
        frame.format(),
        frame.width(),
        frame.height(),
        output_format,
        frame.width(),
        frame.height(),
        ffmpeg::software::scaling::flag::Flags::BILINEAR,
    )?;
    let mut converted = ffmpeg::util::frame::Video::empty();
    scaler.run(frame, &mut converted)?;
    Ok(converted)
}

fn copy_packed_rows(
    frame: &ffmpeg::util::frame::Video,
    bytes_per_pixel: usize,
    layout: DecodedPixelLayout,
) -> Result<Vec<u8>, LibraryError> {
    if layout.storage() != DecodedPixelStorage::StraightRgba8
        || bytes_per_pixel != RGBA8_BYTES_PER_PIXEL
    {
        return Err(LibraryError::FfmpegOther(
            "packed RGBA8 copy received the wrong target layout".to_string(),
        ));
    }
    let (_width, height, row_bytes, stride, required) =
        packed_plane_geometry(frame, bytes_per_pixel, layout)?;
    let plane = frame.data(0);
    if plane.len() < required {
        return Err(LibraryError::FfmpegOther(format!(
            "decoded packed plane has {} bytes but {required} are required",
            plane.len()
        )));
    }

    let mut data = Vec::new();
    data.try_reserve_exact(layout.byte_len()).map_err(|_| {
        LibraryError::FfmpegOther(format!(
            "cannot allocate {} decoded RGBA8 bytes",
            layout.byte_len()
        ))
    })?;
    for row in 0..height {
        let start = row * stride;
        data.extend_from_slice(&plane[start..start + row_bytes]);
    }
    Ok(data)
}

fn packed_plane_geometry(
    frame: &ffmpeg::util::frame::Video,
    bytes_per_pixel: usize,
    layout: DecodedPixelLayout,
) -> Result<(usize, usize, usize, usize, usize), LibraryError> {
    if frame.width() != layout.width() || frame.height() != layout.height() {
        return Err(LibraryError::FfmpegOther(format!(
            "converted frame dimensions {}x{} differ from validated target {}x{}",
            frame.width(),
            frame.height(),
            layout.width(),
            layout.height()
        )));
    }
    let width = usize::try_from(layout.width())
        .map_err(|_| LibraryError::FfmpegOther("decoded width exceeds usize".to_string()))?;
    let height = usize::try_from(layout.height())
        .map_err(|_| LibraryError::FfmpegOther("decoded height exceeds usize".to_string()))?;
    let row_bytes = width.checked_mul(bytes_per_pixel).ok_or_else(|| {
        LibraryError::FfmpegOther("decoded packed-row byte count overflowed".to_string())
    })?;
    let stride = frame.stride(0);
    if stride < row_bytes {
        return Err(LibraryError::FfmpegOther(format!(
            "decoded packed frame stride {stride} is shorter than row size {row_bytes}"
        )));
    }
    let required = stride
        .checked_mul(height.saturating_sub(1))
        .and_then(|last_row| last_row.checked_add(row_bytes))
        .ok_or_else(|| {
            LibraryError::FfmpegOther("decoded packed-plane size overflowed".to_string())
        })?;
    Ok((width, height, row_bytes, stride, required))
}

#[cfg(test)]
mod tests {
    use super::{
        copy_packed_rows, decode_after_layout_validation, decode_frame_pixels,
        decoded_target_storage,
    };
    use crate::model::asset::{
        SourceColorDescription, SourceColorPrimaries, SourceColorRange, SourceMatrixCoefficients,
        SourceTransferCharacteristic,
    };
    use crate::plugin::DecodedPixelBuffer;
    use crate::plugin::loaders::ffmpeg_color_metadata::{DecodeColorPlan, DecodePixelStorage};
    use crate::plugin::loaders::{DecodedPixelStorage, validate_decoded_pixel_layout};
    use ffmpeg_next as ffmpeg;
    use std::cell::Cell;

    #[test]
    fn every_ffmpeg_decode_target_is_typed_before_work_begins() {
        assert_eq!(
            decoded_target_storage(false, DecodePixelStorage::Rgba8),
            DecodedPixelStorage::StraightRgba8
        );
        assert_eq!(
            decoded_target_storage(false, DecodePixelStorage::Rgba32FDirectYuv),
            DecodedPixelStorage::StraightRgba32F
        );
        assert_eq!(
            decoded_target_storage(true, DecodePixelStorage::Rgba8),
            DecodedPixelStorage::StraightRgba32F
        );
    }

    #[test]
    fn oversized_ffmpeg_targets_are_rejected_before_decode_work() {
        for (width, height, storage) in [
            (32_768, 8_192, DecodedPixelStorage::StraightRgba8),
            (16_384, 8_192, DecodedPixelStorage::StraightRgba32F),
        ] {
            let attempted = Cell::new(false);
            let result = decode_after_layout_validation(width, height, storage, |_| {
                attempted.set(true);
                Ok(())
            });
            assert!(result.is_err(), "oversized {storage:?} target was accepted");
            assert!(
                !attempted.get(),
                "decode/scaler callback ran for oversized {storage:?} target"
            );
        }
    }

    #[test]
    fn rgba64_little_endian_is_normalized_without_component_or_alpha_swaps() {
        let mut source = ffmpeg::util::frame::Video::new(ffmpeg::format::Pixel::RGBA64LE, 1, 1);
        source.set_color_space(ffmpeg::color::Space::RGB);
        source.set_color_range(ffmpeg::color::Range::JPEG);
        let values = [0x1234_u16, 0xabcd, 0x0102, 0x8001];
        for (bytes, value) in source.data_mut(0)[..8].chunks_exact_mut(2).zip(values) {
            bytes.copy_from_slice(&value.to_le_bytes());
        }
        let plan = DecodeColorPlan::from_metadata(
            source.format(),
            source.color_space(),
            source.color_range(),
            SourceColorDescription {
                assumption: None,
                primaries: Some(SourceColorPrimaries::Bt2020),
                transfer: Some(SourceTransferCharacteristic::Pq),
                matrix: Some(SourceMatrixCoefficients::Identity),
                range: Some(SourceColorRange::Full),
                bit_depth: Some(16),
                profile: None,
            },
        );

        let decoded = decode_frame_pixels(&source, &plan).expect("RGBA64 conversion");
        let DecodedPixelBuffer::StraightRgba32F(decoded) = decoded else {
            panic!("16-bit/HDR input must use RGBA32F storage");
        };
        let actual = decoded.data()[0];
        for (actual, expected) in actual.into_iter().zip(values) {
            assert!((actual - f32::from(expected) / f32::from(u16::MAX)).abs() <= 1.0e-6);
        }
    }

    #[test]
    fn planar_float_rgb_preserves_negative_and_greater_than_one_samples() {
        let format = ffmpeg::format::Pixel::GBRPF32LE;
        let mut source = ffmpeg::util::frame::Video::new(format, 2, 1);
        source.set_color_space(ffmpeg::color::Space::RGB);
        source.set_color_range(ffmpeg::color::Range::JPEG);
        // AV_PIX_FMT_GBRPF32LE stores G, B, R in its three planes while the
        // descriptor component order remains R, G, B.
        write_f32_row(&mut source, 0, [1.5, -0.5]);
        write_f32_row(&mut source, 1, [0.5, 0.0]);
        write_f32_row(&mut source, 2, [-0.25, 2.25]);
        let plan = DecodeColorPlan::from_metadata(
            format,
            source.color_space(),
            source.color_range(),
            SourceColorDescription {
                assumption: None,
                primaries: Some(SourceColorPrimaries::Bt709),
                transfer: Some(SourceTransferCharacteristic::Linear),
                matrix: Some(SourceMatrixCoefficients::Identity),
                range: Some(SourceColorRange::Full),
                bit_depth: Some(32),
                profile: None,
            },
        );

        let decoded = decode_frame_pixels(&source, &plan).expect("direct float RGB decode");
        let DecodedPixelBuffer::StraightRgba32F(decoded) = decoded else {
            panic!("floating-point RGB must remain RGBA32F");
        };
        assert_eq!(
            decoded.data(),
            &[[-0.25, 1.5, 0.5, 1.0], [2.25, -0.5, 0.0, 1.0]]
        );
    }

    #[test]
    fn packed_half_float_rgba_preserves_extended_rgb_and_straight_alpha() {
        let format = ffmpeg::format::Pixel::RGBAF16LE;
        let mut source = ffmpeg::util::frame::Video::new(format, 1, 1);
        source.set_color_space(ffmpeg::color::Space::RGB);
        source.set_color_range(ffmpeg::color::Range::JPEG);
        let values = [-0.5_f32, 2.0, 0.25, 0.5];
        for (bytes, value) in source.data_mut(0)[..8].chunks_exact_mut(2).zip(values) {
            bytes.copy_from_slice(&half::f16::from_f32(value).to_bits().to_le_bytes());
        }
        let plan = DecodeColorPlan::from_metadata(
            format,
            source.color_space(),
            source.color_range(),
            SourceColorDescription {
                assumption: None,
                primaries: Some(SourceColorPrimaries::Bt709),
                transfer: Some(SourceTransferCharacteristic::Linear),
                matrix: Some(SourceMatrixCoefficients::Identity),
                range: Some(SourceColorRange::Full),
                bit_depth: Some(16),
                profile: None,
            },
        );

        let decoded = decode_frame_pixels(&source, &plan).expect("direct half-float RGBA decode");
        let DecodedPixelBuffer::StraightRgba32F(decoded) = decoded else {
            panic!("half-float RGBA must remain RGBA32F");
        };
        assert_eq!(decoded.data(), &[values]);
    }

    #[test]
    fn unsupported_non_rgb_float_layout_fails_closed_before_unorm_scaling() {
        let format = ffmpeg::format::Pixel::GRAYF32LE;
        let mut source = ffmpeg::util::frame::Video::new(format, 1, 1);
        source.set_color_space(ffmpeg::color::Space::RGB);
        source.set_color_range(ffmpeg::color::Range::JPEG);
        source.data_mut(0)[..4].copy_from_slice(&1.5_f32.to_le_bytes());
        let plan = DecodeColorPlan::from_metadata(
            format,
            source.color_space(),
            source.color_range(),
            SourceColorDescription {
                assumption: None,
                primaries: Some(SourceColorPrimaries::Bt709),
                transfer: Some(SourceTransferCharacteristic::Linear),
                matrix: Some(SourceMatrixCoefficients::Identity),
                range: Some(SourceColorRange::Full),
                bit_depth: Some(32),
                profile: None,
            },
        );

        let error = decode_frame_pixels(&source, &plan)
            .expect_err("non-RGB float input must not pass through a clamping UNORM scaler");
        assert!(error.to_string().contains("refusing unverified"));
    }

    #[test]
    fn packed_row_copy_drops_stride_padding_only() {
        let mut frame = ffmpeg::util::frame::Video::new(ffmpeg::format::Pixel::RGBA, 2, 2);
        let stride = frame.stride(0);
        assert!(stride >= 8);
        frame.data_mut(0)[..8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        frame.data_mut(0)[stride..stride + 8].copy_from_slice(&[9, 10, 11, 12, 13, 14, 15, 16]);

        assert_eq!(
            copy_packed_rows(
                &frame,
                4,
                validate_decoded_pixel_layout(2, 2, DecodedPixelStorage::StraightRgba8,)
                    .expect("small RGBA8 layout"),
            )
            .expect("valid packed frame"),
            (1_u8..=16).collect::<Vec<_>>()
        );
    }

    #[test]
    fn adjacent_ten_twelve_and_sixteen_bit_codes_survive_the_float_decode_boundary() {
        for (format, depth, first, chroma) in [
            (ffmpeg::format::Pixel::YUV444P10LE, 10_u8, 400_u16, 512_u16),
            (
                ffmpeg::format::Pixel::YUV444P12LE,
                12_u8,
                1_600_u16,
                2_048_u16,
            ),
            (
                ffmpeg::format::Pixel::YUV444P16LE,
                16_u8,
                30_000_u16,
                32_768_u16,
            ),
        ] {
            let mut source = ffmpeg::util::frame::Video::new(format, 2, 1);
            source.set_color_space(ffmpeg::color::Space::BT709);
            source.set_color_range(ffmpeg::color::Range::JPEG);
            write_u16_row(&mut source, 0, [first, first + 1]);
            write_u16_row(&mut source, 1, [chroma, chroma]);
            write_u16_row(&mut source, 2, [chroma, chroma]);
            let plan = DecodeColorPlan::from_metadata(
                format,
                source.color_space(),
                source.color_range(),
                SourceColorDescription {
                    assumption: None,
                    primaries: Some(SourceColorPrimaries::Bt709),
                    transfer: Some(SourceTransferCharacteristic::Bt709),
                    matrix: Some(SourceMatrixCoefficients::Bt709),
                    range: Some(SourceColorRange::Full),
                    bit_depth: Some(depth),
                    profile: None,
                },
            );

            let decoded = decode_frame_pixels(&source, &plan).expect("high-bit YUV conversion");
            let DecodedPixelBuffer::StraightRgba32F(decoded) = decoded else {
                panic!("{depth}-bit source was quantized to RGBA8");
            };
            let [first_pixel, second_pixel] = decoded.data() else {
                panic!("2x1 conversion did not return two pixels");
            };
            assert_ne!(
                first_pixel[0], second_pixel[0],
                "adjacent {depth}-bit luma codes collapsed at the decode boundary"
            );
            assert_eq!(first_pixel[3], 1.0);
            assert_eq!(second_pixel[3], 1.0);
        }
    }

    #[test]
    fn ten_bit_yuva_keeps_straight_high_precision_alpha() {
        let format = ffmpeg::format::Pixel::YUVA444P10LE;
        let mut source = ffmpeg::util::frame::Video::new(format, 2, 1);
        source.set_color_space(ffmpeg::color::Space::BT709);
        source.set_color_range(ffmpeg::color::Range::JPEG);
        write_u16_row(&mut source, 0, [512, 512]);
        write_u16_row(&mut source, 1, [512, 512]);
        write_u16_row(&mut source, 2, [512, 512]);
        write_u16_row(&mut source, 3, [257, 257]);
        let plan = DecodeColorPlan::from_metadata(
            format,
            source.color_space(),
            source.color_range(),
            SourceColorDescription {
                assumption: None,
                primaries: Some(SourceColorPrimaries::Bt709),
                transfer: Some(SourceTransferCharacteristic::Bt709),
                matrix: Some(SourceMatrixCoefficients::Bt709),
                range: Some(SourceColorRange::Full),
                bit_depth: Some(10),
                profile: None,
            },
        );

        let decoded = decode_frame_pixels(&source, &plan).expect("10-bit YUVA conversion");
        let DecodedPixelBuffer::StraightRgba32F(decoded) = decoded else {
            panic!("10-bit YUVA source was quantized to RGBA8");
        };
        let pixel = decoded.data()[0];
        assert!(
            (pixel[3] - 257.0 / 1_023.0).abs() <= 2.0 / f32::from(u16::MAX),
            "expected alpha {}, got {}",
            257.0 / 1_023.0,
            pixel[3]
        );
        assert!(
            pixel[0] > 0.45,
            "RGB was premultiplied by alpha at the straight decode boundary"
        );
    }

    fn write_u16_row(frame: &mut ffmpeg::util::frame::Video, plane: usize, values: [u16; 2]) {
        for (bytes, value) in frame.data_mut(plane)[..4].chunks_exact_mut(2).zip(values) {
            bytes.copy_from_slice(&value.to_le_bytes());
        }
    }

    fn write_f32_row(frame: &mut ffmpeg::util::frame::Video, plane: usize, values: [f32; 2]) {
        for (bytes, value) in frame.data_mut(plane)[..8].chunks_exact_mut(4).zip(values) {
            bytes.copy_from_slice(&value.to_le_bytes());
        }
    }
}
