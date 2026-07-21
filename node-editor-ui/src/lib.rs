//! Domain-neutral UI primitives for an egui node editor.
//!
//! This crate owns no graph model. Hosts provide their authoritative graph on
//! every frame and translate UI results back into host-domain commands. The
//! first extraction phase contains stateless wire geometry and selection
//! policies; descriptor-to-intent orchestration will be added only after its
//! host contract is exercised by both the video-editor adapter and a fake host.

#![forbid(unsafe_code)]

pub mod selection;
pub mod wire;
