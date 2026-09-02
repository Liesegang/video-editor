//! Editor services - public API for GUI interaction.
//!
//! This module contains all services that the GUI (app crate) should use
//! to interact with the library.

pub mod color_service;
pub mod handlers;
pub(crate) mod ocio_shim;
pub(crate) mod project_model;
pub mod project_service;
pub mod render_service;
pub mod timeline_editor_service;

pub use color_service::ColorSpaceManager as ColorService;
pub use render_service::{RenderDestination, RenderService};
pub use timeline_editor_service::TimelineEditorService;
