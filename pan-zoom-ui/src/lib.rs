//! Reusable pan, zoom, canvas-theme, and grid primitives for egui surfaces.
//!
//! The crate intentionally has no knowledge of application-domain models or
//! surface-specific gesture owners. Adapters sample input, decide which
//! gesture owns it, then apply the returned [`NavigationDelta`] to their own
//! state.

#![forbid(unsafe_code)]

mod grid;
mod navigation;
mod theme;

pub use grid::{grid_lines, paint_canvas, GridAxis, GridConfig, GridLine, GridLineKind};
pub use navigation::{
    apply_navigation, navigation_delta, sanitize_state, AxisMask, CanvasState, InputPolicy,
    NavigationConfig, NavigationDelta, NavigationInput, ZoomPolicy,
};
pub use theme::{CanvasTheme, GridStroke};
