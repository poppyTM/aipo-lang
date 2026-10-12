//! Pure helpers shared by the interpreter: diagnostic naming, collection
//! identity, lengths, iteration items, sort keys, range normalization.
use crate::fault::VmFault;
use crate::value::{Value, check_safe_int};
use std::rc::Rc;

/// Names a value in a contract diagnostic.
///
/// [`Value::type_name`] reports the runtime kind, which is the right name for a core type but
/// hides which user type failed the contract, so a struct reports the name it was declared
/// with.
pub(crate) fn value_diagnostic_name(value: &Value) -> String {
    match value {
        Value::Struct(instance) => instance.borrow().type_name.clone(),
        other => other.type_name().to_string(),
    }
}

/// Stable identity of a managed collection, used to detect structural mutation during
/// iteration without borrowing the collection itself.
pub(crate) fn collection_identity(value: &Value) -> Option<usize> {
    match value {
        Value::List(list) => Some(Rc::as_ptr(list) as usize),
        Value::Dict(dict) => Some(Rc::as_ptr(dict) as usize),
        Value::Bytes(bytes) => Some(Rc::as_ptr(bytes) as usize),
        Value::Set(items) => Some(Rc::as_ptr(items) as usize),
        _ => None,
    }
}

/// Length of a measurable value, following the canonical `len` semantics.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` for values without a length.
pub(crate) fn length_of(value: &Value) -> Result<Value, VmFault> {
    let len = match value {
        Value::String(text) => text.chars().count(),
        Value::List(list) => list.borrow().len(),
        Value::Dict(dict) => dict.borrow().len(),
        Value::Bytes(bytes) => bytes.borrow().len(),
        Value::Set(items) => items.borrow().len(),
        Value::Range(r) => usize::try_from((r.end - r.start).max(0)).unwrap_or(0),
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "String, List, Dict, Bytes, Set, or Range".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    #[allow(clippy::cast_possible_wrap)]
    check_safe_int(len as i64).map(Value::Int)
}

/// Elements a higher-order collection method applies its callable to.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` when the receiver is not iterable.
pub(crate) fn iterable_items(receiver: &Value) -> Result<Vec<Value>, VmFault> {
    match receiver {
        Value::List(list) => Ok(list.borrow().to_vec()),
        Value::Dict(dict) => Ok(dict.borrow().values()),
        Value::Set(items) => Ok(items.borrow().to_vec()),
        Value::String(text) => Ok(text
            .chars()
            .map(|ch| Value::String(Rc::new(ch.to_string())))
            .collect()),
        Value::Range(r) => Ok((r.start..r.end).map(Value::Int).collect()),
        other => Err(VmFault::TypeMismatch {
            expected: "iterable List, Dict, Set, String, or Range".to_string(),
            actual: other.type_name().to_string(),
        }),
    }
}

/// Orders two sort keys, returning `None` when they are not comparable.
pub(crate) fn compare_keys(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    match (a.numeric_key(), b.numeric_key()) {
        (Some(x), Some(y)) => x.partial_cmp(&y),
        _ => match (a, b) {
            (Value::String(x), Value::String(y)) => Some(x.cmp(y)),
            (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
            _ => None,
        },
    }
}

/// Normalizes a half-open slice range, resolving negative bounds from the end and
/// clamping to the collection bounds.
pub(crate) fn normalize_range(start: i64, end: i64, len: usize) -> (usize, usize) {
    #[allow(clippy::cast_possible_wrap)]
    let len_i = len as i64;
    let resolve = |idx: i64| -> usize {
        let bound = if idx < 0 { len_i + idx } else { idx };
        usize::try_from(bound.clamp(0, len_i)).unwrap_or(0)
    };
    let from = resolve(start);
    let to = resolve(end);
    if from >= to { (from, from) } else { (from, to) }
}
