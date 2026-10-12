//! Canonical core-type conversions for the Aipo runtime.
//!
//! This module is the single source of truth for the explicit conversion forms
//! `Int(value)`, `Float(value)`, `Byte(value)` and `String(value)` that canon defines
//! (`Aipo V1 — Language Reference`, `Aipo Language — Especificação Viva`), plus the
//! `Bytes(count)`, `Set(list)` and `Duration(seconds)` constructions. `aipo-stdlib`
//! delegates to it so the VM and the Prelude can never disagree.
//!
//! Recovery follows the two-channel rule: domain problems (invalid text, out-of-range
//! values, non-representable conversions) produce a recoverable [`Value::Failure`],
//! while unsupported operand categories are a [`VmFault::TypeMismatch`].

use crate::fault::VmFault;
use crate::value::{FailureValue, Value, check_finite_float, check_safe_int};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::rc::Rc;
#[cfg(feature = "unicode")]
use unicode_normalization::UnicodeNormalization;

/// Inclusive upper bound of the Aipo `Byte` value range.
pub const BYTE_MAX: i64 = 255;

/// Largest block a single `Bytes(count)` construction may allocate.
///
/// Canon fixes the construction form (`Bytes(32)` allocates a mutable zero-filled block) but
/// not an upper bound. The provisional cap keeps a source-level size from exhausting memory;
/// a larger request is a recoverable `Failure`. Recorded in
/// `docs/adp/ADP-002-construction-hooks-and-runtime-contracts.md` (gap G4).
pub const BYTES_MAX_ALLOCATION: i64 = 64 * 1024 * 1024;

/// Identity of a fundamental Aipo type, used by type values and runtime `is` tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TypeTag {
    /// `none`
    None,
    /// `Bool`
    Bool,
    /// `Int`
    Int,
    /// `Float`
    Float,
    /// `Byte`
    Byte,
    /// `String`
    String,
    /// `List`
    List,
    /// `Dict`
    Dict,
    /// `Bytes`
    Bytes,
    /// `Range`
    Range,
    /// `Set`
    Set,
    /// `Sequence`
    Sequence,
    /// `Task`
    Task,
    /// `Duration`
    Duration,
}

impl TypeTag {
    /// Returns the canonical source-level name of the type.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Bool => "Bool",
            Self::Int => "Int",
            Self::Float => "Float",
            Self::Byte => "Byte",
            Self::String => "String",
            Self::List => "List",
            Self::Dict => "Dict",
            Self::Bytes => "Bytes",
            Self::Range => "Range",
            Self::Set => "Set",
            Self::Sequence => "Sequence",
            Self::Task => "Task",
            Self::Duration => "Duration",
        }
    }

    /// Resolves the canonical source-level name of a core type back to its tag.
    ///
    /// Signature contracts are written with these names (`name: Int`), so a runtime check
    /// needs the reverse of [`TypeTag::name`]. Names that are not core types (a struct, an
    /// interface, `Function`) return `None` and are checked by the VM against the module's
    /// declared types instead.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "none" => Some(Self::None),
            "Bool" => Some(Self::Bool),
            "Int" => Some(Self::Int),
            "Float" => Some(Self::Float),
            "Byte" => Some(Self::Byte),
            "String" => Some(Self::String),
            "List" => Some(Self::List),
            "Dict" => Some(Self::Dict),
            "Bytes" => Some(Self::Bytes),
            "Range" => Some(Self::Range),
            "Set" => Some(Self::Set),
            "Sequence" => Some(Self::Sequence),
            "Task" => Some(Self::Task),
            "Duration" => Some(Self::Duration),
            _ => None,
        }
    }

    /// Returns whether `value` is an instance of this type.
    #[must_use]
    pub fn matches(self, value: &Value) -> bool {
        matches!(
            (self, value),
            (Self::None, Value::None)
                | (Self::Bool, Value::Bool(_))
                | (Self::Int, Value::Int(_))
                | (Self::Float, Value::Float(_))
                | (Self::Byte, Value::Byte(_))
                | (Self::String, Value::String(_))
                | (Self::List, Value::List(_))
                | (Self::Dict, Value::Dict(_))
                | (Self::Bytes, Value::Bytes(_))
                | (Self::Range, Value::Range(_))
                | (Self::Set, Value::Set(_))
                | (Self::Sequence, Value::Sequence(_))
                | (Self::Task, Value::Task(_))
                | (Self::Duration, Value::Duration(_))
        )
    }
}

fn recoverable(message: impl Into<String>) -> Value {
    Value::Failure(Rc::new(FailureValue::new(message.into())))
}

fn type_error(expected: &str, actual: &Value) -> VmFault {
    VmFault::TypeMismatch {
        expected: expected.to_string(),
        actual: actual.type_name().to_string(),
    }
}

/// Explicit `Bytes(count)` construction.
///
/// Canon defines `Bytes` as a managed, mutable binary block separate from `String`, built with
/// `Bytes(n)` into `n` zero-filled bytes and indexed by byte. Only `Int` is accepted; a
/// negative or oversized count is a recoverable `Failure` because it is a domain problem, not
/// a category error.
///
/// # Errors
/// Returns [`VmFault::TypeMismatch`] when the argument is not an `Int`.
pub fn convert_bytes(value: &Value) -> Result<Value, VmFault> {
    let Value::Int(count) = value else {
        return Err(type_error("Int for Bytes(count)", value));
    };
    if *count < 0 || *count > BYTES_MAX_ALLOCATION {
        return Ok(recoverable(format!(
            "Bytes({count}) is outside the constructible range 0..={BYTES_MAX_ALLOCATION}"
        )));
    }
    Ok(Value::Bytes(Rc::new(RefCell::new(
        (vec![0u8; *count as usize]).into(),
    ))))
}

/// Explicit conversion to `Set`: deduplicates a `List`, keeping first-occurrence
/// order (canon: `Set` preserves insertion order).
///
/// # Errors
/// Returns [`VmFault::TypeMismatch`] for non-list and non-set values.
pub fn convert_set(value: &Value) -> Result<Value, VmFault> {
    match value {
        Value::List(items) => {
            let mut unique: Vec<Value> = Vec::new();
            for item in items.borrow().iter() {
                if !unique.iter().any(|seen| seen == item) {
                    unique.push(item.clone());
                }
            }
            Ok(Value::Set(Rc::new(RefCell::new((unique).into()))))
        }
        Value::Set(items) => Ok(Value::Set(Rc::new(RefCell::new(items.borrow().clone())))),
        Value::Failure(f) => Ok(Value::Failure(Rc::clone(f))),
        other => Err(type_error("List or Set for Set(collection)", other)),
    }
}

/// Explicit conversion to `Duration`: a finite number of seconds.
///
/// # Errors
/// Returns a recoverable `Failure` for non-finite floats; [`VmFault::TypeMismatch`]
/// for categories without a defined conversion.
pub fn convert_duration(value: &Value) -> Result<Value, VmFault> {
    match value {
        #[allow(clippy::cast_precision_loss)]
        Value::Int(n) => Ok(Value::Duration(*n as f64)),
        Value::Byte(b) => Ok(Value::Duration(f64::from(*b))),
        Value::Float(f) => {
            if !f.is_finite() {
                return Ok(recoverable("Duration cannot represent a non-finite Float"));
            }
            Ok(Value::Duration(*f))
        }
        Value::Failure(f) => Ok(Value::Failure(Rc::clone(f))),
        other => Err(type_error(
            "Int or Float seconds for Duration(seconds)",
            other,
        )),
    }
}

/// Explicit conversion to `Int`.
///
/// `Float` truncates toward zero; `Byte` widens; `String` must parse as a whole
/// (surrounding whitespace is not skipped — use `string.trim` first). Out-of-range or
/// invalid text produces a recoverable `Failure`.
///
/// # Errors
/// Returns [`VmFault::TypeMismatch`] for categories without a defined conversion.
pub fn convert_int(value: &Value) -> Result<Value, VmFault> {
    match value {
        Value::Int(n) => Ok(Value::Int(*n)),
        Value::Byte(b) => Ok(Value::Int(i64::from(*b))),
        Value::Float(f) => {
            if !f.is_finite() {
                return Ok(recoverable("Int cannot represent a non-finite Float"));
            }
            let truncated = f.trunc();
            #[allow(clippy::cast_possible_truncation)]
            let as_int = truncated as i64;
            check_safe_int(as_int)
                .map(Value::Int)
                .or_else(|_| Ok(recoverable(format!("Float {f} is outside the Int range"))))
        }
        Value::String(s) => match s.parse::<i64>() {
            Ok(parsed) => check_safe_int(parsed)
                .map(Value::Int)
                .or_else(|_| Ok(recoverable(format!("integer text out of range: {s:?}")))),
            Err(_) => Ok(recoverable(format!("invalid integer text: {s:?}"))),
        },
        Value::Failure(f) => Ok(Value::Failure(Rc::clone(f))),
        other => Err(type_error("String, Int, Float, or Byte", other)),
    }
}

/// Explicit conversion to `Float`.
///
/// `Int`/`Byte` widen; `String` must parse. Invalid text or a non-finite result
/// produces a recoverable `Failure`.
///
/// # Errors
/// Returns [`VmFault::TypeMismatch`] for categories without a defined conversion.
pub fn convert_float(value: &Value) -> Result<Value, VmFault> {
    match value {
        Value::Float(f) => Ok(Value::Float(*f)),
        Value::Int(n) =>
        {
            #[allow(clippy::cast_precision_loss)]
            Ok(Value::Float(*n as f64))
        }
        Value::Byte(b) => Ok(Value::Float(f64::from(*b))),
        Value::String(s) => match s.parse::<f64>() {
            Ok(parsed) => check_finite_float(parsed)
                .map(Value::Float)
                .or_else(|_| Ok(recoverable(format!("non-finite float text: {s:?}")))),
            Err(_) => Ok(recoverable(format!("invalid float text: {s:?}"))),
        },
        Value::Failure(f) => Ok(Value::Failure(Rc::clone(f))),
        other => Err(type_error("String, Int, Float, or Byte", other)),
    }
}

/// Explicit, verified conversion to `Byte`.
///
/// Accepts `Int`, integral `Float` and `String` inside `0..=255`. Values outside the
/// range never wrap around and never saturate: they produce a recoverable `Failure`.
///
/// # Errors
/// Returns [`VmFault::TypeMismatch`] for categories without a defined conversion.
pub fn convert_byte(value: &Value) -> Result<Value, VmFault> {
    let candidate: i64 = match value {
        Value::Byte(b) => return Ok(Value::Byte(*b)),
        Value::Int(n) => *n,
        Value::Float(f) => {
            if !f.is_finite() || f.fract() != 0.0 {
                return Ok(recoverable(format!(
                    "Byte requires an integral value in 0..={BYTE_MAX}, got {f}"
                )));
            }
            #[allow(clippy::cast_possible_truncation)]
            let as_int = f.trunc() as i64;
            as_int
        }
        Value::String(s) => match s.parse::<i64>() {
            Ok(parsed) => parsed,
            Err(_) => return Ok(recoverable(format!("invalid Byte text: {s:?}"))),
        },
        Value::Failure(f) => return Ok(Value::Failure(Rc::clone(f))),
        other => return Err(type_error("String, Int, Float, or Byte", other)),
    };

    if (0..=BYTE_MAX).contains(&candidate) {
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Ok(Value::Byte(candidate as u8))
    } else {
        Ok(recoverable(format!(
            "Byte value {candidate} is outside the range 0..={BYTE_MAX}"
        )))
    }
}

/// Explicit textual conversion to `String`.
///
/// Defined for `String`, `Int`, `Float`, `Byte`, `Bool`, `none` and type values,
/// reusing the VM's canonical textual rendering so `String(value)` agrees with
/// `io.print(value)`.
///
/// # Errors
/// Returns [`VmFault::TypeMismatch`] for collections, structs and functions: canon
/// forbids magic stringification outside the fundamental values.
pub fn convert_string(value: &Value) -> Result<Value, VmFault> {
    match value {
        Value::String(_)
        | Value::Int(_)
        | Value::Float(_)
        | Value::Byte(_)
        | Value::Bool(_)
        | Value::None
        | Value::Type(_)
        | Value::UserType(_) => {
            // Canon makes NFC an invariant of `String`, so an explicit conversion is a
            // construction boundary and its result is normalized before the program sees it.
            #[cfg(feature = "unicode")]
            let text: String = value.to_string().nfc().collect();
            #[cfg(not(feature = "unicode"))]
            let text: String = value.to_string();
            Ok(Value::String(Rc::new(text)))
        }
        Value::Failure(f) => Ok(Value::Failure(Rc::clone(f))),
        other => Err(type_error("String, Int, Float, Byte, Bool, or none", other)),
    }
}

/// Dispatches a call on a core type value to its canonical conversion.
///
/// Types without a conversion form (`List`, `Dict`, `Range`, `Bool`, `none`) are explicit
/// arity/type errors rather than silently invented construction.
///
/// # Errors
/// Returns [`VmFault::TypeMismatch`] when the type is not callable or the arity is wrong.
pub fn convert_via_type(tag: TypeTag, args: &[Value]) -> Result<Value, VmFault> {
    if tag == TypeTag::Set && args.is_empty() {
        return Ok(Value::Set(Rc::new(RefCell::new(Vec::new().into()))));
    }
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: if tag == TypeTag::Set {
                "0 or 1 arguments for Set".to_string()
            } else {
                format!("1 argument for {}", tag.name())
            },
            actual: format!("{} arguments", args.len()),
        });
    }
    match tag {
        TypeTag::Int => convert_int(&args[0]),
        TypeTag::Float => convert_float(&args[0]),
        TypeTag::Byte => convert_byte(&args[0]),
        TypeTag::String => convert_string(&args[0]),
        TypeTag::Bytes => convert_bytes(&args[0]),
        TypeTag::Set => convert_set(&args[0]),
        TypeTag::Duration => convert_duration(&args[0]),
        other => Err(VmFault::NotCallable {
            type_name: format!("{} (no conversion form in V1)", other.name()),
        }),
    }
}
