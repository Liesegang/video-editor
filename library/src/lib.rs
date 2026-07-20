// Core internal modules
pub mod core;

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

// Re-export animation types from model for backward compatibility
pub use model::animation;

// Re-exports for backward compatibility
pub use core::audio;
pub use core::cache;
pub use core::ensemble;
pub use core::framing;
pub use core::rendering;

pub use crate::model::frame::Image;
pub use crate::plugin::ExportSettings;

// Re-export the services that the app will need
pub use core::rendering::render_server::{RenderResult, RenderServer};
pub use core::rendering::skia_renderer::SkiaRenderer;
pub use editor::ExportService;
pub use editor::ProjectModel;
pub use editor::ProjectService;
pub use editor::RenderService;
pub use editor::{ClipBundle, EditorService, KeyframeBatchUpdate, NodeGraphBundle, PropertyOwner};

pub fn run(_: Vec<String>) -> Result<(), LibraryError> {
    println!("Library CLI temporarily disabled during Trinity refactor.");
    Ok(())
}
