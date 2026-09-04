// Core internal modules
pub mod core;

/// Shared color transforms used by graph operations, pickers, and future
/// Preview/export boundaries.
pub mod color_management;

// Editor services - public API for GUI
pub mod editor;

// Restricted, deterministic Python expressions shared by properties and nodes.
pub mod expression;

// Data models (shared with GUI)
pub mod model;

// Plugin system
pub mod plugin;

// Utilities
pub mod util;

// Error types
pub mod error;
pub use error::LibraryError;

// Animation types remain part of the public authoring surface.
pub use model::animation;

// Core subsystems used by the application and plugins.
pub use core::audio;
pub use core::cache;
pub use core::ensemble;
pub use core::framing;
pub use core::model_resource;
pub use core::rendering;

pub use crate::model::frame::Image;
pub use crate::plugin::{ExportColorAuthority, ExportFrame, ExportSettings};

// Authoring Preview/export boundary used by the app.
pub use core::rendering::render_server::{
    AuthoringExportResult, RenderRequestId, RenderResult, RenderServer,
};
pub use core::rendering::skia_renderer::SkiaRenderer;
pub use editor::{RenderDestination, RenderService};

/// Explicitly starts the trusted in-process CPython host. GUI applications
/// call this once during startup before projects or plugins can execute code.
pub fn initialize_python_runtime() -> Result<(), LibraryError> {
    ruvie_python_runtime::initialize_global(ruvie_python_runtime::PythonHostConfig::default())
        .map(|_| ())
        .map_err(|error| LibraryError::Runtime(format!("Cannot initialize CPython: {error}")))
}
