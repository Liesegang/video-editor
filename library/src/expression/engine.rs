use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use lru::LruCache;
use rustpython_parser::Parse;
use rustpython_parser::ast;
use sha2::{Digest, Sha256};

use super::evaluator::EvaluationState;
use super::semantics::{convert_output, validate_context};
use super::validation::ValidationState;
use super::{
    ExpressionDiagnostic, ExpressionDiagnosticKind, ExpressionEvaluationContext,
    ExpressionOutputType, ExpressionSourceSpan, ExpressionValue,
};

const ENGINE_CONTRACT: &[u8] = b"ruvie-python-expression-subset-v1";
const DEFAULT_CACHE_CAPACITY: usize = 256;

/// Deterministic limits applied before and during evaluation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpressionLimits {
    pub max_source_bytes: usize,
    pub max_ast_nodes: u64,
    pub max_depth: u32,
    pub max_operations: u64,
    pub max_calls: u64,
    pub max_collection_items: usize,
    pub max_string_bytes: usize,
    pub max_exponent_abs: u32,
    /// Defense in depth. The operation/depth/size limits are the deterministic
    /// boundary; elapsed wall time can vary by host load.
    pub max_wall_time: Duration,
}

impl Default for ExpressionLimits {
    fn default() -> Self {
        Self {
            max_source_bytes: 16 * 1024,
            max_ast_nodes: 512,
            max_depth: 32,
            max_operations: 2_048,
            max_calls: 64,
            max_collection_items: 64,
            max_string_bytes: 4 * 1024,
            max_exponent_abs: 1_024,
            max_wall_time: Duration::from_millis(10),
        }
    }
}

/// Observable cache counters, primarily for performance regression tests.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExpressionCacheStats {
    pub hits: u64,
    pub compilations: u64,
}

#[derive(Clone)]
pub struct CompiledExpression {
    inner: Arc<CompiledExpressionInner>,
}

impl CompiledExpression {
    #[cfg(test)]
    pub fn source(&self) -> &str {
        &self.inner.source
    }

    #[cfg(test)]
    pub fn source_hash(&self) -> [u8; 32] {
        self.inner.source_hash
    }

    #[cfg(test)]
    pub fn ast_node_count(&self) -> u64 {
        self.inner.ast_node_count
    }
}

struct CompiledExpressionInner {
    #[cfg(test)]
    source: Arc<str>,
    #[cfg(test)]
    source_hash: [u8; 32],
    ast: ast::Expr,
    #[cfg(test)]
    ast_node_count: u64,
}

type CachedCompilation = Result<Arc<CompiledExpressionInner>, ExpressionDiagnostic>;

struct ExpressionEngineInner {
    limits: ExpressionLimits,
    cache: Mutex<LruCache<[u8; 32], CachedCompilation>>,
    cache_hits: AtomicU64,
    compilations: AtomicU64,
}

/// Thread-safe compiler and evaluator. Clones share the runtime-only AST cache.
#[derive(Clone)]
pub struct ExpressionEngine {
    inner: Arc<ExpressionEngineInner>,
}

impl Default for ExpressionEngine {
    fn default() -> Self {
        Self::new(ExpressionLimits::default(), DEFAULT_CACHE_CAPACITY)
    }
}

impl ExpressionEngine {
    pub fn new(limits: ExpressionLimits, cache_capacity: usize) -> Self {
        let capacity = NonZeroUsize::new(cache_capacity.max(1)).unwrap_or(NonZeroUsize::MIN);
        Self {
            inner: Arc::new(ExpressionEngineInner {
                limits,
                cache: Mutex::new(LruCache::new(capacity)),
                cache_hits: AtomicU64::new(0),
                compilations: AtomicU64::new(0),
            }),
        }
    }

    #[cfg(test)]
    pub fn limits(&self) -> &ExpressionLimits {
        &self.inner.limits
    }

    #[cfg(test)]
    pub fn cache_stats(&self) -> ExpressionCacheStats {
        ExpressionCacheStats {
            hits: self.inner.cache_hits.load(AtomicOrdering::Relaxed),
            compilations: self.inner.compilations.load(AtomicOrdering::Relaxed),
        }
    }

    /// Parses and validates a Python expression. Both successful ASTs and
    /// diagnostics are cached. The cache mutex is released before parsing.
    pub fn compile(&self, source: &str) -> Result<CompiledExpression, ExpressionDiagnostic> {
        if source.len() > self.inner.limits.max_source_bytes {
            return Err(ExpressionDiagnostic::compile(
                ExpressionDiagnosticKind::ResourceLimit,
                format!(
                    "source is {} bytes; maximum is {}",
                    source.len(),
                    self.inner.limits.max_source_bytes
                ),
                None,
            ));
        }

        let source_hash = expression_hash(source);
        let cached = {
            let mut cache = self.cache_guard();
            cache.get(&source_hash).cloned()
        };
        if let Some(cached) = cached {
            self.inner.cache_hits.fetch_add(1, AtomicOrdering::Relaxed);
            return cached.map(|inner| CompiledExpression { inner });
        }

        self.inner
            .compilations
            .fetch_add(1, AtomicOrdering::Relaxed);
        let compiled = self.compile_uncached(source, source_hash);
        {
            let mut cache = self.cache_guard();
            drop(cache.put(source_hash, compiled.clone()));
        }
        compiled.map(|inner| CompiledExpression { inner })
    }

    /// Compiles (or retrieves) source and evaluates it against a typed output
    /// contract. No cache lock is held while user-authored AST is evaluated.
    pub fn evaluate(
        &self,
        source: &str,
        context: &ExpressionEvaluationContext,
        output_type: ExpressionOutputType,
    ) -> Result<ExpressionValue, ExpressionDiagnostic> {
        let compiled = self.compile(source)?;
        self.evaluate_compiled(&compiled, context, output_type)
    }

    pub fn evaluate_compiled(
        &self,
        compiled: &CompiledExpression,
        context: &ExpressionEvaluationContext,
        output_type: ExpressionOutputType,
    ) -> Result<ExpressionValue, ExpressionDiagnostic> {
        validate_context(context, output_type, &self.inner.limits)?;
        let mut state = EvaluationState::new(context, &self.inner.limits);
        let result = state.evaluate(&compiled.inner.ast, 1)?;
        convert_output(result, output_type, &self.inner.limits)
    }

    fn compile_uncached(&self, source: &str, _source_hash: [u8; 32]) -> CachedCompilation {
        if source.trim().is_empty() {
            return Err(ExpressionDiagnostic::compile(
                ExpressionDiagnosticKind::Parse,
                "Expression source is empty",
                None,
            ));
        }
        let ast = ast::Expr::parse(source, "<expression>").map_err(|error| {
            let offset = u32::from(error.offset) as usize;
            ExpressionDiagnostic::compile(
                ExpressionDiagnosticKind::Parse,
                error.to_string(),
                Some(ExpressionSourceSpan {
                    start: offset,
                    end: offset,
                }),
            )
        })?;
        let mut validation = ValidationState {
            limits: &self.inner.limits,
            nodes: 0,
        };
        validation.validate(&ast, 1)?;
        Ok(Arc::new(CompiledExpressionInner {
            #[cfg(test)]
            source: Arc::from(source),
            #[cfg(test)]
            source_hash: _source_hash,
            ast,
            #[cfg(test)]
            ast_node_count: validation.nodes,
        }))
    }

    fn cache_guard(&self) -> MutexGuard<'_, LruCache<[u8; 32], CachedCompilation>> {
        self.inner
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

fn expression_hash(source: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(ENGINE_CONTRACT);
    hasher.update([0]);
    hasher.update(source.as_bytes());
    hasher.finalize().into()
}
