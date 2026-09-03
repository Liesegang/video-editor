//! Core internal modules - not directly exposed to GUI.

pub mod audio;
pub mod binding_runtime;
#[allow(
    dead_code,
    reason = "waveform cache APIs are activated with the Timeline waveform UI"
)]
pub mod cache;
pub mod data_source_runtime;
pub mod ensemble;
pub mod event_runtime;
pub mod framing;
pub mod generator_runtime;
pub mod render_plan;
pub mod subtitle_runtime;
// pub mod graph_compiler;
pub mod rendering;
pub mod timeline_runtime;
