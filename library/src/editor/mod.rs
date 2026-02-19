//! Editor services - public API for GUI interaction.
//!
//! This module contains all services that the GUI (app crate) should use
//! to interact with the library.

pub mod audio_service;
pub mod color_service;
pub mod editor_service;
pub mod export_service;
pub(crate) mod handlers;
pub mod ocio_shim;
pub mod project_model;
pub(crate) mod project_service;
pub mod render_service;

// Re-exports for convenient access
pub use audio_service::AudioService;
pub use color_service::ColorSpaceManager as ColorService;
pub use editor_service::EditorService;
pub use export_service::ExportService;
pub use project_model::ProjectModel;
pub use render_service::RenderService;
