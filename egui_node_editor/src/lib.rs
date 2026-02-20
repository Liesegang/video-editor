//! Standalone egui-based node graph editor widget.
//!
//! This crate provides a reusable node editor UI with no domain-specific dependencies.
//! Users implement the [`NodeEditorDataSource`] and [`NodeEditorMutator`] traits to
//! connect their own data model.

pub mod drawing;
pub mod state;
pub mod theme;
pub mod traits;
pub mod types;
pub mod widget;

pub use state::NodeEditorState;
pub use theme::NodeEditorTheme;
pub use traits::{NodeEditorDataSource, NodeEditorMutator};
pub use types::*;
pub use widget::NodeEditorWidget;
