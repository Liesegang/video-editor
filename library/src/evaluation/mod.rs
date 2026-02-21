//! Pull-based node graph evaluation engine.
//!
//! Each node type has a corresponding `NodeEvaluator` that computes output pin values
//! on demand. Evaluation starts from the composition's root track and recursively
//! pulls inputs from upstream nodes, caching results to avoid redundant computation.

pub mod context;
pub mod engine;
pub mod ensemble;
pub mod evaluator;
pub mod evaluators;
pub mod output;
pub mod text_decompose;
