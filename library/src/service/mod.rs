//! Service layer - public API for GUI interaction.
//!
//! This module contains all services that the GUI (app crate) should use
//! to interact with the library.

pub(crate) mod audio_service;
pub mod bounds;
pub mod editor_service;
pub mod export_service;
pub mod handlers;
mod ops;
pub mod project_model;
pub(crate) mod project_service;

// Re-exports for convenient access
pub use crate::rendering::color::ColorSpaceManager as ColorService;
pub use audio_service::AudioService;
pub use editor_service::EditorService;
pub use export_service::ExportService;
pub use project_model::ProjectModel;
