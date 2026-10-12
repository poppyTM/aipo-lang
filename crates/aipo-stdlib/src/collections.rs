//! Canonical `List` and `Dict` core APIs.
//!
//! Every operation here is a *receiver-first* native method (`fn(&Value, &[Value])`), so
//! it is reachable both as a module-style call (`list.add(x)`) and, where canon defines
//! it, through dot-call sugar.
//!
//! Mutating operations (`add`, `insert`, `remove`, `remove_at`, `remove_last`, `clear`)
//! change the receiver's identity in place, as canon requires; the VM rejects them with
//! `MutationDuringIteration` while that same collection is under an active `each` or
//! higher-order iteration.
//!
//! `filter`, `transform` and `sort_by` are **not** here: they need to call back into Aipo
//! code, so the VM executes them (see `MethodKind::HigherOrder`).
//!
//! Deliberately absent from `Dict`: an ambiguous `get`/`find` that could not distinguish
//! "key absent" from "value stored as `none`". Canon states that rationale explicitly, so
//! `has(key)` plus indexing (`d["key"]`, which faults on a missing key) are the canonical
//! forms. See `docs/adp/ADP-001-bytecode-and-core-types-as-values.md`.

use aipo_vm::{DictMap, Value, Vm, VmFault};
use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;

/// Extracts a shared list receiver, or a type fault.
fn expect_list(receiver: &Value) -> Result<Vec<Value>, VmFault> {
    match receiver {
        Value::List(list) => Ok(list.borrow().to_vec()),
        other => Err(VmFault::TypeMismatch {
            expected: "List receiver".to_string(),
            actual: other.type_name().to_string(),
        }),
    }
}

fn expect_dict(receiver: &Value) -> Result<std::cell::Ref<'_, DictMap>, VmFault> {
    match receiver {
        Value::Dict(dict) => Ok(dict.borrow()),
        other => Err(VmFault::TypeMismatch {
            expected: "Dict receiver".to_string(),
            actual: other.type_name().to_string(),
        }),
    }
}

fn require_arity(args: &[Value], expected: usize, op: &str) -> Result<(), VmFault> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(VmFault::TypeMismatch {
            expected: format!("{expected} argument(s) for {op}"),
            actual: format!("{} arguments", args.len()),
        })
    }
}

/// Resolves a possibly negative index against a length, faulting when out of range.
fn resolve_index(index: i64, len: usize) -> Result<usize, VmFault> {
    #[allow(clippy::cast_possible_wrap)]
    let len_i = len as i64;
    let actual = if index < 0 { len_i + index } else { index };
    let resolved = usize::try_from(actual).map_err(|_| VmFault::IndexOutOfRange { index, len })?;
    if resolved > len {
        return Err(VmFault::IndexOutOfRange { index, len });
    }
    Ok(resolved)
}

/// Natural order used by `sort`, covering every orderable fundamental value.
fn natural_order(a: &Value, b: &Value) -> Option<Ordering> {
    match (a, b) {
        (Value::None, Value::None) => Some(Ordering::Equal),
        (Value::Bool(x), Value::Bool(y)) => Some(x.cmp(y)),
        (Value::String(x), Value::String(y)) => Some(x.cmp(y)),
        (Value::Int(_), Value::Int(_))
        | (Value::Float(_), Value::Float(_))
        | (Value::Int(_), Value::Float(_))
        | (Value::Float(_), Value::Int(_))
        | (Value::Byte(_), Value::Byte(_))
        | (Value::Byte(_), Value::Int(_))
        | (Value::Int(_), Value::Byte(_)) => {
            let left = numeric(a)?;
            let right = numeric(b)?;
            left.partial_cmp(&right)
        }
        _ => None,
    }
}

fn numeric(value: &Value) -> Option<f64> {
    match value {
        Value::Int(n) =>
        {
            #[allow(clippy::cast_precision_loss)]
            Some(*n as f64)
        }
        Value::Byte(b) => Some(f64::from(*b)),
        Value::Float(f) => Some(*f),
        _ => None,
    }
}

/// `list.add(value)` — appends in place, preserving the list identity.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a List.
pub fn list_add(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "list.add")?;
    let Value::List(list) = receiver else {
        return Err(VmFault::TypeMismatch {
            expected: "List receiver".to_string(),
            actual: receiver.type_name().to_string(),
        });
    };
    list.borrow_mut().push(args[0].clone());
    Ok(Value::None)
}

/// `list.insert(index, value)` — inserts in place; `index == len` appends.
///
/// # Errors
/// Returns `VmFault::IndexOutOfRange` for indices beyond the list length.
pub fn list_insert(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "list.insert")?;
    let items = expect_list(receiver)?;
    let index = match &args[0] {
        Value::Int(n) => *n,
        Value::Byte(b) => i64::from(*b),
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int index".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let at = resolve_index(index, items.len())?;
    if let Value::List(list) = receiver {
        list.borrow_mut().insert(at, args[1].clone());
    }
    Ok(Value::None)
}

/// `list.remove(value)` — removes the first equal element, reporting whether it existed.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a List.
pub fn list_remove(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "list.remove")?;
    let Value::List(list) = receiver else {
        return Err(VmFault::TypeMismatch {
            expected: "List receiver".to_string(),
            actual: receiver.type_name().to_string(),
        });
    };
    let mut items = list.borrow_mut();
    if let Some(pos) = items.iter().position(|item| *item == args[0]) {
        items.remove(pos);
        Ok(Value::Bool(true))
    } else {
        Ok(Value::Bool(false))
    }
}

/// `list.remove_at(index)` — removes and returns the element at `index`.
///
/// # Errors
/// Returns `VmFault::IndexOutOfRange` when the index is out of range.
pub fn list_remove_at(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "list.remove_at")?;
    let items = expect_list(receiver)?;
    let index = match &args[0] {
        Value::Int(n) => *n,
        Value::Byte(b) => i64::from(*b),
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int index".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    if items.is_empty() {
        return Err(VmFault::IndexOutOfRange { index, len: 0 });
    }
    let at = resolve_index(index, items.len())?;
    if at >= items.len() {
        return Err(VmFault::IndexOutOfRange {
            index,
            len: items.len(),
        });
    }
    if let Value::List(list) = receiver {
        return Ok(list.borrow_mut().remove(at));
    }
    Ok(Value::None)
}

/// `list.remove_last()` — removes and returns the final element.
///
/// # Errors
/// Returns `VmFault::IndexOutOfRange` for an empty list.
pub fn list_remove_last(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "list.remove_last")?;
    let Value::List(list) = receiver else {
        return Err(VmFault::TypeMismatch {
            expected: "List receiver".to_string(),
            actual: receiver.type_name().to_string(),
        });
    };
    let mut items = list.borrow_mut();
    items
        .pop()
        .ok_or(VmFault::IndexOutOfRange { index: -1, len: 0 })
}

/// `list.clear()` — removes every element in place.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a List.
pub fn list_clear(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "list.clear")?;
    let Value::List(list) = receiver else {
        return Err(VmFault::TypeMismatch {
            expected: "List receiver".to_string(),
            actual: receiver.type_name().to_string(),
        });
    };
    list.borrow_mut().clear();
    Ok(Value::None)
}

/// `list.contains(value)` — membership using canonical `==`.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a List.
pub fn list_contains(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "list.contains")?;
    let items = expect_list(receiver)?;
    Ok(Value::Bool(items.iter().any(|item| *item == args[0])))
}

/// `list.find(value)` — index of the first match as `Int`, or `none`.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a List.
pub fn list_find(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "list.find")?;
    let items = expect_list(receiver)?;
    match items.iter().position(|item| *item == args[0]) {
        #[allow(clippy::cast_possible_wrap)]
        Some(index) => Ok(Value::Int(index as i64)),
        None => Ok(Value::None),
    }
}

/// `list.count(value)` — number of occurrences using `==`.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a List.
pub fn list_count(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "list.count")?;
    let items = expect_list(receiver)?;
    let count = items.iter().filter(|item| **item == args[0]).count();
    #[allow(clippy::cast_possible_wrap)]
    Ok(Value::Int(count as i64))
}

/// `list.first()` — first element; strict, faults on an empty list.
///
/// # Errors
/// Returns `VmFault::IndexOutOfRange` for an empty list.
pub fn list_first(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "list.first")?;
    let items = expect_list(receiver)?;
    items
        .first()
        .cloned()
        .ok_or(VmFault::IndexOutOfRange { index: 0, len: 0 })
}

/// `list.first_or(default)` — first element, or `default` if the list is empty.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a List or arity != 1.
pub fn list_first_or(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "list.first_or")?;
    let items = expect_list(receiver)?;
    Ok(items.first().cloned().unwrap_or_else(|| args[0].clone()))
}

/// `list.last()` — last element; strict, faults on an empty list.
///
/// # Errors
/// Returns `VmFault::IndexOutOfRange` for an empty list.
pub fn list_last(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "list.last")?;
    let items = expect_list(receiver)?;
    let len = items.len();
    items
        .last()
        .cloned()
        .ok_or(VmFault::IndexOutOfRange { index: -1, len })
}

/// `list.last_or(default)` — last element, or `default` if the list is empty.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a List or arity != 1.
pub fn list_last_or(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "list.last_or")?;
    let items = expect_list(receiver)?;
    Ok(items.last().cloned().unwrap_or_else(|| args[0].clone()))
}

/// `list.find_index(value)` — index of the first match as `Int`, or `none`.
///
/// Explicit alias for `list.find(value)` per canon.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a List.
pub fn list_find_index(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    list_find(receiver, args)
}

/// `list.is_empty()` — canonical emptiness check.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a List.
pub fn list_is_empty(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "list.is_empty")?;
    Ok(Value::Bool(expect_list(receiver)?.is_empty()))
}

/// `list.reverse()` — returns a new, reversed, shallow-copied List.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a List.
pub fn list_reverse(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "list.reverse")?;
    let mut items = expect_list(receiver)?;
    items.reverse();
    Ok(Value::List(Rc::new(RefCell::new((items).into()))))
}

/// `list.sort()` — returns a new List in natural order; stable.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` when elements are not mutually orderable.
pub fn list_sort(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "list.sort")?;
    let mut items = expect_list(receiver)?;
    let mut failure = None;
    items.sort_by(|a, b| match natural_order(a, b) {
        Some(ordering) => ordering,
        None => {
            failure = Some(VmFault::TypeMismatch {
                expected: "elements with a natural order".to_string(),
                actual: format!("{} and {}", a.type_name(), b.type_name()),
            });
            Ordering::Equal
        }
    });
    match failure {
        Some(fault) => Err(fault),
        None => Ok(Value::List(Rc::new(RefCell::new((items).into())))),
    }
}

/// `list.len()` — element count.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a List.
pub fn list_len(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "list.len")?;
    #[allow(clippy::cast_possible_wrap)]
    Ok(Value::Int(expect_list(receiver)?.len() as i64))
}

/// `list.take(n)` — returns a new List with the first `n` elements.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not an Int.
pub fn list_take(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "list.take")?;
    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    let items = expect_list(receiver)?;
    let n = match &args[0] {
        Value::Int(i) => *i,
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for list.take".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let count = n.max(0) as usize;
    let taken: Vec<Value> = items.into_iter().take(count).collect();
    Ok(Value::List(Rc::new(RefCell::new((taken).into()))))
}

/// `list.skip(n)` — returns a new List skipping the first `n` elements.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not an Int.
pub fn list_skip(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "list.skip")?;
    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    let items = expect_list(receiver)?;
    let n = match &args[0] {
        Value::Int(i) => *i,
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for list.skip".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let count = n.max(0) as usize;
    let skipped: Vec<Value> = items.into_iter().skip(count).collect();
    Ok(Value::List(Rc::new(RefCell::new((skipped).into()))))
}

/// `list.distinct()` — returns a new List with duplicate elements removed, preserving first-occurrence order.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if receiver is not a List.
pub fn list_distinct(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "list.distinct")?;
    let items = expect_list(receiver)?;
    let mut unique = Vec::new();
    for item in items {
        if !unique.iter().any(|seen| seen == &item) {
            unique.push(item);
        }
    }
    Ok(Value::List(Rc::new(RefCell::new((unique).into()))))
}

/// `list.zip(other)` — pairs elements of `list` and `other` into 2-element lists until the shorter ends.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if argument is not a List.
pub fn list_zip(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "list.zip")?;
    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    let items = expect_list(receiver)?;
    let other = expect_list(&args[0])?;
    let paired: Vec<Value> = items
        .into_iter()
        .zip(other)
        .map(|(a, b)| Value::List(Rc::new(RefCell::new((vec![a, b]).into()))))
        .collect();
    Ok(Value::List(Rc::new(RefCell::new((paired).into()))))
}

/// `list.chain(other)` — concatenates elements of `other` after `list`.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if argument is not a List.
pub fn list_chain(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "list.chain")?;
    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    let mut items = expect_list(receiver)?;
    let other = expect_list(&args[0])?;
    items.extend(other);
    Ok(Value::List(Rc::new(RefCell::new((items).into()))))
}

/// `list.chunk(size)` — divides list into non-overlapping lists of length `size`.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if size is not a positive Int.
pub fn list_chunk(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "list.chunk")?;
    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    let items = expect_list(receiver)?;
    let n = match &args[0] {
        Value::Int(i) => *i,
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for list.chunk".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    if n <= 0 {
        return Err(VmFault::TypeMismatch {
            expected: "positive Int for list.chunk size".to_string(),
            actual: n.to_string(),
        });
    }
    let size = n as usize;
    let chunks: Vec<Value> = items
        .chunks(size)
        .map(|c| Value::List(Rc::new(RefCell::new((c.to_vec()).into()))))
        .collect();
    Ok(Value::List(Rc::new(RefCell::new((chunks).into()))))
}

/// `list.window(size)` — sliding windows of length `size` as lists.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if size is not a positive Int.
pub fn list_window(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "list.window")?;
    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    let items = expect_list(receiver)?;
    let n = match &args[0] {
        Value::Int(i) => *i,
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for list.window".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    if n <= 0 {
        return Err(VmFault::TypeMismatch {
            expected: "positive Int for list.window size".to_string(),
            actual: n.to_string(),
        });
    }
    let size = n as usize;
    if size > items.len() {
        return Ok(Value::List(Rc::new(RefCell::new((Vec::new()).into()))));
    }
    let windows: Vec<Value> = items
        .windows(size)
        .map(|w| Value::List(Rc::new(RefCell::new((w.to_vec()).into()))))
        .collect();
    Ok(Value::List(Rc::new(RefCell::new((windows).into()))))
}

/// `list.enumerate()` — returns a List of `[index, value]` pairs as 2-element lists.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if receiver is not a List.
pub fn list_enumerate(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "list.enumerate")?;
    let items = expect_list(receiver)?;
    let mut indexed = Vec::with_capacity(items.len());
    for (i, item) in items.into_iter().enumerate() {
        #[allow(clippy::cast_possible_wrap)]
        let pos = aipo_vm::check_safe_int(i as i64).map(Value::Int)?;
        indexed.push(Value::List(Rc::new(RefCell::new((vec![pos, item]).into()))));
    }
    Ok(Value::List(Rc::new(RefCell::new((indexed).into()))))
}

/// `dict.has(key)` — presence check that cannot be confused with a stored `none`.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a Dict.
pub fn dict_has(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "dict.has")?;
    let entries = expect_dict(receiver)?;
    Ok(Value::Bool(entries.get(&args[0]).is_some()))
}

/// `dict.get(key)` — value stored under `key`, or `none` when the key is absent.
///
/// Mirrors indexing, except that a missing key is absence rather than a fault.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a Dict.
pub fn dict_get(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "dict.get")?;
    let entries = expect_dict(receiver)?;
    Ok(entries.get(&args[0]).cloned().unwrap_or(Value::None))
}

/// `dict.keys()` — new List with the keys in insertion order.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a Dict.
pub fn dict_keys(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "dict.keys")?;
    let entries = expect_dict(receiver)?;
    Ok(Value::List(Rc::new(RefCell::new((entries.keys()).into()))))
}

/// `dict.values()` — new List with the values in insertion order.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a Dict.
pub fn dict_values(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "dict.values")?;
    let entries = expect_dict(receiver)?;
    Ok(Value::List(Rc::new(RefCell::new(
        (entries.values()).into(),
    ))))
}

/// `dict.entries()` — new List with `[key, value]` pairs in insertion order.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a Dict.
pub fn dict_entries(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "dict.entries")?;
    let entries = expect_dict(receiver)?;
    let pairs: Vec<_> = entries
        .entries()
        .iter()
        .map(|(k, v)| Value::List(Rc::new(RefCell::new((vec![k.clone(), v.clone()]).into()))))
        .collect();
    Ok(Value::List(Rc::new(RefCell::new((pairs).into()))))
}

/// `dict.remove(key)` — removes a key, reporting whether it existed.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a Dict.
pub fn dict_remove(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "dict.remove")?;
    let Value::Dict(dict) = receiver else {
        return Err(VmFault::TypeMismatch {
            expected: "Dict receiver".to_string(),
            actual: receiver.type_name().to_string(),
        });
    };
    Ok(Value::Bool(dict.borrow_mut().remove(&args[0])))
}

/// `dict.clear()` — removes every entry in place.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a Dict.
pub fn dict_clear(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "dict.clear")?;
    let Value::Dict(dict) = receiver else {
        return Err(VmFault::TypeMismatch {
            expected: "Dict receiver".to_string(),
            actual: receiver.type_name().to_string(),
        });
    };
    dict.borrow_mut().clear();
    Ok(Value::None)
}

/// `dict.is_empty()` — canonical emptiness check.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a Dict.
pub fn dict_is_empty(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "dict.is_empty")?;
    Ok(Value::Bool(expect_dict(receiver)?.is_empty()))
}

/// `dict.len()` — entry count.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a Dict.
pub fn dict_len(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "dict.len")?;
    #[allow(clippy::cast_possible_wrap)]
    Ok(Value::Int(expect_dict(receiver)?.len() as i64))
}

/// Extracts a shared set receiver, or a type fault.
fn expect_set(receiver: &Value) -> Result<Rc<RefCell<aipo_vm::Collection<Value>>>, VmFault> {
    match receiver {
        Value::Set(items) => Ok(Rc::clone(items)),
        other => Err(VmFault::TypeMismatch {
            expected: "Set receiver".to_string(),
            actual: other.type_name().to_string(),
        }),
    }
}

/// `set.has(value)` — reports whether value is present.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a Set.
pub fn set_has(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "set.has")?;
    let items = expect_set(receiver)?;
    let b = items.borrow();
    Ok(Value::Bool(b.contains(&args[0])))
}

/// `set.add(value)` — adds value if not present, preserving insertion order.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a Set.
pub fn set_add(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "set.add")?;
    let items = expect_set(receiver)?;
    let mut b = items.borrow_mut();
    if !b.contains(&args[0]) {
        b.push(args[0].clone());
    }
    Ok(Value::None)
}

/// `set.remove(value)` — removes value if present, returning whether it existed.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a Set.
pub fn set_remove(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "set.remove")?;
    let items = expect_set(receiver)?;
    let mut b = items.borrow_mut();
    if let Some(pos) = b.iter().position(|x| x == &args[0]) {
        b.remove(pos);
        Ok(Value::Bool(true))
    } else {
        Ok(Value::Bool(false))
    }
}

/// `set.clear()` — clears all elements in place.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a Set.
pub fn set_clear(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "set.clear")?;
    let items = expect_set(receiver)?;
    items.borrow_mut().clear();
    Ok(Value::None)
}

/// `set.is_empty()` — returns whether the set has no elements.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a Set.
pub fn set_is_empty(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "set.is_empty")?;
    let items = expect_set(receiver)?;
    Ok(Value::Bool(items.borrow().is_empty()))
}

/// `set.len()` — returns element count.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a Set.
pub fn set_len(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "set.len")?;
    let items = expect_set(receiver)?;
    #[allow(clippy::cast_possible_wrap)]
    Ok(Value::Int(items.borrow().len() as i64))
}

/// `set.to_list()` — returns elements in insertion order as a new List.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a Set.
pub fn set_to_list(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "set.to_list")?;
    let items = expect_set(receiver)?;
    let list = items.borrow().to_vec();
    Ok(Value::List(Rc::new(RefCell::new((list).into()))))
}

/// `list.lazy()` — snapshots list into a lazy Sequence.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a List.
pub fn list_lazy(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "list.lazy")?;
    let Value::List(list) = receiver else {
        return Err(VmFault::TypeMismatch {
            expected: "List receiver".to_string(),
            actual: receiver.type_name().to_string(),
        });
    };
    let pipeline = aipo_vm::SequencePipeline::new(
        aipo_vm::SequenceSource::List(list.borrow().to_vec()),
        Vec::new(),
    );
    Ok(Value::Sequence(Rc::new(pipeline)))
}

/// `dict.lazy()` — snapshots dict values into a lazy Sequence.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a Dict.
pub fn dict_lazy(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "dict.lazy")?;
    let Value::Dict(dict) = receiver else {
        return Err(VmFault::TypeMismatch {
            expected: "Dict receiver".to_string(),
            actual: receiver.type_name().to_string(),
        });
    };
    let pipeline = aipo_vm::SequencePipeline::new(
        aipo_vm::SequenceSource::Dict(dict.borrow().values()),
        Vec::new(),
    );
    Ok(Value::Sequence(Rc::new(pipeline)))
}

/// `set.lazy()` — snapshots set members into a lazy Sequence.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the receiver is not a Set.
pub fn set_lazy(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "set.lazy")?;
    let items = expect_set(receiver)?;
    let pipeline = aipo_vm::SequencePipeline::new(
        aipo_vm::SequenceSource::Set(items.borrow().to_vec()),
        Vec::new(),
    );
    Ok(Value::Sequence(Rc::new(pipeline)))
}

/// Registers every List, Dict, and Set method on the VM so dot-call sugar resolves them.
pub fn register_methods(vm: &mut Vm) {
    vm.register_method_native("List", "add", 1, list_add);
    vm.register_method_native("List", "insert", 2, list_insert);
    vm.register_method_native("List", "remove", 1, list_remove);
    vm.register_method_native("List", "remove_at", 1, list_remove_at);
    vm.register_method_native("List", "remove_last", 0, list_remove_last);
    vm.register_method_native("List", "clear", 0, list_clear);
    vm.register_method_native("List", "contains", 1, list_contains);
    vm.register_method_native("List", "find", 1, list_find);
    vm.register_method_native("List", "find_index", 1, list_find_index);
    vm.register_method_native("List", "count", 1, list_count);
    vm.register_method_native("List", "first", 0, list_first);
    vm.register_method_native("List", "first_or", 1, list_first_or);
    vm.register_method_native("List", "last", 0, list_last);
    vm.register_method_native("List", "last_or", 1, list_last_or);
    vm.register_method_native("List", "is_empty", 0, list_is_empty);
    vm.register_method_native("List", "reverse", 0, list_reverse);
    vm.register_method_native("List", "sort", 0, list_sort);
    vm.register_method_native("List", "len", 0, list_len);
    vm.register_method_native("List", "take", 1, list_take);
    vm.register_method_native("List", "skip", 1, list_skip);
    vm.register_method_native("List", "distinct", 0, list_distinct);
    vm.register_method_native("List", "zip", 1, list_zip);
    vm.register_method_native("List", "chain", 1, list_chain);
    vm.register_method_native("List", "chunk", 1, list_chunk);
    vm.register_method_native("List", "window", 1, list_window);
    vm.register_method_native("List", "enumerate", 0, list_enumerate);
    vm.register_method_native("List", "lazy", 0, list_lazy);

    vm.register_method_native("Dict", "has", 1, dict_has);
    vm.register_method_native("Dict", "get", 1, dict_get);
    vm.register_method_native("Dict", "keys", 0, dict_keys);
    vm.register_method_native("Dict", "values", 0, dict_values);
    vm.register_method_native("Dict", "entries", 0, dict_entries);
    vm.register_method_native("Dict", "remove", 1, dict_remove);
    vm.register_method_native("Dict", "clear", 0, dict_clear);
    vm.register_method_native("Dict", "is_empty", 0, dict_is_empty);
    vm.register_method_native("Dict", "len", 0, dict_len);
    vm.register_method_native("Dict", "lazy", 0, dict_lazy);

    vm.register_method_native("Set", "has", 1, set_has);
    vm.register_method_native("Set", "add", 1, set_add);
    vm.register_method_native("Set", "remove", 1, set_remove);
    vm.register_method_native("Set", "clear", 0, set_clear);
    vm.register_method_native("Set", "is_empty", 0, set_is_empty);
    vm.register_method_native("Set", "len", 0, set_len);
    vm.register_method_native("Set", "to_list", 0, set_to_list);
    vm.register_method_native("Set", "lazy", 0, set_lazy);
}
