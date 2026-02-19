//! Color management module (OpenColorIO integration).
//!
//! This module is placed in `core` so that both `plugin` and `editor`
//! can depend on it without creating circular dependencies.

mod color_service;
pub(crate) mod ocio_shim;

pub use color_service::{ColorSpaceManager, OcioProcessor};
