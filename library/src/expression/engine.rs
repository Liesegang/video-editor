use ruvie_python_runtime::{PythonHost, global_host};

use super::{
    ExpressionDiagnostic, ExpressionEvaluationContext, ExpressionOutputType, ExpressionValue,
};

/// Compatibility boundary around RuViE's process-global CPython host.
///
/// The persisted evaluator remains named `expression`, while compilation and
/// execution are entirely owned by CPython through `ruvie-python-runtime`.
#[derive(Clone)]
pub struct ExpressionEngine {
    host: Result<&'static PythonHost, ExpressionDiagnostic>,
}

impl Default for ExpressionEngine {
    fn default() -> Self {
        Self {
            host: global_host(),
        }
    }
}

impl ExpressionEngine {
    pub fn evaluate(
        &self,
        source: &str,
        context: &ExpressionEvaluationContext,
        output_type: ExpressionOutputType,
    ) -> Result<ExpressionValue, ExpressionDiagnostic> {
        self.host()?.evaluate(source, context, output_type)
    }

    fn host(&self) -> Result<&'static PythonHost, ExpressionDiagnostic> {
        self.host.as_ref().copied().map_err(Clone::clone)
    }
}
