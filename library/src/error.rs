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
    Ffmpeg(String),
    #[error("Project error: {0}")]
    Project(String),
    #[error("Rendering error: {0}")]
    Render(String),
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),
    #[error("Runtime error: {0}")]
    Runtime(String),
}

impl From<Box<dyn std::error::Error>> for LibraryError {
    fn from(err: Box<dyn std::error::Error>) -> Self {
        LibraryError::Runtime(err.to_string())
    }
}
