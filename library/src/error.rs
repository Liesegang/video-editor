use ffmpeg_next as ffmpeg;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum PluginError {
    #[error("Plugin not found: {0}")]
    NotFound(String),
    #[error("Plugin load failed: {0}")]
    LoadFailed(String),
    #[error("Plugin execution failed: {0}")]
    ExecutionFailed(String),
}

#[derive(Error, Debug)]
pub enum RenderError {
    #[error("Render queue closed")]
    QueueClosed,
    #[error("Failed to submit job to render queue")]
    SubmitFailed,
    #[error("Render worker thread panicked")]
    WorkerPanicked,
    #[error("Save worker thread panicked")]
    SaverPanicked,
    #[error("Rendering error: {0}")]
    Other(String),
}

#[derive(Error, Debug)]
pub enum ProjectError {
    #[error("Invalid composition index: {0}")]
    InvalidCompositionIndex(usize),
    #[error("Validation error: {0}")]
    ValidationFailed(String),
    #[error("Project error: {0}")]
    Other(String),
}

#[derive(Error, Debug)]
pub enum LibraryError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON parsing error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Libloading error: {0}")]
    Libloading(#[from] libloading::Error),
    #[error("{0}")]
    Plugin(#[from] PluginError),
    #[error("Image error: {0}")]
    Image(#[from] image::ImageError),
    #[error("FFmpeg error: {0}")]
    Ffmpeg(#[from] ffmpeg::Error),
    #[error("Other FFmpeg error: {0}")]
    FfmpegOther(String),
    #[error("{0}")]
    Project(#[from] ProjectError),
    #[error("{0}")]
    Render(#[from] RenderError),
    #[error("Invalid argument: {0}")]
    InvalidArgument(String),
    #[error("Runtime error: {0}")]
    Runtime(String),
}

/// Convenience constructors to minimize call-site changes
impl LibraryError {
    pub fn plugin(msg: impl Into<String>) -> Self {
        Self::Plugin(PluginError::ExecutionFailed(msg.into()))
    }

    pub fn render(msg: impl Into<String>) -> Self {
        Self::Render(RenderError::Other(msg.into()))
    }

    pub fn project(msg: impl Into<String>) -> Self {
        Self::Project(ProjectError::Other(msg.into()))
    }

    pub fn validation(msg: impl Into<String>) -> Self {
        Self::Project(ProjectError::ValidationFailed(msg.into()))
    }
}

impl From<Box<dyn std::error::Error>> for LibraryError {
    fn from(err: Box<dyn std::error::Error>) -> Self {
        LibraryError::Runtime(err.to_string())
    }
}
