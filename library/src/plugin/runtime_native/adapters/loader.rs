use std::mem::size_of;
use std::path::Path;

use ruvie_plugin_api::{
    ASSET_KIND_IMAGE_V1, ASSET_KIND_VIDEO_V1, ASSET_METADATA_DIMENSIONS_V1,
    ASSET_METADATA_DURATION_V1, ASSET_METADATA_FPS_V1, ASSET_METADATA_FRAME_COUNT_V1,
    ASSET_METADATA_STREAM_INDEX_V1, ASSET_METADATA_TIME_BASE_V1, LOAD_REQUEST_IMAGE_V1,
    LOAD_REQUEST_VIDEO_FRAME_V1, LOADER_LOAD_CPU_RGBA8_V1, MAX_CPU_RGBA8_DIMENSION_V1,
    MAX_LOADER_STREAMS_V1, RuvieAssetMetadataV1, RuvieBytesView, RuvieLoaderCpuRgba8ApiV1,
    RuvieLoaderRequestV1, RuvieOwnedRgba8FrameV1,
};

use super::super::abi::{ExtensionStatus, RuntimeComponent};
use super::super::property_wire::empty_bytes_view;
use super::super::rgba8::{copy_owned_frame, reclaim_owned_frame};
use super::parse_semver_triplet;
use crate::error::LibraryError;
use crate::plugin::loaders::ffmpeg_video::FileIdentity;
use crate::plugin::{
    AssetMetadata, DecodedPixelDescription, LoadPlugin, LoadPluginError, LoadPluginResult,
    LoadRequest, LoadResponse, Plugin,
};
pub(in crate::plugin::runtime_native) struct RuntimeLoaderPlugin {
    pub(in crate::plugin::runtime_native) component: RuntimeComponent,
    pub(in crate::plugin::runtime_native) api: RuvieLoaderCpuRgba8ApiV1,
}

impl Plugin for RuntimeLoaderPlugin {
    fn id(&self) -> &str {
        &self.component.descriptor.id
    }

    fn name(&self) -> String {
        self.component.descriptor.name.clone()
    }

    fn category(&self) -> String {
        self.component.descriptor.group.clone()
    }

    fn version(&self) -> (u32, u32, u32) {
        parse_semver_triplet(&self.component.descriptor.version)
    }

    fn impl_type(&self) -> String {
        "Native ABI v1 / CPU RGBA8".to_string()
    }
}

impl LoadPlugin for RuntimeLoaderPlugin {
    fn open(&self, path: &str) -> LoadPluginResult<Vec<AssetMetadata>> {
        let open = self.api.open.ok_or_else(|| {
            LoadPluginError::Failed(LibraryError::Plugin(format!(
                "Runtime Loader '{}' open callback is missing",
                self.id()
            )))
        })?;
        let mut metadata = vec![RuvieAssetMetadataV1::default(); MAX_LOADER_STREAMS_V1];
        let mut metadata_len = usize::MAX;
        // SAFETY: The path is borrowed for the call. The fixed-size metadata
        // allocation and its length output are writable host-owned memory.
        let result = unsafe {
            open(
                self.api.context,
                RuvieBytesView::from_slice(self.id().as_bytes()),
                RuvieBytesView::from_slice(path.as_bytes()),
                metadata.as_mut_ptr(),
                metadata.len(),
                &mut metadata_len,
            )
        };
        match self.component.library.consume_extension_result(result) {
            Ok(ExtensionStatus::Unsupported(_)) => return Err(LoadPluginError::Unsupported),
            Ok(ExtensionStatus::Ok) => {}
            Err(error) => return Err(self.failed(path, "inspect", error)),
        }
        if metadata_len == 0 || metadata_len > metadata.len() {
            return Err(self.failed(
                path,
                "inspect",
                LibraryError::Plugin(format!(
                    "returned invalid metadata length {metadata_len} (capacity {})",
                    metadata.len()
                )),
            ));
        }
        metadata
            .into_iter()
            .take(metadata_len)
            .map(|value| {
                metadata_from_wire(value).map_err(|error| self.failed(path, "inspect", error))
            })
            .collect()
    }

    fn load(
        &self,
        request: &LoadRequest,
        cache: &crate::cache::CacheManager,
    ) -> LoadPluginResult<LoadResponse> {
        let cache_key = runtime_loader_cache_key(self.id(), request);
        let cached = match request {
            LoadRequest::Image { .. } => cache.get_image(&cache_key),
            LoadRequest::VideoFrame { source_time, .. } => {
                cache.get_video_frame(&cache_key, source_time_bits(*source_time))
            }
        };
        if let Some(image) = cached {
            return Ok(LoadResponse {
                image,
                decoded: DecodedPixelDescription::abi_v1_srgb_rgba8(),
            });
        }

        let (wire, _borrowed) = loader_request_to_wire(request)
            .map_err(|error| self.failed(request.path(), "decode", error))?;
        let load = self.api.load.ok_or_else(|| {
            self.failed(
                request.path(),
                "decode",
                LibraryError::Plugin(format!(
                    "Runtime Loader '{}' load callback is missing",
                    self.id()
                )),
            )
        })?;
        let mut output = RuvieOwnedRgba8FrameV1::empty();
        // SAFETY: Every byte view in `wire` borrows `request`, which remains
        // alive for the callback. Output starts in the empty ownership state.
        let result = unsafe {
            load(
                self.api.context,
                RuvieBytesView::from_slice(self.id().as_bytes()),
                &wire,
                &mut output,
            )
        };
        match self.component.library.consume_extension_result(result) {
            Ok(ExtensionStatus::Unsupported(_)) => {
                reclaim_owned_frame(self.api.context, self.api.free_frame, output);
                return Err(LoadPluginError::Unsupported);
            }
            Ok(ExtensionStatus::Ok) => {}
            Err(error) => {
                reclaim_owned_frame(self.api.context, self.api.free_frame, output);
                return Err(self.failed(request.path(), "decode", error));
            }
        }
        let image = copy_owned_frame(self.api.context, self.api.free_frame, output)
            .map_err(|error| self.failed(request.path(), "decode", error))?;
        match request {
            LoadRequest::Image { .. } => cache.put_image(&cache_key, &image),
            LoadRequest::VideoFrame { source_time, .. } => {
                cache.put_video_frame(&cache_key, source_time_bits(*source_time), &image);
            }
        }
        Ok(LoadResponse {
            image,
            decoded: DecodedPixelDescription::abi_v1_srgb_rgba8(),
        })
    }
}

impl RuntimeLoaderPlugin {
    fn failed(&self, path: &str, action: &str, cause: LibraryError) -> LoadPluginError {
        LoadPluginError::Failed(LibraryError::Plugin(format!(
            "Runtime Loader '{}' failed to {action} path {:?}: {cause}",
            self.id(),
            path
        )))
    }
}

pub(in crate::plugin::runtime_native) fn metadata_from_wire(
    value: RuvieAssetMetadataV1,
) -> Result<AssetMetadata, LibraryError> {
    const KNOWN_FIELDS: u32 = ASSET_METADATA_DURATION_V1
        | ASSET_METADATA_FPS_V1
        | ASSET_METADATA_DIMENSIONS_V1
        | ASSET_METADATA_STREAM_INDEX_V1
        | ASSET_METADATA_FRAME_COUNT_V1
        | ASSET_METADATA_TIME_BASE_V1;
    if value.present_fields & !KNOWN_FIELDS != 0 {
        return Err(LibraryError::Plugin(format!(
            "Runtime Loader metadata has unknown field bits {:#x}",
            value.present_fields & !KNOWN_FIELDS
        )));
    }
    let kind = match value.kind {
        ASSET_KIND_IMAGE_V1 => crate::model::asset::AssetKind::Image,
        ASSET_KIND_VIDEO_V1 => crate::model::asset::AssetKind::Video,
        other => {
            return Err(LibraryError::Plugin(format!(
                "Runtime Loader metadata has unsupported asset kind {other}"
            )));
        }
    };
    let has = |field| value.present_fields & field != 0;
    let duration = has(ASSET_METADATA_DURATION_V1).then_some(value.duration_seconds);
    if duration.is_some_and(|duration| !duration.is_finite() || duration < 0.0) {
        return Err(LibraryError::Plugin(
            "Runtime Loader metadata duration must be finite and non-negative".to_string(),
        ));
    }
    let fps = has(ASSET_METADATA_FPS_V1).then_some(value.fps);
    if fps.is_some_and(|fps| !fps.is_finite() || fps <= 0.0) {
        return Err(LibraryError::Plugin(
            "Runtime Loader metadata FPS must be finite and positive".to_string(),
        ));
    }
    let (width, height) = if has(ASSET_METADATA_DIMENSIONS_V1) {
        if value.width == 0
            || value.height == 0
            || value.width > MAX_CPU_RGBA8_DIMENSION_V1
            || value.height > MAX_CPU_RGBA8_DIMENSION_V1
        {
            return Err(LibraryError::Plugin(format!(
                "Runtime Loader metadata dimensions {}x{} are invalid",
                value.width, value.height
            )));
        }
        (Some(value.width), Some(value.height))
    } else {
        (None, None)
    };
    let stream_index = has(ASSET_METADATA_STREAM_INDEX_V1)
        .then(|| usize::try_from(value.stream_index))
        .transpose()
        .map_err(|_| LibraryError::Plugin("Runtime Loader stream index overflow".to_string()))?;
    let frame_count = has(ASSET_METADATA_FRAME_COUNT_V1).then_some(value.frame_count);
    let time_base = if has(ASSET_METADATA_TIME_BASE_V1) {
        if value.time_base_denominator == 0 || value.time_base_numerator == 0 {
            return Err(LibraryError::Plugin(
                "Runtime Loader metadata time base must have non-zero terms".to_string(),
            ));
        }
        Some((value.time_base_numerator, value.time_base_denominator))
    } else {
        None
    };
    Ok(AssetMetadata {
        kind,
        duration,
        fps,
        width,
        height,
        stream_index,
        frame_count,
        time_base,
        // Loader ABI v1 predates source color metadata. Runtime plugins leave
        // it explicitly unknown until a future ABI can transport these tags.
        source_color: crate::model::asset::SourceColorDescription::default(),
    })
}

fn loader_request_to_wire(
    request: &LoadRequest,
) -> Result<(RuvieLoaderRequestV1, Vec<&str>), LibraryError> {
    let (request_kind, path, source_time, stream_index, input, output) = match request {
        LoadRequest::Image { path } => (LOAD_REQUEST_IMAGE_V1, path, 0.0, None, None, None),
        LoadRequest::VideoFrame {
            path,
            source_time,
            stream_index,
            input_color_space,
            output_color_space,
        } => {
            if !source_time.is_finite() || *source_time < 0.0 {
                return Err(LibraryError::Plugin(format!(
                    "Runtime Loader source time {source_time} is invalid"
                )));
            }
            (
                LOAD_REQUEST_VIDEO_FRAME_V1,
                path,
                *source_time,
                *stream_index,
                input_color_space.as_deref(),
                output_color_space.as_deref(),
            )
        }
    };
    let stream_index = stream_index
        .map(u32::try_from)
        .transpose()
        .map_err(|_| LibraryError::Plugin("Runtime Loader stream index exceeds u32".to_string()))?;
    let borrowed = vec![path.as_str(), input.unwrap_or(""), output.unwrap_or("")];
    Ok((
        RuvieLoaderRequestV1 {
            struct_size: size_of::<RuvieLoaderRequestV1>(),
            request_kind,
            path: RuvieBytesView::from_slice(borrowed[0].as_bytes()),
            source_time,
            has_stream_index: u32::from(stream_index.is_some()),
            stream_index: stream_index.unwrap_or(0),
            input_color_space: if input.is_some() {
                RuvieBytesView::from_slice(borrowed[1].as_bytes())
            } else {
                empty_bytes_view()
            },
            output_color_space: if output.is_some() {
                RuvieBytesView::from_slice(borrowed[2].as_bytes())
            } else {
                empty_bytes_view()
            },
        },
        borrowed,
    ))
}

pub(in crate::plugin::runtime_native) fn runtime_loader_cache_key(
    loader_id: &str,
    request: &LoadRequest,
) -> String {
    let path = request.path();
    let identity = FileIdentity::read(path).ok();
    let canonical_path = identity
        .as_ref()
        .map_or_else(|| Path::new(path), |identity| identity.canonical_path());
    let source_identity = identity
        .as_ref()
        .map_or_else(|| "unavailable".to_string(), FileIdentity::cache_token);
    match request {
        LoadRequest::Image { .. } => format!(
            "runtime-loader:{loader_id}:{LOADER_LOAD_CPU_RGBA8_V1}:image:{}:{source_identity}",
            canonical_path.display()
        ),
        LoadRequest::VideoFrame {
            stream_index,
            input_color_space,
            output_color_space,
            ..
        } => format!(
            "runtime-loader:{loader_id}:{LOADER_LOAD_CPU_RGBA8_V1}:video:{}:{source_identity}:stream={stream_index:?}:input={input_color_space:?}:output={output_color_space:?}",
            canonical_path.display()
        ),
    }
}

pub(in crate::plugin::runtime_native) fn source_time_bits(source_time: f64) -> i64 {
    i64::from_ne_bytes(source_time.to_bits().to_ne_bytes())
}
