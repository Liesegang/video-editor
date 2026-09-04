//! Authoritative Timeline editing model.
//!
//! This namespace is deliberately independent from the pre-v1 graph-backed
//! `Project`. Timeline placement is authoritative here; reusable Node topology
//! exists only inside a [`ModuleDefinition`].

mod attachment;
mod edit;
mod file_store;
mod ids;
mod module_graph;
mod project;
mod time;
mod timeline;
mod transition;
mod transition_instance;
mod transition_module;

pub use attachment::*;
pub use edit::*;
pub use file_store::*;
pub use ids::*;
pub use module_graph::*;
pub use project::*;
pub use time::*;
pub use timeline::*;
pub use transition::*;
pub use transition_instance::*;
pub use transition_module::*;

#[cfg(test)]
mod file_import_tests;
#[cfg(test)]
mod tests;
