//! Editor services - public API for GUI interaction.
//!
//! This module contains all services that the GUI (app crate) should use
//! to interact with the library.

pub(crate) mod audio_service;
pub(crate) mod color_service;
mod editor_clip_ops;
mod editor_graph_ops;
mod editor_project_ops;
pub mod editor_service;
pub mod export_service;
pub mod handlers;
pub(crate) mod ocio_shim;
pub mod project_model;
pub(crate) mod project_service;

// Re-exports for convenient access
pub use audio_service::AudioService;
pub use color_service::ColorSpaceManager as ColorService;
pub use editor_service::EditorService;
pub use export_service::ExportService;
pub use project_model::ProjectModel;
