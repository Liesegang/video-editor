use std::cmp::Ordering;

use rustpython_parser::ast::{self, CmpOp, Ranged};

use super::builtins::RuntimeFailure;
use super::engine::ExpressionLimits;
use super::evaluator::{RuntimeValue, SequenceKind, VectorKind};
use super::{
    ExpressionDiagnostic, ExpressionDiagnosticKind, ExpressionEvaluationContext,
    ExpressionOutputType, ExpressionSourceSpan, ExpressionValue,
};

const MAX_EXACT_F64_INTEGER: i64 = 1_i64 << 53;

pub(super) fn compare(
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

pub(super) fn ordering(
    left: &RuntimeValue,
    right: &RuntimeValue,
) -> Result<Ordering, RuntimeFailure> {
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

pub(super) fn truthy(value: &RuntimeValue) -> bool {
    match value {
        RuntimeValue::Integer(value) => *value != 0,
        RuntimeValue::Number(value) => *value != 0.0,
        RuntimeValue::Bool(value) => *value,
        RuntimeValue::String(value) => !value.is_empty(),
        RuntimeValue::Sequence(_, values) => !values.is_empty(),
        RuntimeValue::Vector(_, _) | RuntimeValue::Color(_) => true,
    }
}

pub(super) fn is_numeric(value: &RuntimeValue) -> bool {
    matches!(
        value,
        RuntimeValue::Integer(_) | RuntimeValue::Number(_) | RuntimeValue::Bool(_)
    )
}

pub(super) fn number(value: &RuntimeValue) -> Result<f64, RuntimeFailure> {
    match value {
        RuntimeValue::Integer(value) => Ok(*value as f64),
        RuntimeValue::Number(value) => Ok(*value),
        RuntimeValue::Bool(value) => Ok(f64::from(*value)),
        _ => Err(RuntimeFailure::type_error("expected a number")),
    }
}

pub(super) fn integer(value: &RuntimeValue) -> Result<i64, RuntimeFailure> {
    match value {
        RuntimeValue::Integer(value) => Ok(*value),
        RuntimeValue::Bool(value) => Ok(i64::from(*value)),
        _ => Err(RuntimeFailure::type_error("expected an integer")),
    }
}

pub(super) fn two_numbers(arguments: &[RuntimeValue]) -> Result<(f64, f64), RuntimeFailure> {
    Ok((number(&arguments[0])?, number(&arguments[1])?))
}

pub(super) fn finite_number(value: f64) -> Result<RuntimeValue, RuntimeFailure> {
    if value.is_finite() {
        Ok(RuntimeValue::Number(value))
    } else {
        Err(RuntimeFailure::non_finite(
            "numeric operation produced a non-finite result",
        ))
    }
}

pub(super) fn float_to_integer_runtime(value: f64) -> Result<RuntimeValue, RuntimeFailure> {
    float_to_integer(value).map_err(RuntimeFailure::limit)
}

pub(super) fn float_to_integer(value: f64) -> Result<RuntimeValue, String> {
    // `i64::MAX as f64` rounds to 2^63, which is already one past the valid
    // range; keep that upper boundary exclusive. `i64::MIN` is exact.
    if !value.is_finite() || value < i64::MIN as f64 || value >= i64::MAX as f64 {
        return Err("integer result exceeds the supported 64-bit range".to_string());
    }
    Ok(RuntimeValue::Integer(value as i64))
}

pub(super) fn u64_to_integer(value: u64) -> Result<RuntimeValue, String> {
    i64::try_from(value)
        .map(RuntimeValue::Integer)
        .map_err(|_| "resolution exceeds the supported 64-bit range".to_string())
}

pub(super) fn vector_pair(
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

pub(super) fn broadcast_components(
    value: &RuntimeValue,
    count: usize,
) -> Result<Vec<f64>, RuntimeFailure> {
    match value {
        value if is_numeric(value) => Ok(vec![number(value)?; count]),
        RuntimeValue::Vector(_, values) if values.len() == count => Ok(values.clone()),
        _ => Err(RuntimeFailure::type_error(
            "vector bound must be a scalar or matching vector",
        )),
    }
}

pub(super) fn validate_runtime_value(
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

pub(super) fn validate_context(
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
        ExpressionValue::Integer(_) => true,
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

pub(super) fn runtime_from_public(
    value: &ExpressionValue,
) -> Result<RuntimeValue, ExpressionDiagnostic> {
    Ok(match value {
        ExpressionValue::Number(value) => RuntimeValue::Number(*value),
        ExpressionValue::Integer(value) => RuntimeValue::Integer(*value),
        ExpressionValue::Vec2(values) => RuntimeValue::Vector(VectorKind::Vec2, values.to_vec()),
        ExpressionValue::Vec3(values) => RuntimeValue::Vector(VectorKind::Vec3, values.to_vec()),
        ExpressionValue::Vec4(values) => RuntimeValue::Vector(VectorKind::Vec4, values.to_vec()),
        ExpressionValue::Color(values) => RuntimeValue::Color(*values),
        ExpressionValue::Bool(value) => RuntimeValue::Bool(*value),
        ExpressionValue::String(value) => RuntimeValue::String(value.clone()),
    })
}

pub(super) fn convert_output(
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
        (ExpressionOutputType::Integer, RuntimeValue::Integer(value)) => {
            ExpressionValue::Integer(value)
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

pub(super) fn python_index(index: i64, length: usize) -> Option<usize> {
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

pub(super) fn is_context_name(name: &str) -> bool {
    matches!(
        name,
        "time" | "fps" | "frame" | "frame_index" | "width" | "height" | "resolution" | "value"
    )
}

pub(super) fn is_constant_name(name: &str) -> bool {
    matches!(name, "pi" | "tau" | "e")
}

pub(super) fn is_component_name(name: &str) -> bool {
    matches!(name, "x" | "y" | "z" | "w" | "r" | "g" | "b" | "a")
}

pub(super) fn is_math_name(expression: &ast::Expr) -> bool {
    matches!(expression, ast::Expr::Name(node) if node.id.as_str() == "math")
}

pub(super) fn is_math_constant(expression: &ast::Expr, attribute: &str) -> bool {
    is_math_name(expression) && matches!(attribute, "pi" | "tau" | "e")
}

pub(super) fn math_constant(attribute: &str) -> f64 {
    match attribute {
        "pi" => std::f64::consts::PI,
        "tau" => std::f64::consts::TAU,
        "e" => std::f64::consts::E,
        _ => 0.0,
    }
}

pub(super) fn expression_span(expression: &ast::Expr) -> ExpressionSourceSpan {
    let range = expression.range();
    ExpressionSourceSpan {
        start: u32::from(range.start()) as usize,
        end: u32::from(range.end()) as usize,
    }
}

pub(super) fn compile_limit(
    message: impl Into<String>,
    expression: &ast::Expr,
) -> ExpressionDiagnostic {
    ExpressionDiagnostic::compile(
        ExpressionDiagnosticKind::ResourceLimit,
        message,
        Some(expression_span(expression)),
    )
}

pub(super) fn unsupported(name: &str, expression: &ast::Expr) -> ExpressionDiagnostic {
    ExpressionDiagnostic::compile(
        ExpressionDiagnosticKind::UnsupportedSyntax,
        format!("{name} is not supported by the Python Expression subset"),
        Some(expression_span(expression)),
    )
}

pub(super) fn ast_name(expression: &ast::Expr) -> &'static str {
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
