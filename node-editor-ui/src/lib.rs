//! A domain-neutral, immediate-mode node-editor surface for egui.
//!
//! The host remains authoritative. It lends this crate a [`GraphFrame`] for
//! one UI frame and applies the returned [`EditorOutput`] intents to its own
//! model and history. The only retained state is an in-progress pointer
//! gesture in [`InteractionState`].

#![forbid(unsafe_code)]

mod chrome;
mod editor;
mod graph;
mod input;
mod interaction;
mod layout_swipe;
mod selection;
mod wire;

pub use chrome::{
    GroupChrome, HeaderGlyph, NodeHeader, NodePalette, NodeVisualStyle, PortLabel, PortVisualStyle,
};
pub use editor::{Editor, EditorConfig, NodeBodyRenderer, NodeBodyResponse};
pub use graph::{
    AuthoritativeSelection, GraphFrame, GroupDescriptor, ItemId, NodeDescriptor, PortDescriptor,
    PortDirection, PortInstanceId, PortOwner, TypeKey, WireDescriptor,
};
pub use interaction::{EditorOutput, InteractionOptions, InteractionState, MoveEndOutcome};
pub use layout_swipe::{LayoutSwipeAxis, LayoutSwipeHitArea, LayoutSwipeIntent, LayoutSwipePhase};
pub use selection::{after_click, after_marquee, topmost_hit};
pub use wire::{CubicBezier, HitRegion};
