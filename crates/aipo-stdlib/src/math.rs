//! Canonical `math` module for Aipo.
//!
//! Provides core mathematical operations and constants:
//! - `abs`: absolute value for integers and floats
//! - `min`: minimum of two numbers
//! - `max`: maximum of two numbers
//! - `floor`: floor to integer
//! - `ceil`: ceiling to integer
//! - `round`: round to nearest integer
//! - `sqrt`: square root (returns Failure for negative numbers)
//! - `pow`: exponentiation with bounds checks
//! - `truncate`: truncation toward zero, returning `Int`
//! - `clamp`: constrains a number to an inclusive interval
//! - `pi`: constant Archimedes ratio
//! - `e`: constant Euler's number
//! - `tau`: constant circle ratio (2 * pi)

use aipo_vm::{
    DictMap, FailureValue, MAX_SAFE_INT, MIN_SAFE_INT, Value, VmFault, check_finite_float,
    check_safe_int,
};
use std::cell::RefCell;
use std::rc::Rc;

/// Mathematical absolute value.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not Int or Float,
/// or `VmFault::Overflow` if integer overflows.
pub fn math_abs(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    match &args[0] {
        Value::Int(n) => {
            if *n == i64::MIN {
                return Err(VmFault::Overflow {
                    details: "math.abs integer overflow".to_string(),
                });
            }
            check_safe_int(n.abs()).map(Value::Int)
        }
        Value::Float(f) => check_finite_float(f.abs()).map(Value::Float),
        Value::Failure(f) => Ok(Value::Failure(Rc::clone(f))),
        other => Err(VmFault::TypeMismatch {
            expected: "Int or Float".to_string(),
            actual: other.type_name().to_string(),
        }),
    }
}

/// Returns the smaller of two numbers.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operands are not Int or Float.
pub fn math_min(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 2 {
        return Err(VmFault::TypeMismatch {
            expected: "2 arguments".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    match (&args[0], &args[1]) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int((*a).min(*b))),
        (Value::Float(a), Value::Float(b)) => check_finite_float(a.min(*b)).map(Value::Float),
        (Value::Int(a), Value::Float(b)) => {
            #[allow(clippy::cast_precision_loss)]
            let af = *a as f64;
            check_finite_float(af.min(*b)).map(Value::Float)
        }
        (Value::Float(a), Value::Int(b)) => {
            #[allow(clippy::cast_precision_loss)]
            let bf = *b as f64;
            check_finite_float(a.min(bf)).map(Value::Float)
        }
        (Value::Failure(f), _) | (_, Value::Failure(f)) => Ok(Value::Failure(Rc::clone(f))),
        (a, b) => Err(VmFault::TypeMismatch {
            expected: "two numbers (Int or Float)".to_string(),
            actual: format!("{} and {}", a.type_name(), b.type_name()),
        }),
    }
}

/// Returns the larger of two numbers.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operands are not Int or Float.
pub fn math_max(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 2 {
        return Err(VmFault::TypeMismatch {
            expected: "2 arguments".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    match (&args[0], &args[1]) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int((*a).max(*b))),
        (Value::Float(a), Value::Float(b)) => check_finite_float(a.max(*b)).map(Value::Float),
        (Value::Int(a), Value::Float(b)) => {
            #[allow(clippy::cast_precision_loss)]
            let af = *a as f64;
            check_finite_float(af.max(*b)).map(Value::Float)
        }
        (Value::Float(a), Value::Int(b)) => {
            #[allow(clippy::cast_precision_loss)]
            let bf = *b as f64;
            check_finite_float(a.max(bf)).map(Value::Float)
        }
        (Value::Failure(f), _) | (_, Value::Failure(f)) => Ok(Value::Failure(Rc::clone(f))),
        (a, b) => Err(VmFault::TypeMismatch {
            expected: "two numbers (Int or Float)".to_string(),
            actual: format!("{} and {}", a.type_name(), b.type_name()),
        }),
    }
}

/// Floor function to nearest lower integer.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not Int or Float,
/// or `VmFault::Overflow` if result exceeds safe integer range.
pub fn math_floor(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::Int(*n)),
        Value::Float(f) => {
            let floored = f.floor();
            #[allow(clippy::cast_possible_truncation)]
            if floored < MIN_SAFE_INT as f64 || floored > MAX_SAFE_INT as f64 {
                return Err(VmFault::Overflow {
                    details: "math.floor exceeds safe integer bounds".to_string(),
                });
            }
            #[allow(clippy::cast_possible_truncation)]
            check_safe_int(floored as i64).map(Value::Int)
        }
        Value::Failure(f) => Ok(Value::Failure(Rc::clone(f))),
        other => Err(VmFault::TypeMismatch {
            expected: "Int or Float".to_string(),
            actual: other.type_name().to_string(),
        }),
    }
}

/// Ceiling function to nearest upper integer.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not Int or Float,
/// or `VmFault::Overflow` if result exceeds safe integer range.
pub fn math_ceil(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::Int(*n)),
        Value::Float(f) => {
            let ceiled = f.ceil();
            #[allow(clippy::cast_possible_truncation)]
            if ceiled < MIN_SAFE_INT as f64 || ceiled > MAX_SAFE_INT as f64 {
                return Err(VmFault::Overflow {
                    details: "math.ceil exceeds safe integer bounds".to_string(),
                });
            }
            #[allow(clippy::cast_possible_truncation)]
            check_safe_int(ceiled as i64).map(Value::Int)
        }
        Value::Failure(f) => Ok(Value::Failure(Rc::clone(f))),
        other => Err(VmFault::TypeMismatch {
            expected: "Int or Float".to_string(),
            actual: other.type_name().to_string(),
        }),
    }
}

/// Rounds to nearest integer.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not Int or Float,
/// or `VmFault::Overflow` if result exceeds safe integer range.
pub fn math_round(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::Int(*n)),
        Value::Float(f) => {
            let rounded = f.round();
            #[allow(clippy::cast_possible_truncation)]
            if rounded < MIN_SAFE_INT as f64 || rounded > MAX_SAFE_INT as f64 {
                return Err(VmFault::Overflow {
                    details: "math.round exceeds safe integer bounds".to_string(),
                });
            }
            #[allow(clippy::cast_possible_truncation)]
            check_safe_int(rounded as i64).map(Value::Int)
        }
        Value::Failure(f) => Ok(Value::Failure(Rc::clone(f))),
        other => Err(VmFault::TypeMismatch {
            expected: "Int or Float".to_string(),
            actual: other.type_name().to_string(),
        }),
    }
}

/// Truncates toward zero, returning an `Int` when representable.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not Int or Float,
/// or `VmFault::Overflow` if the result exceeds the `Int` range.
pub fn math_truncate(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    match &args[0] {
        Value::Int(n) => Ok(Value::Int(*n)),
        Value::Float(f) => {
            let truncated = f.trunc();
            #[allow(clippy::cast_possible_truncation)]
            if truncated < MIN_SAFE_INT as f64 || truncated > MAX_SAFE_INT as f64 {
                return Err(VmFault::Overflow {
                    details: "math.truncate exceeds safe integer bounds".to_string(),
                });
            }
            #[allow(clippy::cast_possible_truncation)]
            check_safe_int(truncated as i64).map(Value::Int)
        }
        Value::Failure(f) => Ok(Value::Failure(Rc::clone(f))),
        other => Err(VmFault::TypeMismatch {
            expected: "Int or Float".to_string(),
            actual: other.type_name().to_string(),
        }),
    }
}

/// Constrains a number to the inclusive `[min, max]` interval.
///
/// Mixed operands follow the canonical `Int -> Float` promotion. Inverted bounds
/// (`min > max`) produce a recoverable `Failure`; see
/// `docs/adp/ADP-001-byte-and-core-types-as-values.md` for the open question.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operands are not numbers, or
/// `VmFault::NonFiniteFloat` if a float operand is not finite.
pub fn math_clamp(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 3 {
        return Err(VmFault::TypeMismatch {
            expected: "3 arguments".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    if let Value::Failure(f) = &args[1] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    if let Value::Failure(f) = &args[2] {
        return Ok(Value::Failure(Rc::clone(f)));
    }

    if let (Value::Int(value), Value::Int(low), Value::Int(high)) = (&args[0], &args[1], &args[2]) {
        if low > high {
            return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                "math.clamp bounds are inverted: {low} > {high}"
            )))));
        }
        return Ok(Value::Int((*value).clamp(*low, *high)));
    }

    let value = to_float(&args[0])?;
    let low = to_float(&args[1])?;
    let high = to_float(&args[2])?;

    if low > high {
        return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
            "math.clamp bounds are inverted: {low} > {high}"
        )))));
    }

    check_finite_float(value.clamp(low, high)).map(Value::Float)
}

/// Converts an Aipo number (Int or Float) to `f64`.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not Int or Float,
/// or `VmFault::NonFiniteFloat` if float is non-finite.
pub fn to_float(val: &Value) -> Result<f64, VmFault> {
    match val {
        Value::Int(n) =>
        {
            #[allow(clippy::cast_precision_loss)]
            Ok(*n as f64)
        }
        Value::Float(f) => check_finite_float(*f),
        other => Err(VmFault::TypeMismatch {
            expected: "Int or Float".to_string(),
            actual: other.type_name().to_string(),
        }),
    }
}

/// Sine function (radians).
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not Int or Float.
pub fn math_sin(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    let x = to_float(&args[0])?;
    check_finite_float(x.sin()).map(Value::Float)
}

/// Cosine function (radians).
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not Int or Float.
pub fn math_cos(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    let x = to_float(&args[0])?;
    check_finite_float(x.cos()).map(Value::Float)
}

/// Tangent function (radians).
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not Int or Float.
pub fn math_tan(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    let x = to_float(&args[0])?;
    check_finite_float(x.tan()).map(Value::Float)
}

/// Arc sine function.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not Int or Float.
pub fn math_asin(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    let x = to_float(&args[0])?;
    if !(-1.0..=1.0).contains(&x) {
        return Ok(Value::Failure(Rc::new(FailureValue::new(
            "math.asin domain error: argument must be between -1.0 and 1.0".to_string(),
        ))));
    }
    check_finite_float(x.asin()).map(Value::Float)
}

/// Arc cosine function.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not Int or Float.
pub fn math_acos(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    let x = to_float(&args[0])?;
    if !(-1.0..=1.0).contains(&x) {
        return Ok(Value::Failure(Rc::new(FailureValue::new(
            "math.acos domain error: argument must be between -1.0 and 1.0".to_string(),
        ))));
    }
    check_finite_float(x.acos()).map(Value::Float)
}

/// Arc tangent function.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not Int or Float.
pub fn math_atan(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    let x = to_float(&args[0])?;
    check_finite_float(x.atan()).map(Value::Float)
}

/// Two-argument arc tangent function `atan2(y, x)`.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operands are not Int or Float.
pub fn math_atan2(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 2 {
        return Err(VmFault::TypeMismatch {
            expected: "2 arguments".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    if let Value::Failure(f) = &args[1] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    let y = to_float(&args[0])?;
    let x = to_float(&args[1])?;
    check_finite_float(y.atan2(x)).map(Value::Float)
}

/// Euclidean distance `hypot(x, y) = sqrt(x^2 + y^2)`.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operands are not Int or Float.
pub fn math_hypot(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 2 {
        return Err(VmFault::TypeMismatch {
            expected: "2 arguments".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    if let Value::Failure(f) = &args[1] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    let x = to_float(&args[0])?;
    let y = to_float(&args[1])?;
    check_finite_float(x.hypot(y)).map(Value::Float)
}

/// Natural logarithm (base e).
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not Int or Float.
pub fn math_log(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    let x = to_float(&args[0])?;
    if x <= 0.0 {
        return Ok(Value::Failure(Rc::new(FailureValue::new(
            "math.log domain error: argument must be positive".to_string(),
        ))));
    }
    check_finite_float(x.ln()).map(Value::Float)
}

/// Base-2 logarithm.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not Int or Float.
pub fn math_log2(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    let x = to_float(&args[0])?;
    if x <= 0.0 {
        return Ok(Value::Failure(Rc::new(FailureValue::new(
            "math.log2 domain error: argument must be positive".to_string(),
        ))));
    }
    check_finite_float(x.log2()).map(Value::Float)
}

/// Base-10 logarithm.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not Int or Float.
pub fn math_log10(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    let x = to_float(&args[0])?;
    if x <= 0.0 {
        return Ok(Value::Failure(Rc::new(FailureValue::new(
            "math.log10 domain error: argument must be positive".to_string(),
        ))));
    }
    check_finite_float(x.log10()).map(Value::Float)
}

/// Exponential function `e^x`.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not Int or Float.
pub fn math_exp(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    let x = to_float(&args[0])?;
    check_finite_float(x.exp()).map(Value::Float)
}

/// Signum function returning -1, 0, or 1 preserving type.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not Int or Float.
pub fn math_sign(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    match &args[0] {
        Value::Int(n) => {
            let s = if *n > 0 {
                1
            } else if *n < 0 {
                -1
            } else {
                0
            };
            Ok(Value::Int(s))
        }
        Value::Float(f) => {
            check_finite_float(*f)?;
            let s = if *f > 0.0 {
                1.0
            } else if *f < 0.0 {
                -1.0
            } else {
                0.0
            };
            Ok(Value::Float(s))
        }
        Value::Failure(f) => Ok(Value::Failure(Rc::clone(f))),
        other => Err(VmFault::TypeMismatch {
            expected: "Int or Float".to_string(),
            actual: other.type_name().to_string(),
        }),
    }
}

/// Degrees to radians conversion.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not Int or Float.
pub fn math_rad(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    let d = to_float(&args[0])?;
    check_finite_float(d.to_radians()).map(Value::Float)
}

/// Radians to degrees conversion.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not Int or Float.
pub fn math_deg(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    let r = to_float(&args[0])?;
    check_finite_float(r.to_degrees()).map(Value::Float)
}

/// Square root function.
///
/// Computes square root, returning recoverable `Value::Failure` on negative inputs.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not Int or Float.
pub fn math_sqrt(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    let val = match &args[0] {
        Value::Int(n) =>
        {
            #[allow(clippy::cast_precision_loss)]
            (*n as f64)
        }
        Value::Float(f) => *f,
        Value::Failure(f) => return Ok(Value::Failure(Rc::clone(f))),
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int or Float".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };

    if val < 0.0 {
        return Ok(Value::Failure(Rc::new(FailureValue::new(
            "cannot compute square root of negative number".to_string(),
        ))));
    }

    check_finite_float(val.sqrt()).map(Value::Float)
}

/// Power / exponentiation function.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operands are not numbers,
/// or `VmFault::Overflow` / `VmFault::NonFiniteFloat`.
pub fn math_pow(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 2 {
        return Err(VmFault::TypeMismatch {
            expected: "2 arguments".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    match (&args[0], &args[1]) {
        (Value::Int(base), Value::Int(exp)) if *exp >= 0 && *exp <= 53 => {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let exp_u32 = *exp as u32;
            if let Some(res) = base.checked_pow(exp_u32)
                && (MIN_SAFE_INT..=MAX_SAFE_INT).contains(&res)
            {
                return Ok(Value::Int(res));
            }
            #[allow(clippy::cast_precision_loss)]
            let f = (*base as f64).powf(*exp as f64);
            check_finite_float(f).map(Value::Float)
        }
        (Value::Int(base), Value::Int(exp)) => {
            #[allow(clippy::cast_precision_loss)]
            let f = (*base as f64).powf(*exp as f64);
            check_finite_float(f).map(Value::Float)
        }
        (Value::Float(base), Value::Float(exp)) => {
            check_finite_float(base.powf(*exp)).map(Value::Float)
        }
        (Value::Int(base), Value::Float(exp)) => {
            #[allow(clippy::cast_precision_loss)]
            let f = (*base as f64).powf(*exp);
            check_finite_float(f).map(Value::Float)
        }
        (Value::Float(base), Value::Int(exp)) => {
            #[allow(clippy::cast_precision_loss)]
            let f = base.powf(*exp as f64);
            check_finite_float(f).map(Value::Float)
        }
        (Value::Failure(f), _) | (_, Value::Failure(f)) => Ok(Value::Failure(Rc::clone(f))),
        (a, b) => Err(VmFault::TypeMismatch {
            expected: "two numbers (Int or Float)".to_string(),
            actual: format!("{} and {}", a.type_name(), b.type_name()),
        }),
    }
}

/// Constructs the canonical `math` module dictionary.
#[must_use]
pub fn create_module() -> Value {
    let entries = vec![
        (
            Value::String(Rc::new("abs".to_string())),
            Value::native("math.abs", 1, math_abs),
        ),
        (
            Value::String(Rc::new("min".to_string())),
            Value::native("math.min", 2, math_min),
        ),
        (
            Value::String(Rc::new("max".to_string())),
            Value::native("math.max", 2, math_max),
        ),
        (
            Value::String(Rc::new("floor".to_string())),
            Value::native("math.floor", 1, math_floor),
        ),
        (
            Value::String(Rc::new("ceil".to_string())),
            Value::native("math.ceil", 1, math_ceil),
        ),
        (
            Value::String(Rc::new("round".to_string())),
            Value::native("math.round", 1, math_round),
        ),
        (
            Value::String(Rc::new("sqrt".to_string())),
            Value::native("math.sqrt", 1, math_sqrt),
        ),
        (
            Value::String(Rc::new("pow".to_string())),
            Value::native("math.pow", 2, math_pow),
        ),
        (
            Value::String(Rc::new("truncate".to_string())),
            Value::native("math.truncate", 1, math_truncate),
        ),
        (
            Value::String(Rc::new("clamp".to_string())),
            Value::native("math.clamp", 3, math_clamp),
        ),
        (
            Value::String(Rc::new("sin".to_string())),
            Value::native("math.sin", 1, math_sin),
        ),
        (
            Value::String(Rc::new("cos".to_string())),
            Value::native("math.cos", 1, math_cos),
        ),
        (
            Value::String(Rc::new("tan".to_string())),
            Value::native("math.tan", 1, math_tan),
        ),
        (
            Value::String(Rc::new("asin".to_string())),
            Value::native("math.asin", 1, math_asin),
        ),
        (
            Value::String(Rc::new("acos".to_string())),
            Value::native("math.acos", 1, math_acos),
        ),
        (
            Value::String(Rc::new("atan".to_string())),
            Value::native("math.atan", 1, math_atan),
        ),
        (
            Value::String(Rc::new("atan2".to_string())),
            Value::native("math.atan2", 2, math_atan2),
        ),
        (
            Value::String(Rc::new("hypot".to_string())),
            Value::native("math.hypot", 2, math_hypot),
        ),
        (
            Value::String(Rc::new("log".to_string())),
            Value::native("math.log", 1, math_log),
        ),
        (
            Value::String(Rc::new("log2".to_string())),
            Value::native("math.log2", 1, math_log2),
        ),
        (
            Value::String(Rc::new("log10".to_string())),
            Value::native("math.log10", 1, math_log10),
        ),
        (
            Value::String(Rc::new("exp".to_string())),
            Value::native("math.exp", 1, math_exp),
        ),
        (
            Value::String(Rc::new("sign".to_string())),
            Value::native("math.sign", 1, math_sign),
        ),
        (
            Value::String(Rc::new("rad".to_string())),
            Value::native("math.rad", 1, math_rad),
        ),
        (
            Value::String(Rc::new("deg".to_string())),
            Value::native("math.deg", 1, math_deg),
        ),
        (
            Value::String(Rc::new("pi".to_string())),
            Value::Float(std::f64::consts::PI),
        ),
        (
            Value::String(Rc::new("e".to_string())),
            Value::Float(std::f64::consts::E),
        ),
        (
            Value::String(Rc::new("tau".to_string())),
            Value::Float(std::f64::consts::TAU),
        ),
    ];

    Value::Dict(Rc::new(RefCell::new(DictMap::from_entries(entries))))
}
