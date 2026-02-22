//! Node evaluator trait — one implementation per node type category.

use super::context::EvalContext;
use super::output::PinValue;
use crate::error::LibraryError;
use uuid::Uuid;

/// Trait for evaluating a category of nodes.
///
/// Each implementation handles one or more `type_id` prefixes (e.g. `"effect."`,
/// `"style.fill"`, `"compositing.transform"`). The evaluation engine dispatches
/// to the appropriate evaluator based on the node's `type_id`.
pub trait NodeEvaluator: Send + Sync {
    /// The `type_id` prefixes this evaluator handles.
    ///
    /// A node with `type_id = "effect.blur"` would match an evaluator that
    /// returns `&["effect."]` from this method.
    fn handles(&self) -> &[&str];

    /// Evaluate the specified output pin of a node.
    ///
    /// The evaluator should use `ctx.pull_input()` to obtain upstream values
    /// and `ctx.resolve_property()` to read node properties (with graph
    /// override support).
    fn evaluate(
        &self,
        node_id: Uuid,
        pin_name: &str,
        ctx: &mut EvalContext,
    ) -> Result<PinValue, LibraryError>;
}
