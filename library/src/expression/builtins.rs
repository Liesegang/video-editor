use std::cmp::Ordering;

use rustpython_parser::ast;

use super::engine::ExpressionLimits;
use super::evaluator::{RuntimeValue, SequenceKind, VectorKind};
use super::semantics::{
    broadcast_components, expression_span, finite_number, float_to_integer_runtime, integer,
    is_math_name, is_numeric, number, ordering, two_numbers, validate_runtime_value, vector_pair,
};
use super::{ExpressionDiagnostic, ExpressionDiagnosticKind};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Function {
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

pub(super) fn resolve_function(expression: &ast::Expr) -> Option<Function> {
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

pub(super) fn validate_arity(
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
pub(super) struct RuntimeFailure {
    pub(super) kind: ExpressionDiagnosticKind,
    pub(super) message: String,
}

impl RuntimeFailure {
    fn new(kind: ExpressionDiagnosticKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub(super) fn type_error(message: impl Into<String>) -> Self {
        Self::new(ExpressionDiagnosticKind::TypeMismatch, message)
    }

    pub(super) fn non_finite(message: impl Into<String>) -> Self {
        Self::new(ExpressionDiagnosticKind::NonFinite, message)
    }

    pub(super) fn limit(message: impl Into<String>) -> Self {
        Self::new(ExpressionDiagnosticKind::ResourceLimit, message)
    }

    pub(super) fn unsupported(message: impl Into<String>) -> Self {
        Self::new(ExpressionDiagnosticKind::UnsupportedSyntax, message)
    }
}

pub(super) fn call_function(
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
    if !length.is_finite() {
        return Err(RuntimeFailure::non_finite("vector length is not finite"));
    }
    if length == 0.0 {
        return Err(RuntimeFailure::new(
            ExpressionDiagnosticKind::DivisionByZero,
            "cannot normalize a zero-length vector",
        ));
    }
    let normalized = values
        .iter()
        .map(|value| value / length)
        .collect::<Vec<_>>();
    if normalized.iter().any(|value| !value.is_finite()) {
        return Err(RuntimeFailure::non_finite(
            "normalized vector contains a non-finite component",
        ));
    }
    Ok(RuntimeValue::Vector(*kind, normalized))
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

pub(super) fn add(
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

pub(super) fn subtract(
    left: RuntimeValue,
    right: RuntimeValue,
) -> Result<RuntimeValue, RuntimeFailure> {
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

pub(super) fn multiply(
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

pub(super) fn divide(
    left: RuntimeValue,
    right: RuntimeValue,
) -> Result<RuntimeValue, RuntimeFailure> {
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

pub(super) fn floor_divide(
    left: RuntimeValue,
    right: RuntimeValue,
) -> Result<RuntimeValue, RuntimeFailure> {
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

pub(super) fn modulo(
    left: RuntimeValue,
    right: RuntimeValue,
) -> Result<RuntimeValue, RuntimeFailure> {
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

pub(super) fn power(
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

pub(super) fn unary_plus(value: RuntimeValue) -> Result<RuntimeValue, RuntimeFailure> {
    match value {
        RuntimeValue::Integer(_) | RuntimeValue::Number(_) => Ok(value),
        RuntimeValue::Bool(value) => Ok(RuntimeValue::Integer(i64::from(value))),
        _ => Err(RuntimeFailure::type_error("unary '+' expects a number")),
    }
}

pub(super) fn negate(value: RuntimeValue) -> Result<RuntimeValue, RuntimeFailure> {
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
