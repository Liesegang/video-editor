use ffmpeg_next as ffmpeg;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LibraryError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Libloading error: {0}")]
    Libloading(#[from] libloading::Error),
    #[error("Plugin error: {0}")]
    Plugin(String),
    #[error("Image error: {0}")]
    Image(#[from] image::ImageError),
    #[error("FFmpeg error: {0}")]
    Ffmpeg(#[from] ffmpeg::Error),
    #[error("Other FFmpeg error: {0}")]
    FfmpegOther(String),
    #[error(
        "video frame out of range: path={path:?}, stream={stream_index}, requested={frame_number}, frame_count={frame_count}"
    )]
    VideoFrameOutOfRange {
        path: String,
        stream_index: usize,
        frame_number: u64,
        frame_count: u64,
    },
    #[error(
        "video frame decode failed: path={path:?}, stream={stream_index}, frame={frame_number}: {source}"
    )]
    VideoFrameDecode {
        path: String,
        stream_index: usize,
        frame_number: u64,
        #[source]
        source: Box<LibraryError>,
    },
    #[error(
        "video timestamp out of range: path={path:?}, stream={stream_index}, source_time={source_time}, duration={duration:?}"
    )]
    VideoTimestampOutOfRange {
        path: String,
        stream_index: usize,
        source_time: f64,
        duration: Option<f64>,
    },
    #[error(
        "video timestamp decode failed: path={path:?}, stream={stream_index}, source_time={source_time}: {source}"
    )]
    VideoTimestampDecode {
        path: String,
        stream_index: usize,
        source_time: f64,
        #[source]
        source: Box<LibraryError>,
    },
    #[error("Project error: {0}")]
    Project(String),
    #[error("Rendering error: {0}")]
    Render(String),
    #[error("Invalid composition index: {0}")]
    InvalidCompositionIndex(usize),
    #[error("Runtime error: {0}")]
    Runtime(String),
    #[error("Validation error: {0}")]
    Validation(String),
}

impl From<Box<dyn std::error::Error>> for LibraryError {
    fn from(err: Box<dyn std::error::Error>) -> Self {
        LibraryError::Runtime(err.to_string())
    }
}
