// Internal modules
mod cli;
pub mod core;
pub mod editor;
pub mod error;
pub mod model;
pub mod plugin;
pub mod util;

// Error types
pub use error::{LibraryError, PluginError, ProjectError, RenderError};

// Animation types
pub use model::animation;

// Core sub-module aliases
pub use core::audio;
pub use core::cache;
pub use core::framing;
pub use core::rendering;

// Commonly used types
pub use crate::model::frame::Image;
pub use crate::plugin::ExportSettings;

// Service types
pub use core::rendering::render_server::{RenderResult, RenderServer};
pub use core::rendering::render_service::RenderService;
pub use core::rendering::skia_renderer::SkiaRenderer;
pub use editor::EditorService;
pub use editor::ExportService;
pub use editor::ProjectModel;

// CLI entry point
pub use cli::run;
