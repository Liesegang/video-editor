use rustpython_parser::ast::{self, CmpOp, Constant, Operator, UnaryOp};

use super::builtins::{resolve_function, validate_arity};
use super::engine::ExpressionLimits;
use super::semantics::{
    ast_name, compile_limit, expression_span, is_component_name, is_constant_name, is_context_name,
    is_math_constant, unsupported,
};
use super::{ExpressionDiagnostic, ExpressionDiagnosticKind};

pub(super) struct ValidationState<'a> {
    pub(super) limits: &'a ExpressionLimits,
    pub(super) nodes: u64,
}

impl ValidationState<'_> {
    pub(super) fn validate(
        &mut self,
        expression: &ast::Expr,
        depth: u32,
    ) -> Result<(), ExpressionDiagnostic> {
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
