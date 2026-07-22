//! Descriptor-driven planar integer YUV/YUVA to unclamped straight RGBA32F.
//!
//! This module deliberately does not use libswscale. H.273 code-value
//! normalization, chroma reconstruction, and the non-constant-luminance matrix
//! are evaluated in `f32`, so valid source code values may produce negative or
//! greater-than-one encoded RGB without crossing a UNORM boundary.

use super::DecodedPixelLayout;
use super::ffmpeg_yuv_color::{H273Range, H273YuvToRgb};
use crate::error::LibraryError;
use crate::plugin::{DecodedPixelBuffer, DecodedStraightRgba32F, YuvChromaLocation};
use ffmpeg_next as ffmpeg;

const SUPPORTED_DEPTHS: [u8; 6] = [8, 9, 10, 12, 14, 16];

pub(super) fn decode_planar_yuv_to_rgba32f(
    frame: &ffmpeg::util::frame::Video,
    conversion: H273YuvToRgb,
    target: DecodedPixelLayout,
) -> Result<DecodedPixelBuffer, LibraryError> {
    let source = PlanarYuvFrame::new(frame, conversion)?;
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(target.pixel_count())
        .map_err(|_| {
            LibraryError::FfmpegOther(format!(
                "cannot allocate {} floating-point YUV decode pixels",
                target.pixel_count()
            ))
        })?;

    let width = usize::try_from(target.width())
        .map_err(|_| LibraryError::FfmpegOther("decoded width exceeds usize".to_string()))?;
    let height = usize::try_from(target.height())
        .map_err(|_| LibraryError::FfmpegOther("decoded height exceeds usize".to_string()))?;
    if width != source.width || height != source.height {
        return Err(LibraryError::FfmpegOther(format!(
            "validated target {}x{} differs from YUV source {}x{}",
            width, height, source.width, source.height
        )));
    }

    for row in 0..height {
        for column in 0..width {
            let y = normalize_luma(
                source.luma.read(column, row) as f32,
                source.depth,
                conversion.range(),
            );
            let cb = normalize_chroma(
                source.chroma_sample(&source.cb, column, row)?,
                source.depth,
                conversion.range(),
            );
            let cr = normalize_chroma(
                source.chroma_sample(&source.cr, column, row)?,
                source.depth,
                conversion.range(),
            );
            let matrix = conversion.matrix();
            let kr = matrix.kr();
            let kb = matrix.kb();
            let kg = 1.0 - kr - kb;
            let red = y + 2.0 * (1.0 - kr) * cr;
            let blue = y + 2.0 * (1.0 - kb) * cb;
            let green = y - (2.0 * kb * (1.0 - kb) / kg) * cb - (2.0 * kr * (1.0 - kr) / kg) * cr;
            let alpha = source.alpha.as_ref().map_or(1.0, |alpha| {
                alpha.read(column, row) as f32 / code_max(source.depth)
            });
            pixels.push([red, green, blue, alpha]);
        }
    }

    Ok(DecodedPixelBuffer::StraightRgba32F(
        DecodedStraightRgba32F::new(frame.width(), frame.height(), pixels)?,
    ))
}

#[derive(Clone, Copy, Debug)]
struct ChromaPosition {
    x: f32,
    y: f32,
}

impl ChromaPosition {
    fn from_frame(
        frame: &ffmpeg::util::frame::Video,
        conversion: H273YuvToRgb,
        horizontal_subsampling: u8,
        vertical_subsampling: u8,
    ) -> Result<Self, LibraryError> {
        if horizontal_subsampling == 0 && vertical_subsampling == 0 {
            return Ok(Self { x: 0.0, y: 0.0 });
        }
        let location = conversion
            .chroma_location(frame.chroma_location())
            .map_err(|reason| {
                LibraryError::FfmpegOther(format!(
                    "subsampled YUV format '{}' cannot resolve chroma location: {reason}",
                    pixel_format_name(frame.format())
                ))
            })?;
        let position = match location {
            YuvChromaLocation::Left => Self { x: 0.0, y: 0.5 },
            YuvChromaLocation::Center => Self { x: 0.5, y: 0.5 },
            YuvChromaLocation::TopLeft => Self { x: 0.0, y: 0.0 },
            YuvChromaLocation::Top => Self { x: 0.5, y: 0.0 },
            YuvChromaLocation::BottomLeft => Self { x: 0.0, y: 1.0 },
            YuvChromaLocation::Bottom => Self { x: 0.5, y: 1.0 },
        };
        Ok(position)
    }
}

struct PlanarYuvFrame<'a> {
    luma: IntegerPlane<'a>,
    cb: IntegerPlane<'a>,
    cr: IntegerPlane<'a>,
    alpha: Option<IntegerPlane<'a>>,
    width: usize,
    height: usize,
    depth: u8,
    horizontal_subsampling: u8,
    vertical_subsampling: u8,
    chroma_position: ChromaPosition,
}

impl<'a> PlanarYuvFrame<'a> {
    fn new(
        frame: &'a ffmpeg::util::frame::Video,
        conversion: H273YuvToRgb,
    ) -> Result<Self, LibraryError> {
        let format = frame.format();
        let descriptor = format.descriptor().ok_or_else(|| {
            LibraryError::FfmpegOther(format!(
                "YUV pixel format '{}' has no component descriptor",
                pixel_format_name(format)
            ))
        })?;
        validate_descriptor_flags(format)?;
        let component_count = usize::from(descriptor.nb_components());
        if !(3..=4).contains(&component_count) {
            return Err(unsupported_layout(
                format,
                format!("expected three YUV components and optional alpha, got {component_count}"),
            ));
        }
        let horizontal_subsampling = descriptor.log2_chroma_w();
        let vertical_subsampling = descriptor.log2_chroma_h();
        if !matches!(
            (horizontal_subsampling, vertical_subsampling),
            (0, 0) | (1, 0) | (1, 1)
        ) {
            return Err(unsupported_layout(
                format,
                format!(
                    "only planar 4:4:4, 4:2:2, and 4:2:0 are supported, got log2 subsampling {horizontal_subsampling}:{vertical_subsampling}"
                ),
            ));
        }
        let width = usize::try_from(frame.width())
            .map_err(|_| LibraryError::FfmpegOther("decoded width exceeds usize".to_string()))?;
        let height = usize::try_from(frame.height())
            .map_err(|_| LibraryError::FfmpegOther("decoded height exceeds usize".to_string()))?;
        let components = component_descriptors(descriptor.as_ptr(), component_count);
        let depth = component_depth(components[0], format, 0)?;
        if !SUPPORTED_DEPTHS.contains(&depth) {
            return Err(unsupported_layout(
                format,
                format!("component depth {depth} is not one of 8/9/10/12/14/16"),
            ));
        }
        for (index, component) in components.iter().copied().enumerate().skip(1) {
            let actual = component_depth(component, format, index)?;
            if actual != depth {
                return Err(unsupported_layout(
                    format,
                    format!("component {index} depth {actual} differs from luma depth {depth}"),
                ));
            }
        }
        let big_endian = pixel_format_has_flag(format, ffmpeg::ffi::AV_PIX_FMT_FLAG_BE);
        let luma = IntegerPlane::new(frame, components[0], 0, width, height, depth, big_endian)?;
        let chroma_width = width.div_ceil(1_usize << horizontal_subsampling);
        let chroma_height = height.div_ceil(1_usize << vertical_subsampling);
        let cb = IntegerPlane::new(
            frame,
            components[1],
            1,
            chroma_width,
            chroma_height,
            depth,
            big_endian,
        )?;
        let cr = IntegerPlane::new(
            frame,
            components[2],
            2,
            chroma_width,
            chroma_height,
            depth,
            big_endian,
        )?;
        let alpha = components
            .get(3)
            .copied()
            .map(|component| {
                IntegerPlane::new(frame, component, 3, width, height, depth, big_endian)
            })
            .transpose()?;
        let chroma_position = ChromaPosition::from_frame(
            frame,
            conversion,
            horizontal_subsampling,
            vertical_subsampling,
        )?;
        Ok(Self {
            luma,
            cb,
            cr,
            alpha,
            width,
            height,
            depth,
            horizontal_subsampling,
            vertical_subsampling,
            chroma_position,
        })
    }

    fn chroma_sample(
        &self,
        plane: &IntegerPlane<'_>,
        column: usize,
        row: usize,
    ) -> Result<f32, LibraryError> {
        let horizontal_scale = (1_u32 << self.horizontal_subsampling) as f32;
        let vertical_scale = (1_u32 << self.vertical_subsampling) as f32;
        let x = (column as f32 - self.chroma_position.x) / horizontal_scale;
        let y = (row as f32 - self.chroma_position.y) / vertical_scale;
        bilinear_sample(plane, x, y)
    }
}

#[derive(Clone, Copy)]
struct IntegerPlane<'a> {
    data: &'a [u8],
    stride: usize,
    width: usize,
    height: usize,
    step: usize,
    byte_width: usize,
    big_endian: bool,
}

impl<'a> IntegerPlane<'a> {
    #[allow(
        clippy::too_many_arguments,
        reason = "one validated descriptor component needs its frame geometry and sample contract"
    )]
    fn new(
        frame: &'a ffmpeg::util::frame::Video,
        component: ffmpeg::ffi::AVComponentDescriptor,
        component_index: usize,
        width: usize,
        height: usize,
        depth: u8,
        big_endian: bool,
    ) -> Result<Self, LibraryError> {
        let format = frame.format();
        let plane = usize::try_from(component.plane).map_err(|_| {
            unsupported_layout(
                format,
                format!("component {component_index} has a negative plane"),
            )
        })?;
        if plane != component_index || plane >= frame.planes() {
            return Err(unsupported_layout(
                format,
                format!(
                    "component {component_index} is not in its independent planar plane (got {plane})"
                ),
            ));
        }
        let step = usize::try_from(component.step)
            .ok()
            .filter(|step| *step > 0)
            .ok_or_else(|| {
                unsupported_layout(
                    format,
                    format!("component {component_index} has invalid step"),
                )
            })?;
        let offset = usize::try_from(component.offset).map_err(|_| {
            unsupported_layout(
                format,
                format!("component {component_index} has negative offset"),
            )
        })?;
        if offset != 0 || component.shift != 0 {
            return Err(unsupported_layout(
                format,
                format!(
                    "component {component_index} uses unsupported offset/shift {offset}/{}",
                    component.shift
                ),
            ));
        }
        let byte_width = usize::from(depth).div_ceil(8);
        if step != byte_width {
            return Err(unsupported_layout(
                format,
                format!(
                    "component {component_index} is not tightly planar: step {step}, sample bytes {byte_width}"
                ),
            ));
        }
        // SAFETY: `frame` is a live borrowed FFmpeg frame for the duration of
        // this validation and decode operation.
        let frame_ptr = unsafe { frame.as_ptr() };
        // SAFETY: a borrowed Video owns a live AVFrame. The checked plane index
        // is within both fixed AVFrame arrays, and only scalar/pointer fields
        // are copied before any slice is requested from ffmpeg-next.
        let (data_ptr, signed_stride) = unsafe {
            (
                std::ptr::addr_of!((*frame_ptr).data[plane]).read(),
                std::ptr::addr_of!((*frame_ptr).linesize[plane]).read(),
            )
        };
        if data_ptr.is_null() || signed_stride <= 0 {
            return Err(unsupported_layout(
                format,
                format!(
                    "component {component_index} has a null plane or non-positive stride {signed_stride}"
                ),
            ));
        }
        let stride = usize::try_from(signed_stride).map_err(|_| {
            unsupported_layout(
                format,
                format!("component {component_index} stride overflow"),
            )
        })?;
        let row_bytes = width
            .checked_sub(1)
            .and_then(|last| last.checked_mul(step))
            .and_then(|last| last.checked_add(byte_width))
            .ok_or_else(|| unsupported_layout(format, "empty or overflowing component row"))?;
        if stride < row_bytes {
            return Err(unsupported_layout(
                format,
                format!(
                    "component {component_index} stride {stride} is shorter than row {row_bytes}"
                ),
            ));
        }
        let required = height
            .checked_sub(1)
            .and_then(|last| last.checked_mul(stride))
            .and_then(|last| last.checked_add(row_bytes))
            .ok_or_else(|| unsupported_layout(format, "empty or overflowing component plane"))?;
        let plane_span = stride.checked_mul(height).ok_or_else(|| {
            unsupported_layout(
                format,
                format!("component {component_index} plane span overflow"),
            )
        })?;
        if plane_span > isize::MAX as usize || plane_span < required {
            return Err(unsupported_layout(
                format,
                format!(
                    "component {component_index} plane span {plane_span} cannot cover {required} bytes"
                ),
            ));
        }
        // SAFETY: FFmpeg-owned AVFrames guarantee each non-null data pointer
        // covers `linesize * plane_height` bytes. We validated the descriptor
        // plane, positive linesize, checked span arithmetic, and `isize::MAX`.
        // Corrupt foreign AVFrame allocation provenance cannot be proven by
        // Rust and is intentionally outside this loader's accepted boundary.
        let data = unsafe { std::slice::from_raw_parts(data_ptr, plane_span) };
        Ok(Self {
            data,
            stride,
            width,
            height,
            step,
            byte_width,
            big_endian,
        })
    }

    fn read(self, column: usize, row: usize) -> u32 {
        debug_assert!(column < self.width && row < self.height);
        let start = row * self.stride + column * self.step;
        let bytes = &self.data[start..start + self.byte_width];
        if self.big_endian {
            bytes
                .iter()
                .fold(0_u32, |value, byte| (value << 8) | u32::from(*byte))
        } else {
            bytes
                .iter()
                .rev()
                .fold(0_u32, |value, byte| (value << 8) | u32::from(*byte))
        }
    }
}

fn bilinear_sample(plane: &IntegerPlane<'_>, x: f32, y: f32) -> Result<f32, LibraryError> {
    if plane.width == 0 || plane.height == 0 {
        return Err(LibraryError::FfmpegOther("empty chroma plane".to_string()));
    }
    let x0 = x.floor();
    let y0 = y.floor();
    let tx = x - x0;
    let ty = y - y0;
    let clamp = |value: f32, maximum: usize| -> usize {
        value.max(0.0).min(maximum.saturating_sub(1) as f32) as usize
    };
    let left = clamp(x0, plane.width);
    let right = clamp(x0 + 1.0, plane.width);
    let top = clamp(y0, plane.height);
    let bottom = clamp(y0 + 1.0, plane.height);
    let top_value = plane.read(left, top) as f32 * (1.0 - tx) + plane.read(right, top) as f32 * tx;
    let bottom_value =
        plane.read(left, bottom) as f32 * (1.0 - tx) + plane.read(right, bottom) as f32 * tx;
    Ok(top_value * (1.0 - ty) + bottom_value * ty)
}

fn normalize_luma(code: f32, depth: u8, range: H273Range) -> f32 {
    match range {
        H273Range::Full => code / code_max(depth),
        H273Range::Limited => {
            let scale = limited_scale(depth);
            (code - 16.0 * scale) / (219.0 * scale)
        }
    }
}

fn normalize_chroma(code: f32, depth: u8, range: H273Range) -> f32 {
    let center = (1_u32 << (depth - 1)) as f32;
    match range {
        H273Range::Full => (code - center) / code_max(depth),
        H273Range::Limited => (code - center) / (224.0 * limited_scale(depth)),
    }
}

fn code_max(depth: u8) -> f32 {
    ((1_u32 << depth) - 1) as f32
}

fn limited_scale(depth: u8) -> f32 {
    (1_u32 << (depth - 8)) as f32
}

fn component_descriptors(
    descriptor: *const ffmpeg::ffi::AVPixFmtDescriptor,
    count: usize,
) -> Vec<ffmpeg::ffi::AVComponentDescriptor> {
    let mut components = Vec::with_capacity(count);
    for index in 0..count {
        // SAFETY: AVPixFmtDescriptor is an immutable process-lifetime table
        // entry. `count` came from `nb_components` and is checked to be <= 4,
        // the fixed size of `comp`.
        let component = unsafe {
            std::ptr::addr_of!((*descriptor).comp)
                .cast::<ffmpeg::ffi::AVComponentDescriptor>()
                .add(index)
                .read()
        };
        components.push(component);
    }
    components
}

fn component_depth(
    component: ffmpeg::ffi::AVComponentDescriptor,
    format: ffmpeg::format::Pixel,
    index: usize,
) -> Result<u8, LibraryError> {
    u8::try_from(component.depth)
        .ok()
        .filter(|depth| *depth > 0)
        .ok_or_else(|| unsupported_layout(format, format!("component {index} has invalid depth")))
}

fn validate_descriptor_flags(format: ffmpeg::format::Pixel) -> Result<(), LibraryError> {
    let planar = pixel_format_has_flag(format, ffmpeg::ffi::AV_PIX_FMT_FLAG_PLANAR);
    let forbidden = [
        ffmpeg::ffi::AV_PIX_FMT_FLAG_RGB,
        ffmpeg::ffi::AV_PIX_FMT_FLAG_FLOAT,
        ffmpeg::ffi::AV_PIX_FMT_FLAG_PAL,
        ffmpeg::ffi::AV_PIX_FMT_FLAG_BITSTREAM,
        ffmpeg::ffi::AV_PIX_FMT_FLAG_HWACCEL,
        ffmpeg::ffi::AV_PIX_FMT_FLAG_BAYER,
    ]
    .into_iter()
    .any(|flag| pixel_format_has_flag(format, flag));
    if !planar || forbidden {
        return Err(unsupported_layout(
            format,
            "only software-backed planar integer YUV/YUVA is supported",
        ));
    }
    Ok(())
}

fn pixel_format_has_flag(format: ffmpeg::format::Pixel, flag: i32) -> bool {
    let Some(descriptor) = format.descriptor() else {
        return false;
    };
    let Ok(flag) = u64::try_from(flag) else {
        return false;
    };
    // SAFETY: FFmpeg pixel descriptors are immutable process-lifetime table
    // entries. Only the scalar flags field is copied.
    let flags = unsafe { std::ptr::addr_of!((*descriptor.as_ptr()).flags).read() };
    flags & flag != 0
}

fn unsupported_layout(
    format: ffmpeg::format::Pixel,
    detail: impl std::fmt::Display,
) -> LibraryError {
    LibraryError::FfmpegOther(format!(
        "unsupported verified YUV layout '{}': {detail}",
        pixel_format_name(format)
    ))
}

fn pixel_format_name(format: ffmpeg::format::Pixel) -> &'static str {
    format
        .descriptor()
        .map_or("unknown-pixel-format", |descriptor| descriptor.name())
}
