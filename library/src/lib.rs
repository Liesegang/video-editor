// Internal modules
pub mod audio;
pub mod builtin;
mod cli;
pub mod error;
pub mod pipeline;

// Backward compatibility: `evaluation` re-exports from `pipeline`
pub mod evaluation {
    pub use crate::pipeline::*;
}
pub mod plugin;
pub mod project;
pub mod rendering;
pub mod runtime;
pub mod service;
pub(crate) mod timing;

// Backward compatibility: `editor` re-exports from `service`
pub mod editor {
    pub use crate::service::*;
}

// Error types
pub use error::{LibraryError, PluginError, ProjectError, RenderError};

// Animation types
pub use project::animation;

// Core sub-module aliases
pub use rendering::cache;

// Commonly used types
pub use crate::plugin::ExportSettings;
pub use crate::runtime::Image;

// Service types
pub use rendering::render_server::{RenderResult, RenderServer};
pub use rendering::skia_renderer::SkiaRenderer;
pub use service::EditorService;
pub use service::ExportService;
pub use service::ProjectModel;

// CLI entry point
pub use cli::run;
