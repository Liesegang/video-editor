//! Trusted, in-process CPython host shared by RuViE expressions and future
//! automation/plugins.
//!
//! This crate deliberately embeds ordinary, standard-GIL CPython. Evaluated
//! code has Python builtins and imports and is therefore **not sandboxed**.

mod context;
mod diagnostic;
mod host;
mod runtime_home;
mod value;

pub use context::EvaluationContext;
pub use diagnostic::{Diagnostic, DiagnosticKind, Phase, SourceSpan};
pub use host::{
    CacheStats, CompiledCode, PythonHost, PythonHostConfig, global_host, initialize_global,
};
pub use value::{OutputType, PythonValue};
