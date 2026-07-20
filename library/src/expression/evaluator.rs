use std::time::Instant;

use rustpython_parser::ast::{self, BoolOp, CmpOp, Constant, Operator, UnaryOp};

use super::builtins::{
    Function, RuntimeFailure, add, call_function, divide, floor_divide, modulo, multiply, negate,
    power, resolve_function, subtract, unary_plus,
};
use super::engine::ExpressionLimits;
use super::semantics::{
    ast_name, compare, expression_span, float_to_integer, integer, is_math_constant, math_constant,
    python_index, runtime_from_public, truthy, u64_to_integer,
};
use super::{
    ExpressionDiagnostic, ExpressionDiagnosticKind, ExpressionEvaluationContext,
    ExpressionSourceSpan,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SequenceKind {
    List,
    Tuple,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VectorKind {
    Vec2,
    Vec3,
    Vec4,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum RuntimeValue {
    Integer(i64),
    Number(f64),
    Bool(bool),
    String(String),
    Sequence(SequenceKind, Vec<RuntimeValue>),
    Vector(VectorKind, Vec<f64>),
    Color([f64; 4]),
}

pub(super) struct EvaluationState<'a> {
    context: &'a ExpressionEvaluationContext,
    limits: &'a ExpressionLimits,
    started: Instant,
    operations: u64,
    calls: u64,
}

impl<'a> EvaluationState<'a> {
    pub(super) fn new(
        context: &'a ExpressionEvaluationContext,
        limits: &'a ExpressionLimits,
    ) -> Self {
        Self {
            context,
            limits,
            started: Instant::now(),
            operations: 0,
            calls: 0,
        }
    }

    pub(super) fn evaluate(
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
        self.calls = self.calls.saturating_add(1);
        if self.calls > self.limits.max_calls {
            return Err(self.error(
                ExpressionDiagnosticKind::ResourceLimit,
                format!("evaluation exceeds {} helper calls", self.limits.max_calls),
                expression,
            ));
        }
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
