//! CPU RGBA8 image and video fixture loader extension.

use std::ffi::c_void;

use ruvie_plugin_api::{
    ComponentDescriptorV1, RuvieAssetMetadataV1, RuvieBuffer, RuvieBytesView,
    RuvieExtensionResultV1, RuvieLoaderCpuRgba8ApiV1, RuvieLoaderRequestV1, RuvieOwnedRgba8FrameV1,
    ALPHA_MODE_STRAIGHT_V1, ASSET_KIND_IMAGE_V1, ASSET_KIND_VIDEO_V1, ASSET_METADATA_DIMENSIONS_V1,
    ASSET_METADATA_DURATION_V1, ASSET_METADATA_FPS_V1, ASSET_METADATA_FRAME_COUNT_V1,
    ASSET_METADATA_STREAM_INDEX_V1, ASSET_METADATA_TIME_BASE_V1, COLOR_PROFILE_SRGB_V1,
    LOADER_CATEGORY, LOADER_LOAD_CPU_RGBA8_V1, LOADER_OPEN_V1, LOAD_REQUEST_IMAGE_V1,
    LOAD_REQUEST_VIDEO_FRAME_V1, MAX_CPU_RGBA8_DIMENSION_V1, MAX_CPU_RGBA8_FRAME_BYTES_V1,
    RUVIE_PLUGIN_ABI_V1, STATUS_PLUGIN_ERROR,
};

use crate::abi::{extension_guard, free_frame, invalid_extension, utf8_from_view};

pub(super) const COMPONENT_ID: &str = "runtime_rgba_fixture_loader";

const IMAGE_FIXTURE_MAGIC: &[u8; 8] = b"RUVRGBA1";
const VIDEO_FIXTURE_MAGIC: &[u8; 8] = b"RUVVID01";
const IMAGE_FIXTURE_SUFFIX: &str = ".rgba-fixture";
const VIDEO_FIXTURE_SUFFIX: &str = ".rgba-video-fixture";
const VIDEO_DURATION_SECONDS: f64 = 2.0;
const VIDEO_FPS: f64 = 24.0;
const VIDEO_FRAME_COUNT: u64 = 48;

pub(super) fn descriptor() -> ComponentDescriptorV1 {
    ComponentDescriptorV1 {
        id: COMPONENT_ID.to_string(),
        name: "Runtime RGBA Fixture Loader".to_string(),
        category: LOADER_CATEGORY.to_string(),
        group: "Loader/Runtime Fixture".to_string(),
        version: "0.1.0".to_string(),
        operations: vec![
            LOADER_OPEN_V1.to_string(),
            LOADER_LOAD_CPU_RGBA8_V1.to_string(),
        ],
        properties: Vec::new(),
        output_default: None,
    }
}

enum FixtureRequest {
    Image,
    Video {
        source_time: f64,
        stream_index: u32,
        input_color_space: String,
        output_color_space: String,
    },
}

struct RgbaFixture {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
    request: FixtureRequest,
}

fn fixture_path_is_supported(path: &str) -> bool {
    path.ends_with(IMAGE_FIXTURE_SUFFIX) || path.ends_with(VIDEO_FIXTURE_SUFFIX)
}

fn read_rgba_fixture(path: &str) -> Result<RgbaFixture, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("could not read fixture: {error}"))?;
    if bytes.len() < 16 {
        return Err("fixture header magic is invalid".to_string());
    }
    let width = u32::from_le_bytes(bytes[8..12].try_into().unwrap_or_default());
    let height = u32::from_le_bytes(bytes[12..16].try_into().unwrap_or_default());
    if width == 0
        || height == 0
        || width > MAX_CPU_RGBA8_DIMENSION_V1
        || height > MAX_CPU_RGBA8_DIMENSION_V1
    {
        return Err(format!("fixture dimensions {width}x{height} are invalid"));
    }
    let expected_pixels = usize::try_from(width)
        .ok()
        .and_then(|width| width.checked_mul(4))
        .and_then(|row| row.checked_mul(usize::try_from(height).unwrap_or(usize::MAX)))
        .ok_or_else(|| "fixture pixel length overflow".to_string())?;
    if expected_pixels > MAX_CPU_RGBA8_FRAME_BYTES_V1 {
        return Err("fixture pixel length exceeds the ABI limit".to_string());
    }

    let (request, pixels_offset) = if &bytes[..8] == IMAGE_FIXTURE_MAGIC {
        (FixtureRequest::Image, 16)
    } else if &bytes[..8] == VIDEO_FIXTURE_MAGIC {
        if bytes.len() < 32 {
            return Err("video fixture request header is truncated".to_string());
        }
        let source_time = f64::from_le_bytes(bytes[16..24].try_into().unwrap_or_default());
        if !source_time.is_finite() || source_time < 0.0 {
            return Err("video fixture source time is invalid".to_string());
        }
        let stream_index = u32::from_le_bytes(bytes[24..28].try_into().unwrap_or_default());
        let input_len = usize::from(u16::from_le_bytes(
            bytes[28..30].try_into().unwrap_or_default(),
        ));
        let output_len = usize::from(u16::from_le_bytes(
            bytes[30..32].try_into().unwrap_or_default(),
        ));
        let input_end = 32_usize
            .checked_add(input_len)
            .ok_or_else(|| "video fixture request header length overflow".to_string())?;
        let output_end = input_end
            .checked_add(output_len)
            .ok_or_else(|| "video fixture request header length overflow".to_string())?;
        if bytes.len() < output_end {
            return Err("video fixture request header is truncated".to_string());
        }
        let input_color_space = std::str::from_utf8(&bytes[32..input_end])
            .map_err(|error| format!("video fixture input color space is invalid: {error}"))?
            .to_string();
        let output_color_space = std::str::from_utf8(&bytes[input_end..output_end])
            .map_err(|error| format!("video fixture output color space is invalid: {error}"))?
            .to_string();
        (
            FixtureRequest::Video {
                source_time,
                stream_index,
                input_color_space,
                output_color_space,
            },
            output_end,
        )
    } else {
        return Err("fixture header magic is invalid".to_string());
    };
    let expected_len = pixels_offset
        .checked_add(expected_pixels)
        .ok_or_else(|| "fixture payload length overflow".to_string())?;
    if bytes.len() != expected_len {
        return Err(format!(
            "fixture payload length {} does not match expected {expected_pixels}",
            bytes.len().saturating_sub(pixels_offset)
        ));
    }
    Ok(RgbaFixture {
        width,
        height,
        pixels: bytes[pixels_offset..].to_vec(),
        request,
    })
}

unsafe extern "C" fn open(
    _context: *mut c_void,
    component_id: RuvieBytesView,
    path: RuvieBytesView,
    metadata: *mut RuvieAssetMetadataV1,
    metadata_capacity: usize,
    out_metadata_len: *mut usize,
) -> RuvieExtensionResultV1 {
    extension_guard(|| {
        // SAFETY: IDs and paths are callback-scoped host byte views.
        let component_id = match unsafe { utf8_from_view(component_id) } {
            Ok(value) => value,
            Err(error) => return invalid_extension(error),
        };
        // SAFETY: Same borrowed-view contract as above.
        let path = match unsafe { utf8_from_view(path) } {
            Ok(value) => value,
            Err(error) => return invalid_extension(error),
        };
        if component_id != COMPONENT_ID || !fixture_path_is_supported(path) {
            return RuvieExtensionResultV1::unsupported();
        }
        if metadata.is_null() || out_metadata_len.is_null() || metadata_capacity < 1 {
            return invalid_extension("Loader metadata output is invalid");
        }
        let fixture = match read_rgba_fixture(path) {
            Ok(value) => value,
            Err(error) => return RuvieExtensionResultV1::error(STATUS_PLUGIN_ERROR, error),
        };
        let value = match &fixture.request {
            FixtureRequest::Image if path.ends_with(IMAGE_FIXTURE_SUFFIX) => RuvieAssetMetadataV1 {
                kind: ASSET_KIND_IMAGE_V1,
                present_fields: ASSET_METADATA_DIMENSIONS_V1,
                width: fixture.width,
                height: fixture.height,
                ..RuvieAssetMetadataV1::default()
            },
            FixtureRequest::Video { stream_index, .. } if path.ends_with(VIDEO_FIXTURE_SUFFIX) => {
                RuvieAssetMetadataV1 {
                    kind: ASSET_KIND_VIDEO_V1,
                    present_fields: ASSET_METADATA_DURATION_V1
                        | ASSET_METADATA_FPS_V1
                        | ASSET_METADATA_DIMENSIONS_V1
                        | ASSET_METADATA_STREAM_INDEX_V1
                        | ASSET_METADATA_FRAME_COUNT_V1
                        | ASSET_METADATA_TIME_BASE_V1,
                    duration_seconds: VIDEO_DURATION_SECONDS,
                    fps: VIDEO_FPS,
                    width: fixture.width,
                    height: fixture.height,
                    stream_index: *stream_index,
                    frame_count: VIDEO_FRAME_COUNT,
                    time_base_numerator: 1,
                    time_base_denominator: VIDEO_FPS as i32,
                }
            }
            FixtureRequest::Image | FixtureRequest::Video { .. } => {
                return RuvieExtensionResultV1::error(
                    STATUS_PLUGIN_ERROR,
                    "fixture magic does not match its path suffix",
                );
            }
        };
        // SAFETY: Capacity is at least one and both output pointers were
        // checked. The host initialized and owns this memory.
        unsafe {
            *metadata = value;
            *out_metadata_len = 1;
        }
        RuvieExtensionResultV1::ok()
    })
}

unsafe extern "C" fn load(
    _context: *mut c_void,
    component_id: RuvieBytesView,
    request: *const RuvieLoaderRequestV1,
    output: *mut RuvieOwnedRgba8FrameV1,
) -> RuvieExtensionResultV1 {
    extension_guard(|| {
        if request.is_null() || output.is_null() {
            return invalid_extension("Loader request or output is null");
        }
        // SAFETY: Component IDs are callback-scoped host byte views.
        let component_id = match unsafe { utf8_from_view(component_id) } {
            Ok(value) => value,
            Err(error) => return invalid_extension(error),
        };
        // SAFETY: The request pointer is host-owned for this callback.
        let request = unsafe { &*request };
        if request.struct_size < std::mem::size_of::<RuvieLoaderRequestV1>() {
            return invalid_extension("Loader request table is truncated");
        }
        // SAFETY: The request path is callback-scoped host memory.
        let path = match unsafe { utf8_from_view(request.path) } {
            Ok(value) => value,
            Err(error) => return invalid_extension(error),
        };
        if component_id != COMPONENT_ID || !fixture_path_is_supported(path) {
            return RuvieExtensionResultV1::unsupported();
        }
        let fixture = match read_rgba_fixture(path) {
            Ok(value) => value,
            Err(error) => return RuvieExtensionResultV1::error(STATUS_PLUGIN_ERROR, error),
        };
        match &fixture.request {
            FixtureRequest::Image
                if request.request_kind == LOAD_REQUEST_IMAGE_V1
                    && path.ends_with(IMAGE_FIXTURE_SUFFIX) => {}
            FixtureRequest::Video {
                source_time,
                stream_index,
                input_color_space,
                output_color_space,
            } if request.request_kind == LOAD_REQUEST_VIDEO_FRAME_V1
                && path.ends_with(VIDEO_FIXTURE_SUFFIX) =>
            {
                // SAFETY: Color-space names are callback-scoped host views.
                let input = match unsafe { utf8_from_view(request.input_color_space) } {
                    Ok(value) => value,
                    Err(error) => return invalid_extension(error),
                };
                // SAFETY: Same borrowed-view contract as above.
                let output = match unsafe { utf8_from_view(request.output_color_space) } {
                    Ok(value) => value,
                    Err(error) => return invalid_extension(error),
                };
                if request.source_time.to_bits() != source_time.to_bits()
                    || request.has_stream_index != 1
                    || request.stream_index != *stream_index
                    || input != input_color_space
                    || output != output_color_space
                {
                    return RuvieExtensionResultV1::error(
                        STATUS_PLUGIN_ERROR,
                        format!(
                            "video request metadata mismatch: time={}, stream={:?}, input={input:?}, output={output:?}",
                            request.source_time,
                            (request.has_stream_index == 1).then_some(request.stream_index),
                        ),
                    );
                }
            }
            FixtureRequest::Image | FixtureRequest::Video { .. } => {
                return RuvieExtensionResultV1::unsupported();
            }
        }
        let stride_bytes = usize::try_from(fixture.width)
            .ok()
            .and_then(|width| width.checked_mul(4))
            .unwrap_or_default();
        // SAFETY: Output is uniquely writable and starts in the host-defined
        // empty ownership state.
        unsafe {
            *output = RuvieOwnedRgba8FrameV1 {
                struct_size: std::mem::size_of::<RuvieOwnedRgba8FrameV1>(),
                width: fixture.width,
                height: fixture.height,
                stride_bytes,
                alpha_mode: ALPHA_MODE_STRAIGHT_V1,
                color_profile: COLOR_PROFILE_SRGB_V1,
                pixels: RuvieBuffer::from_vec(fixture.pixels),
            }
        };
        RuvieExtensionResultV1::ok()
    })
}

pub(super) static API: RuvieLoaderCpuRgba8ApiV1 = RuvieLoaderCpuRgba8ApiV1 {
    abi_version: RUVIE_PLUGIN_ABI_V1,
    struct_size: std::mem::size_of::<RuvieLoaderCpuRgba8ApiV1>(),
    context: std::ptr::null_mut(),
    open: Some(open),
    load: Some(load),
    free_frame: Some(free_frame),
};
