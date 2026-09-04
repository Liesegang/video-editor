use crate::error::LibraryError;
use crate::plugin::{LoadPluginError, LoadPluginResult};
use ffmpeg_next as ffmpeg;
use std::collections::HashSet;
use std::ffi::{CStr, c_void};
use std::path::Path;
use std::sync::OnceLock;

struct FfmpegRuntime {
    initialization: Result<(), ffmpeg::Error>,
    demuxer_extensions: HashSet<String>,
}

fn collect_registered_demuxer_extensions() -> HashSet<String> {
    let mut extensions = HashSet::new();
    let mut opaque: *mut c_void = std::ptr::null_mut();
    loop {
        // SAFETY: `opaque` starts null and is passed back only to
        // `av_demuxer_iterate`, as required by the FFmpeg iterator API. The
        // process-wide runtime initializer serializes this registry walk with
        // FFmpeg's one initialization call.
        let input_format = unsafe { ffmpeg::ffi::av_demuxer_iterate(&mut opaque) };
        if input_format.is_null() {
            break;
        }
        // SAFETY: A non-null entry returned by `av_demuxer_iterate` points to
        // a registered `AVInputFormat` for the lifetime of libavformat.
        let extension_list = unsafe { (*input_format).extensions };
        if extension_list.is_null() {
            continue;
        }
        // SAFETY: `AVInputFormat.extensions` is either null or a
        // null-terminated, comma-separated string owned by libavformat.
        let extension_list = unsafe { CStr::from_ptr(extension_list) }.to_string_lossy();
        extensions.extend(
            extension_list
                .split(',')
                .map(str::trim)
                .filter(|extension| !extension.is_empty())
                .map(str::to_ascii_lowercase),
        );
    }
    extensions
}

fn runtime() -> &'static FfmpegRuntime {
    static RUNTIME: OnceLock<FfmpegRuntime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        #[cfg(test)]
        INITIALIZER_CALLS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let initialization = ffmpeg::init();
        let demuxer_extensions = collect_registered_demuxer_extensions();
        FfmpegRuntime {
            initialization,
            demuxer_extensions,
        }
    })
}

#[cfg(test)]
static INITIALIZER_CALLS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

#[cfg(test)]
pub(super) fn initializer_calls() -> usize {
    INITIALIZER_CALLS.load(std::sync::atomic::Ordering::SeqCst)
}

pub(super) fn initialize_ffmpeg() -> Result<(), LibraryError> {
    runtime().initialization.map_err(LibraryError::from)
}

fn registered_demuxer_extensions() -> &'static HashSet<String> {
    &runtime().demuxer_extensions
}

pub(super) fn has_registered_ffmpeg_media_extension(path: &str) -> bool {
    let extension = Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .map(str::to_ascii_lowercase);
    extension.is_some_and(|extension| registered_demuxer_extensions().contains(extension.as_str()))
}

pub(super) fn classify_ffmpeg_probe_failure(path: &str, error: LibraryError) -> LoadPluginError {
    if has_registered_ffmpeg_media_extension(path) {
        LoadPluginError::Failed(error)
    } else {
        LoadPluginError::Unsupported
    }
}

pub(super) fn initialize_ffmpeg_for_path(path: &str) -> LoadPluginResult<()> {
    initialize_ffmpeg().map_err(|error| classify_ffmpeg_probe_failure(path, error))
}
