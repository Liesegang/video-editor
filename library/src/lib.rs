// Internal modules
pub mod audio;
pub mod builtin;
mod cli;
pub mod error;
pub mod nodes;
pub mod pipeline;
pub mod plugin;
pub mod project;
pub mod rendering;
pub mod runtime;
pub mod service;
pub(crate) mod timing;

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
