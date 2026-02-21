//! Unified command system: registry, shortcuts, handler, and history.

pub(crate) mod handler;
pub(crate) mod history;
mod registry;
pub(crate) mod shortcut;

pub(crate) use registry::*;
