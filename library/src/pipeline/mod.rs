//! Pull-based node graph evaluation engine.
//!
//! Each node type has a corresponding `NodeEvaluator` that computes output pin values
//! on demand. Evaluation starts from the composition's root track and recursively
//! pulls inputs from upstream nodes, caching results to avoid redundant computation.

pub mod context;
pub mod engine;
pub mod evaluator;
pub mod output;

// Pipeline stage submodules
pub mod compositing;
pub mod effects;
pub mod processing;
pub mod sources;

// Backward compatibility: old `evaluators` and `ensemble` paths re-export from new locations.
pub mod ensemble;
pub mod evaluators;
