//! Canonical core-type conversions for the Aipo Prelude V1.
//!
//! This module is a thin, arity-checking adapter: the conversion *semantics* live in
//! `aipo_vm::convert`, which is the single source of truth shared with the VM's call
//! path for type values. Keeping one implementation means `Int(value)` cannot behave
//! differently when called through the Prelude global and when called through a
//! first-class type value.
//!
//! - `Int(value)`: truncates `Float` toward zero, widens `Byte`, parses `String`.
//! - `Float(value)`: widens `Int`/`Byte`, parses `String`.
//! - `Byte(value)`: verified range `0..=255`; no wraparound and no silent saturation.
//! - `String(value)`: canonical textual conversion for the fundamental values.
//! - `Bytes(count)`: zero-filled managed byte block of `count` bytes.
//!
//! Every conversion is total: domain problems return a recoverable `Value::Failure`
//! (Model B), unsupported operand categories are a `VmFault::TypeMismatch`.

use aipo_vm::convert as vm_convert;
use aipo_vm::{Value, VmFault};

fn single_argument<'a>(op: &str, args: &'a [Value]) -> Result<&'a Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: format!("1 argument for {op}"),
            actual: format!("{} arguments", args.len()),
        });
    }
    Ok(&args[0])
}

/// Explicit conversion to `Int`.
///
/// # Errors
/// Returns [`VmFault::TypeMismatch`] for categories without a defined conversion.
pub fn convert_int(args: &[Value]) -> Result<Value, VmFault> {
    vm_convert::convert_int(single_argument("Int", args)?)
}

/// Explicit conversion to `Float`.
///
/// # Errors
/// Returns [`VmFault::TypeMismatch`] for categories without a defined conversion.
pub fn convert_float(args: &[Value]) -> Result<Value, VmFault> {
    vm_convert::convert_float(single_argument("Float", args)?)
}

/// Explicit, verified conversion to `Byte`.
///
/// # Errors
/// Returns [`VmFault::TypeMismatch`] for categories without a defined conversion.
pub fn convert_byte(args: &[Value]) -> Result<Value, VmFault> {
    vm_convert::convert_byte(single_argument("Byte", args)?)
}

/// Explicit textual conversion to `String`.
///
/// # Errors
/// Returns [`VmFault::TypeMismatch`] for collections, structs and functions: canon
/// forbids magic stringification outside the fundamental values.
pub fn convert_string(args: &[Value]) -> Result<Value, VmFault> {
    vm_convert::convert_string(single_argument("String", args)?)
}

/// Explicit `Bytes(count)` construction of a zero-filled managed byte block.
///
/// # Errors
/// Returns [`VmFault::TypeMismatch`] when the argument is not an `Int`.
pub fn convert_bytes(args: &[Value]) -> Result<Value, VmFault> {
    vm_convert::convert_bytes(single_argument("Bytes", args)?)
}

/// Explicit conversion to `Set` from a list or existing set, or an empty set when 0 arguments are given.
///
/// # Errors
/// Returns [`VmFault::TypeMismatch`] when the argument is not a `List` or `Set`, or more than 1 argument is given.
pub fn convert_set(args: &[Value]) -> Result<Value, VmFault> {
    match args.len() {
        0 => Ok(Value::Set(std::rc::Rc::new(std::cell::RefCell::new(
            Vec::new().into(),
        )))),
        1 => vm_convert::convert_set(&args[0]),
        n => Err(VmFault::TypeMismatch {
            expected: "0 or 1 arguments for Set".to_string(),
            actual: format!("{n} arguments"),
        }),
    }
}

/// Explicit conversion to `Duration` from Int or Float seconds.
///
/// # Errors
/// Returns [`VmFault::TypeMismatch`] when the argument is not numeric or non-finite.
pub fn convert_duration(args: &[Value]) -> Result<Value, VmFault> {
    vm_convert::convert_duration(single_argument("Duration", args)?)
}
