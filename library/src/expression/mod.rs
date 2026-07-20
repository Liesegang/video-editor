//! Deterministic Python Expression subset for properties and value nodes.
//!
//! Source is parsed with RustPython's Python 3 parser. RuViE evaluates only an
//! explicit, side-effect-free AST whitelist; it does not expose imports,
//! Python builtins, files, networking, processes, or host objects.

mod builtins;
mod diagnostic;
mod engine;
mod evaluator;
mod semantics;
mod validation;
mod value;

pub use diagnostic::{
    ExpressionDiagnostic, ExpressionDiagnosticKind, ExpressionPhase, ExpressionSourceSpan,
};
pub use engine::{CompiledExpression, ExpressionCacheStats, ExpressionEngine, ExpressionLimits};
pub use value::{ExpressionEvaluationContext, ExpressionOutputType, ExpressionValue};
