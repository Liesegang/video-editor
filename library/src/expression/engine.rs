use std::cmp::Ordering;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use lru::LruCache;
use rustpython_parser::Parse;
use rustpython_parser::ast::{self, BoolOp, CmpOp, Constant, Operator, Ranged, UnaryOp};
use sha2::{Digest, Sha256};

use super::{
    ExpressionDiagnostic, ExpressionDiagnosticKind, ExpressionEvaluationContext,
    ExpressionOutputType, ExpressionSourceSpan, ExpressionValue,
};

const ENGINE_CONTRACT: &[u8] = b"ruvie-python-expression-subset-v1";
const DEFAULT_CACHE_CAPACITY: usize = 256;
const MAX_EXACT_F64_INTEGER: i64 = 1_i64 << 53;

/// Deterministic limits applied before and during evaluation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExpressionLimits {
    pub max_source_bytes: usize,
    pub max_ast_nodes: u64,
    pub max_depth: u32,
    pub max_operations: u64,
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
            max_collection_items: 64,
            max_string_bytes: 4 * 1024,
            max_exponent_abs: 1_024,
            max_wall_time: Duration::from_millis(10),
        }
    }
}

/// Observable cache counters, primarily for performance regression tests.
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
    pub fn source(&self) -> &str {
        &self.inner.source
    }

    pub fn source_hash(&self) -> [u8; 32] {
        self.inner.source_hash
    }

    pub fn ast_node_count(&self) -> u64 {
        self.inner.ast_node_count
    }
}

struct CompiledExpressionInner {
    source: Arc<str>,
    source_hash: [u8; 32],
    ast: ast::Expr,
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

    pub fn limits(&self) -> &ExpressionLimits {
        &self.inner.limits
    }

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

    fn compile_uncached(&self, source: &str, source_hash: [u8; 32]) -> CachedCompilation {
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
            source: Arc::from(source),
            source_hash,
            ast,
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

struct ValidationState<'a> {
    limits: &'a ExpressionLimits,
    nodes: u64,
}

impl ValidationState<'_> {
    fn validate(&mut self, expression: &ast::Expr, depth: u32) -> Result<(), ExpressionDiagnostic> {
        self.nodes = self.nodes.saturating_add(1);
        if self.nodes > self.limits.max_ast_nodes {
            return Err(compile_limit(
                format!("AST exceeds {} nodes", self.limits.max_ast_nodes),
                expression,
            ));
        }
        if depth > self.limits.max_depth {
            return Err(compile_limit(
                format!("AST exceeds depth {}", self.limits.max_depth),
                expression,
            ));
        }
        let child_depth = depth.saturating_add(1);
        match expression {
            ast::Expr::Constant(node) => self.validate_constant(&node.value, expression),
            ast::Expr::Name(node) => {
                if is_context_name(node.id.as_str()) || is_constant_name(node.id.as_str()) {
                    Ok(())
                } else {
                    Err(ExpressionDiagnostic::compile(
                        ExpressionDiagnosticKind::UnknownName,
                        format!("name '{}' is not available", node.id),
                        Some(expression_span(expression)),
                    ))
                }
            }
            ast::Expr::BinOp(node) => {
                if !matches!(
                    node.op,
                    Operator::Add
                        | Operator::Sub
                        | Operator::Mult
                        | Operator::Div
                        | Operator::FloorDiv
                        | Operator::Mod
                        | Operator::Pow
                ) {
                    return Err(unsupported("binary operator", expression));
                }
                self.validate(&node.left, child_depth)?;
                self.validate(&node.right, child_depth)
            }
            ast::Expr::UnaryOp(node) => {
                if !matches!(node.op, UnaryOp::Not | UnaryOp::UAdd | UnaryOp::USub) {
                    return Err(unsupported("unary operator", expression));
                }
                self.validate(&node.operand, child_depth)
            }
            ast::Expr::BoolOp(node) => {
                for value in &node.values {
                    self.validate(value, child_depth)?;
                }
                Ok(())
            }
            ast::Expr::IfExp(node) => {
                self.validate(&node.test, child_depth)?;
                self.validate(&node.body, child_depth)?;
                self.validate(&node.orelse, child_depth)
            }
            ast::Expr::Compare(node) => {
                if node.ops.iter().any(|operator| {
                    !matches!(
                        operator,
                        CmpOp::Eq | CmpOp::NotEq | CmpOp::Lt | CmpOp::LtE | CmpOp::Gt | CmpOp::GtE
                    )
                }) {
                    return Err(unsupported("comparison operator", expression));
                }
                self.validate(&node.left, child_depth)?;
                for comparator in &node.comparators {
                    self.validate(comparator, child_depth)?;
                }
                Ok(())
            }
            ast::Expr::Call(node) => {
                let function = resolve_function(&node.func).ok_or_else(|| {
                    ExpressionDiagnostic::compile(
                        ExpressionDiagnosticKind::UnknownName,
                        "only documented Expression helpers may be called",
                        Some(expression_span(&node.func)),
                    )
                })?;
                if !node.keywords.is_empty() {
                    return Err(ExpressionDiagnostic::compile(
                        ExpressionDiagnosticKind::InvalidArguments,
                        "keyword and unpacked arguments are not supported",
                        Some(expression_span(expression)),
                    ));
                }
                if node.args.len() > self.limits.max_collection_items {
                    return Err(compile_limit(
                        format!(
                            "call has {} arguments; maximum is {}",
                            node.args.len(),
                            self.limits.max_collection_items
                        ),
                        expression,
                    ));
                }
                validate_arity(function, node.args.len(), expression)?;
                for argument in &node.args {
                    self.validate(argument, child_depth)?;
                }
                Ok(())
            }
            ast::Expr::Attribute(node) => {
                if is_math_constant(&node.value, node.attr.as_str()) {
                    Ok(())
                } else if is_component_name(node.attr.as_str()) {
                    self.validate(&node.value, child_depth)
                } else {
                    Err(unsupported("attribute access", expression))
                }
            }
            ast::Expr::Subscript(node) => {
                if matches!(&*node.slice, ast::Expr::Slice(_)) {
                    return Err(unsupported("slice", expression));
                }
                self.validate(&node.value, child_depth)?;
                self.validate(&node.slice, child_depth)
            }
            ast::Expr::List(node) => self.validate_collection(&node.elts, child_depth, expression),
            ast::Expr::Tuple(node) => self.validate_collection(&node.elts, child_depth, expression),
            ast::Expr::NamedExpr(_)
            | ast::Expr::Lambda(_)
            | ast::Expr::Dict(_)
            | ast::Expr::Set(_)
            | ast::Expr::ListComp(_)
            | ast::Expr::SetComp(_)
            | ast::Expr::DictComp(_)
            | ast::Expr::GeneratorExp(_)
            | ast::Expr::Await(_)
            | ast::Expr::Yield(_)
            | ast::Expr::YieldFrom(_)
            | ast::Expr::FormattedValue(_)
            | ast::Expr::JoinedStr(_)
            | ast::Expr::Starred(_)
            | ast::Expr::Slice(_) => Err(unsupported(ast_name(expression), expression)),
        }
    }

    fn validate_constant(
        &self,
        constant: &Constant,
        expression: &ast::Expr,
    ) -> Result<(), ExpressionDiagnostic> {
        match constant {
            Constant::Bool(_) | Constant::Int(_) => Ok(()),
            Constant::Float(value) if value.is_finite() => Ok(()),
            Constant::Float(_) => Err(ExpressionDiagnostic::compile(
                ExpressionDiagnosticKind::NonFinite,
                "non-finite numeric literals are not supported",
                Some(expression_span(expression)),
            )),
            Constant::Str(value) if value.len() <= self.limits.max_string_bytes => Ok(()),
            Constant::Str(value) => Err(compile_limit(
                format!(
                    "string literal is {} bytes; maximum is {}",
                    value.len(),
                    self.limits.max_string_bytes
                ),
                expression,
            )),
            Constant::None
            | Constant::Bytes(_)
            | Constant::Tuple(_)
            | Constant::Complex { .. }
            | Constant::Ellipsis => Err(unsupported("literal", expression)),
        }
    }

    fn validate_collection(
        &mut self,
        values: &[ast::Expr],
        depth: u32,
        expression: &ast::Expr,
    ) -> Result<(), ExpressionDiagnostic> {
        if values.len() > self.limits.max_collection_items {
            return Err(compile_limit(
                format!(
                    "collection has {} items; maximum is {}",
                    values.len(),
                    self.limits.max_collection_items
                ),
                expression,
            ));
        }
        for value in values {
            self.validate(value, depth)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SequenceKind {
    List,
    Tuple,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VectorKind {
    Vec2,
    Vec3,
    Vec4,
}

#[derive(Clone, Debug, PartialEq)]
enum RuntimeValue {
    Integer(i64),
    Number(f64),
    Bool(bool),
    String(String),
    Sequence(SequenceKind, Vec<RuntimeValue>),
    Vector(VectorKind, Vec<f64>),
    Color([f64; 4]),
}

struct EvaluationState<'a> {
    context: &'a ExpressionEvaluationContext,
    limits: &'a ExpressionLimits,
    started: Instant,
    operations: u64,
}

impl<'a> EvaluationState<'a> {
    fn new(context: &'a ExpressionEvaluationContext, limits: &'a ExpressionLimits) -> Self {
        Self {
            context,
            limits,
            started: Instant::now(),
            operations: 0,
        }
    }

    fn evaluate(
        &mut self,
        expression: &ast::Expr,
        depth: u32,
    ) -> Result<RuntimeValue, ExpressionDiagnostic> {
        self.consume(1, Some(expression_span(expression)))?;
        if depth > self.limits.max_depth {
            return Err(self.error(
                ExpressionDiagnosticKind::ResourceLimit,
                format!("evaluation exceeds depth {}", self.limits.max_depth),
                expression,
            ));
        }
        let child_depth = depth.saturating_add(1);
        match expression {
            ast::Expr::Constant(node) => self.evaluate_constant(&node.value, expression),
            ast::Expr::Name(node) => self.evaluate_name(node.id.as_str(), expression),
            ast::Expr::BinOp(node) => {
                let left = self.evaluate(&node.left, child_depth)?;
                let right = self.evaluate(&node.right, child_depth)?;
                self.evaluate_binary(node.op, left, right, expression)
            }
            ast::Expr::UnaryOp(node) => {
                let value = self.evaluate(&node.operand, child_depth)?;
                self.evaluate_unary(node.op, value, expression)
            }
            ast::Expr::BoolOp(node) => {
                self.evaluate_bool(node.op, &node.values, child_depth, expression)
            }
            ast::Expr::IfExp(node) => {
                let condition = self.evaluate(&node.test, child_depth)?;
                if truthy(&condition) {
                    self.evaluate(&node.body, child_depth)
                } else {
                    self.evaluate(&node.orelse, child_depth)
                }
            }
            ast::Expr::Compare(node) => self.evaluate_compare(
                &node.left,
                &node.ops,
                &node.comparators,
                child_depth,
                expression,
            ),
            ast::Expr::Call(node) => {
                let Some(function) = resolve_function(&node.func) else {
                    return Err(self.error(
                        ExpressionDiagnosticKind::UnknownName,
                        "call target is not available",
                        expression,
                    ));
                };
                let mut arguments = Vec::with_capacity(node.args.len());
                for argument in &node.args {
                    arguments.push(self.evaluate(argument, child_depth)?);
                }
                self.call(function, &arguments, expression)
            }
            ast::Expr::Attribute(node) => {
                if is_math_constant(&node.value, node.attr.as_str()) {
                    return Ok(RuntimeValue::Number(math_constant(node.attr.as_str())));
                }
                let value = self.evaluate(&node.value, child_depth)?;
                self.component(value, node.attr.as_str(), expression)
            }
            ast::Expr::Subscript(node) => {
                let value = self.evaluate(&node.value, child_depth)?;
                let index = self.evaluate(&node.slice, child_depth)?;
                self.index(value, index, expression)
            }
            ast::Expr::List(node) => {
                self.evaluate_sequence(SequenceKind::List, &node.elts, child_depth)
            }
            ast::Expr::Tuple(node) => {
                self.evaluate_sequence(SequenceKind::Tuple, &node.elts, child_depth)
            }
            _ => Err(self.error(
                ExpressionDiagnosticKind::UnsupportedSyntax,
                format!("{} is not supported", ast_name(expression)),
                expression,
            )),
        }
    }

    fn consume(
        &mut self,
        count: u64,
        span: Option<ExpressionSourceSpan>,
    ) -> Result<(), ExpressionDiagnostic> {
        self.operations = self.operations.saturating_add(count);
        if self.operations > self.limits.max_operations {
            return Err(ExpressionDiagnostic::evaluate(
                ExpressionDiagnosticKind::ResourceLimit,
                format!(
                    "evaluation exceeds {} operations",
                    self.limits.max_operations
                ),
                span,
            ));
        }
        if self.started.elapsed() > self.limits.max_wall_time {
            return Err(ExpressionDiagnostic::evaluate(
                ExpressionDiagnosticKind::ResourceLimit,
                format!(
                    "evaluation exceeded the {:?} wall-time limit",
                    self.limits.max_wall_time
                ),
                span,
            ));
        }
        Ok(())
    }

    fn evaluate_constant(
        &self,
        constant: &Constant,
        expression: &ast::Expr,
    ) -> Result<RuntimeValue, ExpressionDiagnostic> {
        match constant {
            Constant::Bool(value) => Ok(RuntimeValue::Bool(*value)),
            Constant::Int(value) => value
                .to_string()
                .parse::<i64>()
                .map(RuntimeValue::Integer)
                .map_err(|_| {
                    self.error(
                        ExpressionDiagnosticKind::ResourceLimit,
                        "integer literal exceeds the supported 64-bit range",
                        expression,
                    )
                }),
            Constant::Float(value) if value.is_finite() => Ok(RuntimeValue::Number(*value)),
            Constant::Str(value) => Ok(RuntimeValue::String(value.clone())),
            _ => Err(self.error(
                ExpressionDiagnosticKind::UnsupportedSyntax,
                "literal is not supported",
                expression,
            )),
        }
    }

    fn evaluate_name(
        &self,
        name: &str,
        expression: &ast::Expr,
    ) -> Result<RuntimeValue, ExpressionDiagnostic> {
        match name {
            "time" => Ok(RuntimeValue::Number(self.context.time())),
            "fps" => Ok(RuntimeValue::Number(self.context.fps())),
            "frame" => Ok(RuntimeValue::Number(self.context.frame())),
            "frame_index" => float_to_integer(self.context.frame().floor()).map_err(|message| {
                self.error(ExpressionDiagnosticKind::ResourceLimit, message, expression)
            }),
            "width" => u64_to_integer(self.context.width()).map_err(|message| {
                self.error(ExpressionDiagnosticKind::ResourceLimit, message, expression)
            }),
            "height" => u64_to_integer(self.context.height()).map_err(|message| {
                self.error(ExpressionDiagnosticKind::ResourceLimit, message, expression)
            }),
            "resolution" => Ok(RuntimeValue::Vector(
                VectorKind::Vec2,
                vec![self.context.width() as f64, self.context.height() as f64],
            )),
            "value" => self
                .context
                .value()
                .map(runtime_from_public)
                .transpose()?
                .ok_or_else(|| {
                    self.error(
                        ExpressionDiagnosticKind::InvalidContext,
                        "value is unavailable for this Expression",
                        expression,
                    )
                }),
            "pi" => Ok(RuntimeValue::Number(std::f64::consts::PI)),
            "tau" => Ok(RuntimeValue::Number(std::f64::consts::TAU)),
            "e" => Ok(RuntimeValue::Number(std::f64::consts::E)),
            _ => Err(self.error(
                ExpressionDiagnosticKind::UnknownName,
                format!("name '{name}' is not available"),
                expression,
            )),
        }
    }

    fn evaluate_binary(
        &self,
        operator: Operator,
        left: RuntimeValue,
        right: RuntimeValue,
        expression: &ast::Expr,
    ) -> Result<RuntimeValue, ExpressionDiagnostic> {
        let result = match operator {
            Operator::Add => add(left, right, self.limits),
            Operator::Sub => subtract(left, right),
            Operator::Mult => multiply(left, right, self.limits),
            Operator::Div => divide(left, right),
            Operator::FloorDiv => floor_divide(left, right),
            Operator::Mod => modulo(left, right),
            Operator::Pow => power(left, right, self.limits),
            _ => Err(RuntimeFailure::unsupported("binary operator")),
        };
        result.map_err(|failure| self.failure(failure, expression))
    }

    fn evaluate_unary(
        &self,
        operator: UnaryOp,
        value: RuntimeValue,
        expression: &ast::Expr,
    ) -> Result<RuntimeValue, ExpressionDiagnostic> {
        let result = match operator {
            UnaryOp::Not => Ok(RuntimeValue::Bool(!truthy(&value))),
            UnaryOp::UAdd => unary_plus(value),
            UnaryOp::USub => negate(value),
            UnaryOp::Invert => Err(RuntimeFailure::unsupported("bitwise inversion")),
        };
        result.map_err(|failure| self.failure(failure, expression))
    }

    fn evaluate_bool(
        &mut self,
        operator: BoolOp,
        values: &[ast::Expr],
        depth: u32,
        expression: &ast::Expr,
    ) -> Result<RuntimeValue, ExpressionDiagnostic> {
        let Some((first, rest)) = values.split_first() else {
            return Err(self.error(
                ExpressionDiagnosticKind::Runtime,
                "boolean expression has no operands",
                expression,
            ));
        };
        let mut result = self.evaluate(first, depth)?;
        for value in rest {
            let short_circuits = match operator {
                BoolOp::And => !truthy(&result),
                BoolOp::Or => truthy(&result),
            };
            if short_circuits {
                return Ok(result);
            }
            result = self.evaluate(value, depth)?;
        }
        Ok(result)
    }

    fn evaluate_compare(
        &mut self,
        left_expression: &ast::Expr,
        operators: &[CmpOp],
        comparators: &[ast::Expr],
        depth: u32,
        expression: &ast::Expr,
    ) -> Result<RuntimeValue, ExpressionDiagnostic> {
        let mut left = self.evaluate(left_expression, depth)?;
        for (operator, comparator) in operators.iter().zip(comparators) {
            let right = self.evaluate(comparator, depth)?;
            let matches = compare(*operator, &left, &right)
                .map_err(|failure| self.failure(failure, expression))?;
            if !matches {
                return Ok(RuntimeValue::Bool(false));
            }
            left = right;
        }
        Ok(RuntimeValue::Bool(true))
    }

    fn evaluate_sequence(
        &mut self,
        kind: SequenceKind,
        expressions: &[ast::Expr],
        depth: u32,
    ) -> Result<RuntimeValue, ExpressionDiagnostic> {
        let mut values = Vec::with_capacity(expressions.len());
        for expression in expressions {
            values.push(self.evaluate(expression, depth)?);
        }
        Ok(RuntimeValue::Sequence(kind, values))
    }

    fn component(
        &self,
        value: RuntimeValue,
        component: &str,
        expression: &ast::Expr,
    ) -> Result<RuntimeValue, ExpressionDiagnostic> {
        let index = match component {
            "x" | "r" => 0,
            "y" | "g" => 1,
            "z" | "b" => 2,
            "w" | "a" => 3,
            _ => {
                return Err(self.error(
                    ExpressionDiagnosticKind::UnsupportedSyntax,
                    format!("component '.{component}' is not supported"),
                    expression,
                ));
            }
        };
        let component = match value {
            RuntimeValue::Vector(_, values) => values.get(index).copied(),
            RuntimeValue::Color(values) => values.get(index).copied(),
            _ => None,
        };
        component.map(RuntimeValue::Number).ok_or_else(|| {
            self.error(
                ExpressionDiagnosticKind::TypeMismatch,
                "component is unavailable on this value",
                expression,
            )
        })
    }

    fn index(
        &mut self,
        value: RuntimeValue,
        index: RuntimeValue,
        expression: &ast::Expr,
    ) -> Result<RuntimeValue, ExpressionDiagnostic> {
        let index = integer(&index).map_err(|_| {
            self.error(
                ExpressionDiagnosticKind::TypeMismatch,
                "index must be an integer",
                expression,
            )
        })?;
        match value {
            RuntimeValue::Sequence(_, values) => {
                let Some(index) = python_index(index, values.len()) else {
                    return Err(self.error(
                        ExpressionDiagnosticKind::Runtime,
                        "sequence index is out of range",
                        expression,
                    ));
                };
                Ok(values[index].clone())
            }
            RuntimeValue::Vector(_, values) => {
                let Some(index) = python_index(index, values.len()) else {
                    return Err(self.error(
                        ExpressionDiagnosticKind::Runtime,
                        "vector index is out of range",
                        expression,
                    ));
                };
                Ok(RuntimeValue::Number(values[index]))
            }
            RuntimeValue::Color(values) => {
                let Some(index) = python_index(index, values.len()) else {
                    return Err(self.error(
                        ExpressionDiagnosticKind::Runtime,
                        "color index is out of range",
                        expression,
                    ));
                };
                Ok(RuntimeValue::Number(values[index]))
            }
            RuntimeValue::String(value) => {
                let character_count = value.chars().count();
                self.consume(character_count as u64, Some(expression_span(expression)))?;
                let Some(index) = python_index(index, character_count) else {
                    return Err(self.error(
                        ExpressionDiagnosticKind::Runtime,
                        "string index is out of range",
                        expression,
                    ));
                };
                let Some(character) = value.chars().nth(index) else {
                    return Err(self.error(
                        ExpressionDiagnosticKind::Runtime,
                        "string index is out of range",
                        expression,
                    ));
                };
                Ok(RuntimeValue::String(character.to_string()))
            }
            _ => Err(self.error(
                ExpressionDiagnosticKind::TypeMismatch,
                "value does not support indexing",
                expression,
            )),
        }
    }

    fn call(
        &mut self,
        function: Function,
        arguments: &[RuntimeValue],
        expression: &ast::Expr,
    ) -> Result<RuntimeValue, ExpressionDiagnostic> {
        self.consume(arguments.len() as u64, Some(expression_span(expression)))?;
        call_function(function, arguments, self.limits)
            .map_err(|failure| self.failure(failure, expression))
    }

    fn error(
        &self,
        kind: ExpressionDiagnosticKind,
        message: impl Into<String>,
        expression: &ast::Expr,
    ) -> ExpressionDiagnostic {
        ExpressionDiagnostic::evaluate(kind, message, Some(expression_span(expression)))
    }

    fn failure(&self, failure: RuntimeFailure, expression: &ast::Expr) -> ExpressionDiagnostic {
        self.error(failure.kind, failure.message, expression)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Function {
    Sin,
    Cos,
    Tan,
    Atan2,
    Floor,
    Ceil,
    Fmod,
    Round,
    Abs,
    Min,
    Max,
    Clamp,
    Lerp,
    Smoothstep,
    Vec2,
    Vec3,
    Vec4,
    Rgb,
    Rgba,
    Random,
    Noise,
    Length,
    Normalize,
    Dot,
}

fn resolve_function(expression: &ast::Expr) -> Option<Function> {
    match expression {
        ast::Expr::Name(node) => match node.id.as_str() {
            "round" => Some(Function::Round),
            "abs" => Some(Function::Abs),
            "min" => Some(Function::Min),
            "max" => Some(Function::Max),
            "clamp" => Some(Function::Clamp),
            "lerp" => Some(Function::Lerp),
            "smoothstep" => Some(Function::Smoothstep),
            "vec2" => Some(Function::Vec2),
            "vec3" => Some(Function::Vec3),
            "vec4" => Some(Function::Vec4),
            "rgb" => Some(Function::Rgb),
            "rgba" => Some(Function::Rgba),
            "random" => Some(Function::Random),
            "noise" => Some(Function::Noise),
            "length" => Some(Function::Length),
            "normalize" => Some(Function::Normalize),
            "dot" => Some(Function::Dot),
            _ => None,
        },
        ast::Expr::Attribute(node) if is_math_name(&node.value) => match node.attr.as_str() {
            "sin" => Some(Function::Sin),
            "cos" => Some(Function::Cos),
            "tan" => Some(Function::Tan),
            "atan2" => Some(Function::Atan2),
            "floor" => Some(Function::Floor),
            "ceil" => Some(Function::Ceil),
            "fmod" => Some(Function::Fmod),
            _ => None,
        },
        _ => None,
    }
}

fn validate_arity(
    function: Function,
    actual: usize,
    expression: &ast::Expr,
) -> Result<(), ExpressionDiagnostic> {
    let (minimum, maximum) = match function {
        Function::Sin
        | Function::Cos
        | Function::Tan
        | Function::Floor
        | Function::Ceil
        | Function::Abs
        | Function::Random
        | Function::Length
        | Function::Normalize => (1, 1),
        Function::Atan2 | Function::Fmod | Function::Noise | Function::Dot => (2, 2),
        Function::Round => (1, 2),
        Function::Min | Function::Max => (1, usize::MAX),
        Function::Clamp
        | Function::Lerp
        | Function::Smoothstep
        | Function::Vec3
        | Function::Rgb => (3, 3),
        Function::Vec2 => (2, 2),
        Function::Vec4 | Function::Rgba => (4, 4),
    };
    if (minimum..=maximum).contains(&actual) {
        Ok(())
    } else {
        let expected = if minimum == maximum {
            minimum.to_string()
        } else {
            format!("{minimum}..={maximum}")
        };
        Err(ExpressionDiagnostic::compile(
            ExpressionDiagnosticKind::InvalidArguments,
            format!("helper expects {expected} arguments, got {actual}"),
            Some(expression_span(expression)),
        ))
    }
}

#[derive(Debug)]
struct RuntimeFailure {
    kind: ExpressionDiagnosticKind,
    message: String,
}

impl RuntimeFailure {
    fn new(kind: ExpressionDiagnosticKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    fn type_error(message: impl Into<String>) -> Self {
        Self::new(ExpressionDiagnosticKind::TypeMismatch, message)
    }

    fn non_finite(message: impl Into<String>) -> Self {
        Self::new(ExpressionDiagnosticKind::NonFinite, message)
    }

    fn limit(message: impl Into<String>) -> Self {
        Self::new(ExpressionDiagnosticKind::ResourceLimit, message)
    }

    fn unsupported(message: impl Into<String>) -> Self {
        Self::new(ExpressionDiagnosticKind::UnsupportedSyntax, message)
    }
}

fn call_function(
    function: Function,
    arguments: &[RuntimeValue],
    limits: &ExpressionLimits,
) -> Result<RuntimeValue, RuntimeFailure> {
    match function {
        Function::Sin => unary_math(arguments, f64::sin),
        Function::Cos => unary_math(arguments, f64::cos),
        Function::Tan => unary_math(arguments, f64::tan),
        Function::Atan2 => binary_math(arguments, f64::atan2),
        Function::Floor => integer_math(arguments, f64::floor),
        Function::Ceil => integer_math(arguments, f64::ceil),
        Function::Fmod => {
            let (left, right) = two_numbers(arguments)?;
            if right == 0.0 {
                return Err(RuntimeFailure::new(
                    ExpressionDiagnosticKind::DivisionByZero,
                    "math.fmod division by zero",
                ));
            }
            finite_number(left % right)
        }
        Function::Round => round_value(arguments),
        Function::Abs => absolute(arguments),
        Function::Min => minimum_or_maximum(arguments, Ordering::Less),
        Function::Max => minimum_or_maximum(arguments, Ordering::Greater),
        Function::Clamp => clamp_value(arguments),
        Function::Lerp => lerp_value(arguments),
        Function::Smoothstep => smoothstep(arguments),
        Function::Vec2 => make_vector(VectorKind::Vec2, arguments),
        Function::Vec3 => make_vector(VectorKind::Vec3, arguments),
        Function::Vec4 => make_vector(VectorKind::Vec4, arguments),
        Function::Rgb => make_color(arguments, false),
        Function::Rgba => make_color(arguments, true),
        Function::Random => deterministic_random(arguments),
        Function::Noise => deterministic_noise(arguments),
        Function::Length => vector_length(arguments),
        Function::Normalize => normalize_vector(arguments),
        Function::Dot => dot_product(arguments),
    }
    .and_then(|value| validate_runtime_value(value, limits))
}

fn unary_math(
    arguments: &[RuntimeValue],
    operation: impl FnOnce(f64) -> f64,
) -> Result<RuntimeValue, RuntimeFailure> {
    finite_number(operation(number(&arguments[0])?))
}

fn binary_math(
    arguments: &[RuntimeValue],
    operation: impl FnOnce(f64, f64) -> f64,
) -> Result<RuntimeValue, RuntimeFailure> {
    let (left, right) = two_numbers(arguments)?;
    finite_number(operation(left, right))
}

fn integer_math(
    arguments: &[RuntimeValue],
    operation: impl FnOnce(f64) -> f64,
) -> Result<RuntimeValue, RuntimeFailure> {
    float_to_integer_runtime(operation(number(&arguments[0])?))
}

fn round_value(arguments: &[RuntimeValue]) -> Result<RuntimeValue, RuntimeFailure> {
    let value = number(&arguments[0])?;
    if arguments.len() == 1 {
        return float_to_integer_runtime(value.round_ties_even());
    }
    let digits = integer(&arguments[1])?;
    let digits = i32::try_from(digits)
        .map_err(|_| RuntimeFailure::limit("round digits exceed the supported range"))?;
    if !(-308..=308).contains(&digits) {
        return Err(RuntimeFailure::limit(
            "round digits must be between -308 and 308",
        ));
    }
    let factor = 10_f64.powi(digits.abs());
    let rounded = if digits >= 0 {
        let scaled = value * factor;
        if scaled.is_finite() {
            scaled.round_ties_even() / factor
        } else {
            value
        }
    } else {
        (value / factor).round_ties_even() * factor
    };
    finite_number(rounded)
}

fn absolute(arguments: &[RuntimeValue]) -> Result<RuntimeValue, RuntimeFailure> {
    match &arguments[0] {
        RuntimeValue::Integer(value) => value
            .checked_abs()
            .map(RuntimeValue::Integer)
            .ok_or_else(|| RuntimeFailure::limit("integer absolute value overflowed")),
        RuntimeValue::Number(value) => finite_number(value.abs()),
        RuntimeValue::Bool(value) => Ok(RuntimeValue::Integer(i64::from(*value))),
        _ => Err(RuntimeFailure::type_error("abs expects a number")),
    }
}

fn minimum_or_maximum(
    arguments: &[RuntimeValue],
    wanted: Ordering,
) -> Result<RuntimeValue, RuntimeFailure> {
    let values = if arguments.len() == 1 {
        match &arguments[0] {
            RuntimeValue::Sequence(_, values) if !values.is_empty() => values.as_slice(),
            RuntimeValue::Sequence(_, _) => {
                return Err(RuntimeFailure::new(
                    ExpressionDiagnosticKind::Runtime,
                    "min/max argument is an empty collection",
                ));
            }
            _ => arguments,
        }
    } else {
        arguments
    };
    let Some(first) = values.first() else {
        return Err(RuntimeFailure::new(
            ExpressionDiagnosticKind::InvalidArguments,
            "min/max expects at least one value",
        ));
    };
    let mut result = first;
    for candidate in &values[1..] {
        if ordering(candidate, result)? == wanted {
            result = candidate;
        }
    }
    Ok(result.clone())
}

fn clamp_value(arguments: &[RuntimeValue]) -> Result<RuntimeValue, RuntimeFailure> {
    match &arguments[0] {
        RuntimeValue::Integer(_) | RuntimeValue::Number(_) | RuntimeValue::Bool(_) => {
            let value = number(&arguments[0])?;
            let minimum = number(&arguments[1])?;
            let maximum = number(&arguments[2])?;
            if minimum > maximum {
                return Err(RuntimeFailure::new(
                    ExpressionDiagnosticKind::InvalidArguments,
                    "clamp minimum cannot exceed maximum",
                ));
            }
            finite_number(value.clamp(minimum, maximum))
        }
        RuntimeValue::Vector(kind, values) => {
            let minimum = broadcast_components(&arguments[1], values.len())?;
            let maximum = broadcast_components(&arguments[2], values.len())?;
            let mut result = Vec::with_capacity(values.len());
            for ((value, minimum), maximum) in values.iter().zip(minimum).zip(maximum) {
                if minimum > maximum {
                    return Err(RuntimeFailure::new(
                        ExpressionDiagnosticKind::InvalidArguments,
                        "clamp minimum cannot exceed maximum",
                    ));
                }
                result.push(value.clamp(minimum, maximum));
            }
            Ok(RuntimeValue::Vector(*kind, result))
        }
        _ => Err(RuntimeFailure::type_error(
            "clamp expects a number or vector value",
        )),
    }
}

fn lerp_value(arguments: &[RuntimeValue]) -> Result<RuntimeValue, RuntimeFailure> {
    let amount = number(&arguments[2])?;
    match (&arguments[0], &arguments[1]) {
        (left, right) if is_numeric(left) && is_numeric(right) => {
            let left = number(left)?;
            finite_number(left + (number(right)? - left) * amount)
        }
        (RuntimeValue::Vector(left_kind, left), RuntimeValue::Vector(right_kind, right))
            if left_kind == right_kind =>
        {
            Ok(RuntimeValue::Vector(
                *left_kind,
                left.iter()
                    .zip(right)
                    .map(|(left, right)| left + (right - left) * amount)
                    .collect(),
            ))
        }
        (RuntimeValue::Color(left), RuntimeValue::Color(right)) => {
            Ok(RuntimeValue::Color(std::array::from_fn(|index| {
                left[index] + (right[index] - left[index]) * amount
            })))
        }
        _ => Err(RuntimeFailure::type_error(
            "lerp endpoints must have the same numeric, vector, or color type",
        )),
    }
}

fn smoothstep(arguments: &[RuntimeValue]) -> Result<RuntimeValue, RuntimeFailure> {
    let edge0 = number(&arguments[0])?;
    let edge1 = number(&arguments[1])?;
    let value = number(&arguments[2])?;
    if edge0 == edge1 {
        return Err(RuntimeFailure::new(
            ExpressionDiagnosticKind::InvalidArguments,
            "smoothstep edges must differ",
        ));
    }
    let position = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    finite_number(position * position * (3.0 - 2.0 * position))
}

fn make_vector(
    kind: VectorKind,
    arguments: &[RuntimeValue],
) -> Result<RuntimeValue, RuntimeFailure> {
    arguments
        .iter()
        .map(number)
        .collect::<Result<Vec<_>, _>>()
        .map(|values| RuntimeValue::Vector(kind, values))
}

fn make_color(arguments: &[RuntimeValue], has_alpha: bool) -> Result<RuntimeValue, RuntimeFailure> {
    let mut channels = [1.0; 4];
    for (index, argument) in arguments.iter().enumerate() {
        channels[index] = number(argument)?;
    }
    if !has_alpha {
        channels[3] = 1.0;
    }
    if channels
        .iter()
        .any(|channel| !(0.0..=1.0).contains(channel))
    {
        return Err(RuntimeFailure::type_error(
            "rgb/rgba channels must be in 0.0..=1.0",
        ));
    }
    Ok(RuntimeValue::Color(channels))
}

fn deterministic_random(arguments: &[RuntimeValue]) -> Result<RuntimeValue, RuntimeFailure> {
    let seed = integer(&arguments[0])? as u64;
    finite_number(unit_random(seed))
}

fn deterministic_noise(arguments: &[RuntimeValue]) -> Result<RuntimeValue, RuntimeFailure> {
    let position = number(&arguments[0])?;
    let seed = integer(&arguments[1])? as u64;
    let lower = position.floor();
    let lower_index = match float_to_integer_runtime(lower)? {
        RuntimeValue::Integer(value) => value,
        _ => {
            return Err(RuntimeFailure::new(
                ExpressionDiagnosticKind::Runtime,
                "noise lattice conversion failed",
            ));
        }
    };
    let fraction = position - lower;
    let fade = fraction * fraction * (3.0 - 2.0 * fraction);
    let first = unit_random(seed ^ (lower_index as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15));
    let second_index = lower_index
        .checked_add(1)
        .ok_or_else(|| RuntimeFailure::limit("noise position exceeds the supported range"))?;
    let second = unit_random(seed ^ (second_index as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15));
    finite_number((first + (second - first) * fade) * 2.0 - 1.0)
}

fn vector_length(arguments: &[RuntimeValue]) -> Result<RuntimeValue, RuntimeFailure> {
    let RuntimeValue::Vector(_, values) = &arguments[0] else {
        return Err(RuntimeFailure::type_error("length expects a vector"));
    };
    finite_number(values.iter().map(|value| value * value).sum::<f64>().sqrt())
}

fn normalize_vector(arguments: &[RuntimeValue]) -> Result<RuntimeValue, RuntimeFailure> {
    let RuntimeValue::Vector(kind, values) = &arguments[0] else {
        return Err(RuntimeFailure::type_error("normalize expects a vector"));
    };
    let length = values.iter().map(|value| value * value).sum::<f64>().sqrt();
    if length == 0.0 {
        return Err(RuntimeFailure::new(
            ExpressionDiagnosticKind::DivisionByZero,
            "cannot normalize a zero-length vector",
        ));
    }
    Ok(RuntimeValue::Vector(
        *kind,
        values.iter().map(|value| value / length).collect(),
    ))
}

fn dot_product(arguments: &[RuntimeValue]) -> Result<RuntimeValue, RuntimeFailure> {
    let (RuntimeValue::Vector(left_kind, left), RuntimeValue::Vector(right_kind, right)) =
        (&arguments[0], &arguments[1])
    else {
        return Err(RuntimeFailure::type_error("dot expects two vectors"));
    };
    if left_kind != right_kind {
        return Err(RuntimeFailure::type_error(
            "dot vectors must have matching dimensions",
        ));
    }
    finite_number(
        left.iter()
            .zip(right)
            .map(|(left, right)| left * right)
            .sum(),
    )
}

fn unit_random(seed: u64) -> f64 {
    let mut value = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^= value >> 31;
    (value >> 11) as f64 / (1_u64 << 53) as f64
}

fn add(
    left: RuntimeValue,
    right: RuntimeValue,
    limits: &ExpressionLimits,
) -> Result<RuntimeValue, RuntimeFailure> {
    match (left, right) {
        (RuntimeValue::Integer(left), RuntimeValue::Integer(right)) => left
            .checked_add(right)
            .map(RuntimeValue::Integer)
            .ok_or_else(|| RuntimeFailure::limit("integer addition overflowed")),
        (left, right) if is_numeric(&left) && is_numeric(&right) => {
            finite_number(number(&left)? + number(&right)?)
        }
        (RuntimeValue::String(mut left), RuntimeValue::String(right)) => {
            let length = left.len().saturating_add(right.len());
            if length > limits.max_string_bytes {
                return Err(RuntimeFailure::limit(
                    "string result exceeds its size limit",
                ));
            }
            left.push_str(&right);
            Ok(RuntimeValue::String(left))
        }
        (
            RuntimeValue::Sequence(left_kind, mut left),
            RuntimeValue::Sequence(right_kind, right),
        ) if left_kind == right_kind => {
            if left.len().saturating_add(right.len()) > limits.max_collection_items {
                return Err(RuntimeFailure::limit(
                    "collection result exceeds its item limit",
                ));
            }
            left.extend(right);
            Ok(RuntimeValue::Sequence(left_kind, left))
        }
        (RuntimeValue::Vector(left_kind, left), RuntimeValue::Vector(right_kind, right))
            if left_kind == right_kind =>
        {
            vector_pair(left_kind, &left, &right, |left, right| left + right)
        }
        (RuntimeValue::Color(left), RuntimeValue::Color(right)) => {
            Ok(RuntimeValue::Color(std::array::from_fn(|index| {
                left[index] + right[index]
            })))
        }
        _ => Err(RuntimeFailure::type_error(
            "'+' operands have incompatible types",
        )),
    }
}

fn subtract(left: RuntimeValue, right: RuntimeValue) -> Result<RuntimeValue, RuntimeFailure> {
    match (left, right) {
        (RuntimeValue::Integer(left), RuntimeValue::Integer(right)) => left
            .checked_sub(right)
            .map(RuntimeValue::Integer)
            .ok_or_else(|| RuntimeFailure::limit("integer subtraction overflowed")),
        (left, right) if is_numeric(&left) && is_numeric(&right) => {
            finite_number(number(&left)? - number(&right)?)
        }
        (RuntimeValue::Vector(left_kind, left), RuntimeValue::Vector(right_kind, right))
            if left_kind == right_kind =>
        {
            vector_pair(left_kind, &left, &right, |left, right| left - right)
        }
        (RuntimeValue::Color(left), RuntimeValue::Color(right)) => {
            Ok(RuntimeValue::Color(std::array::from_fn(|index| {
                left[index] - right[index]
            })))
        }
        _ => Err(RuntimeFailure::type_error(
            "'-' operands have incompatible types",
        )),
    }
}

fn multiply(
    left: RuntimeValue,
    right: RuntimeValue,
    limits: &ExpressionLimits,
) -> Result<RuntimeValue, RuntimeFailure> {
    match (left, right) {
        (RuntimeValue::Integer(left), RuntimeValue::Integer(right)) => left
            .checked_mul(right)
            .map(RuntimeValue::Integer)
            .ok_or_else(|| RuntimeFailure::limit("integer multiplication overflowed")),
        (left, right) if is_numeric(&left) && is_numeric(&right) => {
            finite_number(number(&left)? * number(&right)?)
        }
        (RuntimeValue::String(value), count) | (count, RuntimeValue::String(value)) => {
            repeat_string(value, integer(&count)?, limits)
        }
        (RuntimeValue::Sequence(kind, values), count)
        | (count, RuntimeValue::Sequence(kind, values)) => {
            repeat_sequence(kind, values, integer(&count)?, limits)
        }
        (RuntimeValue::Vector(kind, values), scalar)
        | (scalar, RuntimeValue::Vector(kind, values))
            if is_numeric(&scalar) =>
        {
            let scalar = number(&scalar)?;
            Ok(RuntimeValue::Vector(
                kind,
                values.into_iter().map(|value| value * scalar).collect(),
            ))
        }
        (RuntimeValue::Color(values), scalar) | (scalar, RuntimeValue::Color(values))
            if is_numeric(&scalar) =>
        {
            let scalar = number(&scalar)?;
            Ok(RuntimeValue::Color(values.map(|value| value * scalar)))
        }
        _ => Err(RuntimeFailure::type_error(
            "'*' operands have incompatible types",
        )),
    }
}

fn divide(left: RuntimeValue, right: RuntimeValue) -> Result<RuntimeValue, RuntimeFailure> {
    let divisor = number(&right)?;
    if divisor == 0.0 {
        return Err(RuntimeFailure::new(
            ExpressionDiagnosticKind::DivisionByZero,
            "division by zero",
        ));
    }
    match left {
        value if is_numeric(&value) => finite_number(number(&value)? / divisor),
        RuntimeValue::Vector(kind, values) => Ok(RuntimeValue::Vector(
            kind,
            values.into_iter().map(|value| value / divisor).collect(),
        )),
        RuntimeValue::Color(values) => Ok(RuntimeValue::Color(values.map(|value| value / divisor))),
        _ => Err(RuntimeFailure::type_error(
            "'/' expects numeric operands or a vector/color and scalar",
        )),
    }
}

fn floor_divide(left: RuntimeValue, right: RuntimeValue) -> Result<RuntimeValue, RuntimeFailure> {
    match (left, right) {
        (RuntimeValue::Integer(left), RuntimeValue::Integer(right)) => {
            if right == 0 {
                return Err(RuntimeFailure::new(
                    ExpressionDiagnosticKind::DivisionByZero,
                    "integer floor division by zero",
                ));
            }
            let quotient = left
                .checked_div(right)
                .ok_or_else(|| RuntimeFailure::limit("integer floor division overflowed"))?;
            let remainder = left
                .checked_rem(right)
                .ok_or_else(|| RuntimeFailure::limit("integer floor division overflowed"))?;
            let quotient = if remainder != 0 && (remainder < 0) != (right < 0) {
                quotient
                    .checked_sub(1)
                    .ok_or_else(|| RuntimeFailure::limit("integer floor division overflowed"))?
            } else {
                quotient
            };
            Ok(RuntimeValue::Integer(quotient))
        }
        (left, right) if is_numeric(&left) && is_numeric(&right) => {
            let divisor = number(&right)?;
            if divisor == 0.0 {
                return Err(RuntimeFailure::new(
                    ExpressionDiagnosticKind::DivisionByZero,
                    "float floor division by zero",
                ));
            }
            finite_number((number(&left)? / divisor).floor())
        }
        _ => Err(RuntimeFailure::type_error("'//' expects numeric operands")),
    }
}

fn modulo(left: RuntimeValue, right: RuntimeValue) -> Result<RuntimeValue, RuntimeFailure> {
    match (left, right) {
        (RuntimeValue::Integer(left), RuntimeValue::Integer(right)) => {
            if right == 0 {
                return Err(RuntimeFailure::new(
                    ExpressionDiagnosticKind::DivisionByZero,
                    "integer modulo by zero",
                ));
            }
            let remainder = left
                .checked_rem(right)
                .ok_or_else(|| RuntimeFailure::limit("integer modulo overflowed"))?;
            let remainder = if remainder != 0 && (remainder < 0) != (right < 0) {
                remainder
                    .checked_add(right)
                    .ok_or_else(|| RuntimeFailure::limit("integer modulo overflowed"))?
            } else {
                remainder
            };
            Ok(RuntimeValue::Integer(remainder))
        }
        (left, right) if is_numeric(&left) && is_numeric(&right) => {
            let dividend = number(&left)?;
            let divisor = number(&right)?;
            if divisor == 0.0 {
                return Err(RuntimeFailure::new(
                    ExpressionDiagnosticKind::DivisionByZero,
                    "float modulo by zero",
                ));
            }
            let mut remainder = dividend % divisor;
            if remainder != 0.0 {
                if (divisor < 0.0) != (remainder < 0.0) {
                    remainder += divisor;
                }
            } else {
                remainder = 0.0_f64.copysign(divisor);
            }
            finite_number(remainder)
        }
        _ => Err(RuntimeFailure::type_error("'%' expects numeric operands")),
    }
}

fn power(
    left: RuntimeValue,
    right: RuntimeValue,
    limits: &ExpressionLimits,
) -> Result<RuntimeValue, RuntimeFailure> {
    if let (RuntimeValue::Integer(base), RuntimeValue::Integer(exponent)) = (&left, &right)
        && *exponent >= 0
    {
        let exponent = u32::try_from(*exponent)
            .map_err(|_| RuntimeFailure::limit("power exponent exceeds the supported range"))?;
        if exponent > limits.max_exponent_abs {
            return Err(RuntimeFailure::limit(format!(
                "power exponent exceeds {}",
                limits.max_exponent_abs
            )));
        }
        return base
            .checked_pow(exponent)
            .map(RuntimeValue::Integer)
            .ok_or_else(|| RuntimeFailure::limit("integer power overflowed"));
    }
    let base = number(&left)?;
    let exponent = number(&right)?;
    if exponent.abs() > f64::from(limits.max_exponent_abs) {
        return Err(RuntimeFailure::limit(format!(
            "power exponent exceeds {}",
            limits.max_exponent_abs
        )));
    }
    if base == 0.0 && exponent < 0.0 {
        return Err(RuntimeFailure::new(
            ExpressionDiagnosticKind::DivisionByZero,
            "zero cannot be raised to a negative power",
        ));
    }
    if base < 0.0 && exponent.fract() != 0.0 {
        return Err(RuntimeFailure::type_error(
            "complex power results are not supported",
        ));
    }
    finite_number(base.powf(exponent))
}

fn unary_plus(value: RuntimeValue) -> Result<RuntimeValue, RuntimeFailure> {
    match value {
        RuntimeValue::Integer(_) | RuntimeValue::Number(_) => Ok(value),
        RuntimeValue::Bool(value) => Ok(RuntimeValue::Integer(i64::from(value))),
        _ => Err(RuntimeFailure::type_error("unary '+' expects a number")),
    }
}

fn negate(value: RuntimeValue) -> Result<RuntimeValue, RuntimeFailure> {
    match value {
        RuntimeValue::Integer(value) => value
            .checked_neg()
            .map(RuntimeValue::Integer)
            .ok_or_else(|| RuntimeFailure::limit("integer negation overflowed")),
        RuntimeValue::Number(value) => finite_number(-value),
        RuntimeValue::Bool(value) => Ok(RuntimeValue::Integer(-i64::from(value))),
        RuntimeValue::Vector(kind, values) => Ok(RuntimeValue::Vector(
            kind,
            values.into_iter().map(|value| -value).collect(),
        )),
        _ => Err(RuntimeFailure::type_error(
            "unary '-' expects a number or vector",
        )),
    }
}

fn repeat_string(
    value: String,
    count: i64,
    limits: &ExpressionLimits,
) -> Result<RuntimeValue, RuntimeFailure> {
    let count = if count <= 0 {
        0
    } else {
        usize::try_from(count)
            .map_err(|_| RuntimeFailure::limit("string repetition count is too large"))?
    };
    let bytes = value
        .len()
        .checked_mul(count)
        .ok_or_else(|| RuntimeFailure::limit("string result exceeds its size limit"))?;
    if bytes > limits.max_string_bytes {
        return Err(RuntimeFailure::limit(
            "string result exceeds its size limit",
        ));
    }
    Ok(RuntimeValue::String(value.repeat(count)))
}

fn repeat_sequence(
    kind: SequenceKind,
    values: Vec<RuntimeValue>,
    count: i64,
    limits: &ExpressionLimits,
) -> Result<RuntimeValue, RuntimeFailure> {
    let count = if count <= 0 {
        0
    } else {
        usize::try_from(count)
            .map_err(|_| RuntimeFailure::limit("collection repetition count is too large"))?
    };
    let item_count = values
        .len()
        .checked_mul(count)
        .ok_or_else(|| RuntimeFailure::limit("collection result exceeds its item limit"))?;
    if item_count > limits.max_collection_items {
        return Err(RuntimeFailure::limit(
            "collection result exceeds its item limit",
        ));
    }
    let mut repeated = Vec::with_capacity(item_count);
    for _ in 0..count {
        repeated.extend(values.iter().cloned());
    }
    Ok(RuntimeValue::Sequence(kind, repeated))
}

fn compare(
    operator: CmpOp,
    left: &RuntimeValue,
    right: &RuntimeValue,
) -> Result<bool, RuntimeFailure> {
    match operator {
        CmpOp::Eq => Ok(equal(left, right)),
        CmpOp::NotEq => Ok(!equal(left, right)),
        CmpOp::Lt => Ok(ordering(left, right)? == Ordering::Less),
        CmpOp::LtE => Ok(ordering(left, right)? != Ordering::Greater),
        CmpOp::Gt => Ok(ordering(left, right)? == Ordering::Greater),
        CmpOp::GtE => Ok(ordering(left, right)? != Ordering::Less),
        _ => Err(RuntimeFailure::unsupported("comparison operator")),
    }
}

fn equal(left: &RuntimeValue, right: &RuntimeValue) -> bool {
    if is_numeric(left) && is_numeric(right) {
        return number(left).ok() == number(right).ok();
    }
    match (left, right) {
        (RuntimeValue::Bool(left), RuntimeValue::Bool(right)) => left == right,
        (RuntimeValue::String(left), RuntimeValue::String(right)) => left == right,
        (RuntimeValue::Sequence(left_kind, left), RuntimeValue::Sequence(right_kind, right)) => {
            left_kind == right_kind
                && left.len() == right.len()
                && left.iter().zip(right).all(|(a, b)| equal(a, b))
        }
        (RuntimeValue::Vector(left_kind, left), RuntimeValue::Vector(right_kind, right)) => {
            left_kind == right_kind && left == right
        }
        (RuntimeValue::Color(left), RuntimeValue::Color(right)) => left == right,
        _ => false,
    }
}

fn ordering(left: &RuntimeValue, right: &RuntimeValue) -> Result<Ordering, RuntimeFailure> {
    if is_numeric(left) && is_numeric(right) {
        return number(left)?
            .partial_cmp(&number(right)?)
            .ok_or_else(|| RuntimeFailure::non_finite("comparison produced an invalid number"));
    }
    match (left, right) {
        (RuntimeValue::String(left), RuntimeValue::String(right)) => Ok(left.cmp(right)),
        _ => Err(RuntimeFailure::type_error(
            "ordered comparison requires two numbers or two strings",
        )),
    }
}

fn truthy(value: &RuntimeValue) -> bool {
    match value {
        RuntimeValue::Integer(value) => *value != 0,
        RuntimeValue::Number(value) => *value != 0.0,
        RuntimeValue::Bool(value) => *value,
        RuntimeValue::String(value) => !value.is_empty(),
        RuntimeValue::Sequence(_, values) => !values.is_empty(),
        RuntimeValue::Vector(_, _) | RuntimeValue::Color(_) => true,
    }
}

fn is_numeric(value: &RuntimeValue) -> bool {
    matches!(
        value,
        RuntimeValue::Integer(_) | RuntimeValue::Number(_) | RuntimeValue::Bool(_)
    )
}

fn number(value: &RuntimeValue) -> Result<f64, RuntimeFailure> {
    match value {
        RuntimeValue::Integer(value) => Ok(*value as f64),
        RuntimeValue::Number(value) => Ok(*value),
        RuntimeValue::Bool(value) => Ok(f64::from(*value)),
        _ => Err(RuntimeFailure::type_error("expected a number")),
    }
}

fn integer(value: &RuntimeValue) -> Result<i64, RuntimeFailure> {
    match value {
        RuntimeValue::Integer(value) => Ok(*value),
        RuntimeValue::Bool(value) => Ok(i64::from(*value)),
        _ => Err(RuntimeFailure::type_error("expected an integer")),
    }
}

fn two_numbers(arguments: &[RuntimeValue]) -> Result<(f64, f64), RuntimeFailure> {
    Ok((number(&arguments[0])?, number(&arguments[1])?))
}

fn finite_number(value: f64) -> Result<RuntimeValue, RuntimeFailure> {
    if value.is_finite() {
        Ok(RuntimeValue::Number(value))
    } else {
        Err(RuntimeFailure::non_finite(
            "numeric operation produced a non-finite result",
        ))
    }
}

fn float_to_integer_runtime(value: f64) -> Result<RuntimeValue, RuntimeFailure> {
    float_to_integer(value).map_err(RuntimeFailure::limit)
}

fn float_to_integer(value: f64) -> Result<RuntimeValue, String> {
    if !value.is_finite() || value < i64::MIN as f64 || value > i64::MAX as f64 {
        return Err("integer result exceeds the supported 64-bit range".to_string());
    }
    Ok(RuntimeValue::Integer(value as i64))
}

fn u64_to_integer(value: u64) -> Result<RuntimeValue, String> {
    i64::try_from(value)
        .map(RuntimeValue::Integer)
        .map_err(|_| "resolution exceeds the supported 64-bit range".to_string())
}

fn vector_pair(
    kind: VectorKind,
    left: &[f64],
    right: &[f64],
    operation: impl Fn(f64, f64) -> f64,
) -> Result<RuntimeValue, RuntimeFailure> {
    let values = left
        .iter()
        .zip(right)
        .map(|(left, right)| operation(*left, *right))
        .collect::<Vec<_>>();
    if values.iter().all(|value| value.is_finite()) {
        Ok(RuntimeValue::Vector(kind, values))
    } else {
        Err(RuntimeFailure::non_finite(
            "vector operation produced a non-finite result",
        ))
    }
}

fn broadcast_components(value: &RuntimeValue, count: usize) -> Result<Vec<f64>, RuntimeFailure> {
    match value {
        value if is_numeric(value) => Ok(vec![number(value)?; count]),
        RuntimeValue::Vector(_, values) if values.len() == count => Ok(values.clone()),
        _ => Err(RuntimeFailure::type_error(
            "vector bound must be a scalar or matching vector",
        )),
    }
}

fn validate_runtime_value(
    value: RuntimeValue,
    limits: &ExpressionLimits,
) -> Result<RuntimeValue, RuntimeFailure> {
    let valid = match &value {
        RuntimeValue::Integer(_) | RuntimeValue::Bool(_) => true,
        RuntimeValue::Number(value) => value.is_finite(),
        RuntimeValue::String(value) => value.len() <= limits.max_string_bytes,
        RuntimeValue::Sequence(_, values) => values.len() <= limits.max_collection_items,
        RuntimeValue::Vector(_, values) => values.iter().all(|value| value.is_finite()),
        RuntimeValue::Color(values) => values.iter().all(|value| value.is_finite()),
    };
    if valid {
        Ok(value)
    } else {
        Err(RuntimeFailure::non_finite(
            "helper produced an invalid or oversized value",
        ))
    }
}

fn validate_context(
    context: &ExpressionEvaluationContext,
    output_type: ExpressionOutputType,
    limits: &ExpressionLimits,
) -> Result<(), ExpressionDiagnostic> {
    if let Some(value) = context.value() {
        if value.output_type() != output_type {
            return Err(ExpressionDiagnostic::evaluate(
                ExpressionDiagnosticKind::InvalidContext,
                format!(
                    "value has type {:?}, expected {:?}",
                    value.output_type(),
                    output_type
                ),
                None,
            ));
        }
        validate_public_value(value, limits)?;
    }
    Ok(())
}

fn validate_public_value(
    value: &ExpressionValue,
    limits: &ExpressionLimits,
) -> Result<(), ExpressionDiagnostic> {
    let valid = match value {
        ExpressionValue::Number(value) => value.is_finite(),
        ExpressionValue::Vec2(values) => values.iter().all(|value| value.is_finite()),
        ExpressionValue::Vec3(values) => values.iter().all(|value| value.is_finite()),
        ExpressionValue::Vec4(values) => values.iter().all(|value| value.is_finite()),
        ExpressionValue::Color(values) => values
            .iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(value)),
        ExpressionValue::Bool(_) => true,
        ExpressionValue::String(value) => value.len() <= limits.max_string_bytes,
    };
    if valid {
        Ok(())
    } else {
        Err(ExpressionDiagnostic::evaluate(
            ExpressionDiagnosticKind::InvalidContext,
            "value fallback is non-finite, out of range, or oversized",
            None,
        ))
    }
}

fn runtime_from_public(value: &ExpressionValue) -> Result<RuntimeValue, ExpressionDiagnostic> {
    Ok(match value {
        ExpressionValue::Number(value) => RuntimeValue::Number(*value),
        ExpressionValue::Vec2(values) => RuntimeValue::Vector(VectorKind::Vec2, values.to_vec()),
        ExpressionValue::Vec3(values) => RuntimeValue::Vector(VectorKind::Vec3, values.to_vec()),
        ExpressionValue::Vec4(values) => RuntimeValue::Vector(VectorKind::Vec4, values.to_vec()),
        ExpressionValue::Color(values) => RuntimeValue::Color(*values),
        ExpressionValue::Bool(value) => RuntimeValue::Bool(*value),
        ExpressionValue::String(value) => RuntimeValue::String(value.clone()),
    })
}

fn convert_output(
    value: RuntimeValue,
    output_type: ExpressionOutputType,
    limits: &ExpressionLimits,
) -> Result<ExpressionValue, ExpressionDiagnostic> {
    let output = match (output_type, value) {
        (ExpressionOutputType::Number, RuntimeValue::Number(value)) if value.is_finite() => {
            ExpressionValue::Number(value)
        }
        (ExpressionOutputType::Number, RuntimeValue::Integer(value))
            if (-MAX_EXACT_F64_INTEGER..=MAX_EXACT_F64_INTEGER).contains(&value) =>
        {
            ExpressionValue::Number(value as f64)
        }
        (ExpressionOutputType::Vec2, RuntimeValue::Vector(VectorKind::Vec2, values)) => {
            ExpressionValue::Vec2([values[0], values[1]])
        }
        (ExpressionOutputType::Vec3, RuntimeValue::Vector(VectorKind::Vec3, values)) => {
            ExpressionValue::Vec3([values[0], values[1], values[2]])
        }
        (ExpressionOutputType::Vec4, RuntimeValue::Vector(VectorKind::Vec4, values)) => {
            ExpressionValue::Vec4([values[0], values[1], values[2], values[3]])
        }
        (ExpressionOutputType::Color, RuntimeValue::Color(values))
            if values
                .iter()
                .all(|value| value.is_finite() && (0.0..=1.0).contains(value)) =>
        {
            ExpressionValue::Color(values)
        }
        (ExpressionOutputType::Bool, RuntimeValue::Bool(value)) => ExpressionValue::Bool(value),
        (ExpressionOutputType::String, RuntimeValue::String(value))
            if value.len() <= limits.max_string_bytes =>
        {
            ExpressionValue::String(value)
        }
        (expected, actual) => {
            return Err(ExpressionDiagnostic::evaluate(
                ExpressionDiagnosticKind::TypeMismatch,
                format!(
                    "Expression returned {}, expected {expected:?}",
                    runtime_type_name(&actual)
                ),
                None,
            ));
        }
    };
    Ok(output)
}

fn runtime_type_name(value: &RuntimeValue) -> &'static str {
    match value {
        RuntimeValue::Integer(_) => "Integer",
        RuntimeValue::Number(_) => "Number",
        RuntimeValue::Bool(_) => "Bool",
        RuntimeValue::String(_) => "String",
        RuntimeValue::Sequence(SequenceKind::List, _) => "List",
        RuntimeValue::Sequence(SequenceKind::Tuple, _) => "Tuple",
        RuntimeValue::Vector(VectorKind::Vec2, _) => "Vec2",
        RuntimeValue::Vector(VectorKind::Vec3, _) => "Vec3",
        RuntimeValue::Vector(VectorKind::Vec4, _) => "Vec4",
        RuntimeValue::Color(_) => "Color",
    }
}

fn python_index(index: i64, length: usize) -> Option<usize> {
    let length = i64::try_from(length).ok()?;
    let normalized = if index < 0 {
        length.checked_add(index)?
    } else {
        index
    };
    (0..length)
        .contains(&normalized)
        .then(|| usize::try_from(normalized).ok())
        .flatten()
}

fn is_context_name(name: &str) -> bool {
    matches!(
        name,
        "time" | "fps" | "frame" | "frame_index" | "width" | "height" | "resolution" | "value"
    )
}

fn is_constant_name(name: &str) -> bool {
    matches!(name, "pi" | "tau" | "e")
}

fn is_component_name(name: &str) -> bool {
    matches!(name, "x" | "y" | "z" | "w" | "r" | "g" | "b" | "a")
}

fn is_math_name(expression: &ast::Expr) -> bool {
    matches!(expression, ast::Expr::Name(node) if node.id.as_str() == "math")
}

fn is_math_constant(expression: &ast::Expr, attribute: &str) -> bool {
    is_math_name(expression) && matches!(attribute, "pi" | "tau" | "e")
}

fn math_constant(attribute: &str) -> f64 {
    match attribute {
        "pi" => std::f64::consts::PI,
        "tau" => std::f64::consts::TAU,
        "e" => std::f64::consts::E,
        _ => 0.0,
    }
}

fn expression_span(expression: &ast::Expr) -> ExpressionSourceSpan {
    let range = expression.range();
    ExpressionSourceSpan {
        start: u32::from(range.start()) as usize,
        end: u32::from(range.end()) as usize,
    }
}

fn compile_limit(message: impl Into<String>, expression: &ast::Expr) -> ExpressionDiagnostic {
    ExpressionDiagnostic::compile(
        ExpressionDiagnosticKind::ResourceLimit,
        message,
        Some(expression_span(expression)),
    )
}

fn unsupported(name: &str, expression: &ast::Expr) -> ExpressionDiagnostic {
    ExpressionDiagnostic::compile(
        ExpressionDiagnosticKind::UnsupportedSyntax,
        format!("{name} is not supported by the Python Expression subset"),
        Some(expression_span(expression)),
    )
}

fn ast_name(expression: &ast::Expr) -> &'static str {
    match expression {
        ast::Expr::BoolOp(_) => "boolean expression",
        ast::Expr::NamedExpr(_) => "assignment expression",
        ast::Expr::BinOp(_) => "binary expression",
        ast::Expr::UnaryOp(_) => "unary expression",
        ast::Expr::Lambda(_) => "lambda",
        ast::Expr::IfExp(_) => "conditional expression",
        ast::Expr::Dict(_) => "dictionary",
        ast::Expr::Set(_) => "set",
        ast::Expr::ListComp(_) => "list comprehension",
        ast::Expr::SetComp(_) => "set comprehension",
        ast::Expr::DictComp(_) => "dictionary comprehension",
        ast::Expr::GeneratorExp(_) => "generator expression",
        ast::Expr::Await(_) => "await",
        ast::Expr::Yield(_) => "yield",
        ast::Expr::YieldFrom(_) => "yield from",
        ast::Expr::Compare(_) => "comparison",
        ast::Expr::Call(_) => "function call",
        ast::Expr::FormattedValue(_) | ast::Expr::JoinedStr(_) => "formatted string",
        ast::Expr::Constant(_) => "literal",
        ast::Expr::Attribute(_) => "attribute access",
        ast::Expr::Subscript(_) => "index",
        ast::Expr::Starred(_) => "unpacking",
        ast::Expr::Name(_) => "name",
        ast::Expr::List(_) => "list",
        ast::Expr::Tuple(_) => "tuple",
        ast::Expr::Slice(_) => "slice",
    }
}
