//! Canonical `regex` module for Aipo.
//!
//! Provides linear-time, sandboxed regular expression matching and transformation:
//! - `regex.compile(pattern)`: compiles a regex into a `Pattern` struct, or returns
//!   a recoverable `Failure` if the pattern is invalid.
//! - `pattern.is_match(text)`: tests whether the pattern matches anywhere in `text`.
//! - `pattern.find(text)`: returns the first matching substring, or `none`.
//! - `pattern.find_all(text)`: returns a List of all non-overlapping matches.
//! - `pattern.replace(text, replacement)`: replaces all matches with `replacement`.
//! - `pattern.split(text)`: splits `text` around matches into a List of substrings.
//! - `regex.is_match(pattern, text)`: module-level convenience function.
//! - `regex.replace(pattern, text, replacement)`: module-level convenience function.

use aipo_vm::{DictMap, FailureValue, StructInstance, Value, Vm, VmFault};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Mutex;

static REGEX_CACHE: Mutex<Option<HashMap<String, regex::Regex>>> = Mutex::new(None);

fn with_regex<R>(pattern: &str, f: impl FnOnce(&regex::Regex) -> R) -> Result<R, String> {
    let mut guard = REGEX_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let map = guard.get_or_insert_with(HashMap::new);
    if !map.contains_key(pattern) {
        let compiled = regex::Regex::new(pattern).map_err(|e| e.to_string())?;
        map.insert(pattern.to_string(), compiled);
    }
    let re = map.get(pattern).unwrap();
    Ok(f(re))
}

fn recoverable(message: impl Into<String>) -> Value {
    Value::Failure(Rc::new(FailureValue::new(message.into())))
}

fn expect_string<'a>(val: &'a Value, op: &str) -> Result<&'a str, VmFault> {
    match val {
        Value::String(s) => Ok(s.as_str()),
        other => Err(VmFault::TypeMismatch {
            expected: format!("String for {op}"),
            actual: other.type_name().to_string(),
        }),
    }
}

/// Creates a `Pattern` struct instance wrapping a validated pattern string.
#[must_use]
pub fn create_pattern_instance(pattern: &str) -> Value {
    let inst = StructInstance {
        type_name: "Pattern".to_string(),
        fields: vec![(
            "pattern".to_string(),
            Value::String(Rc::new(pattern.to_string())),
        )],
        fixed_fields: HashSet::new(),
        under_construction: false,
    };
    Value::Struct(Rc::new(RefCell::new(inst)))
}

fn get_pattern_str(receiver: &Value) -> Result<String, VmFault> {
    if let Value::Struct(inst) = receiver {
        let b = inst.borrow();
        if b.type_name != "Pattern" {
            return Err(VmFault::TypeMismatch {
                expected: "Pattern struct instance".to_string(),
                actual: b.type_name.clone(),
            });
        }
        match b.get_field("pattern") {
            Some(Value::String(s)) => Ok(s.to_string()),
            _ => Err(VmFault::TypeMismatch {
                expected: "Pattern with String 'pattern' field".to_string(),
                actual: "Pattern without pattern field".to_string(),
            }),
        }
    } else {
        Err(VmFault::TypeMismatch {
            expected: "Pattern struct receiver".to_string(),
            actual: receiver.type_name().to_string(),
        })
    }
}

/// `pattern.is_match(text)` — tests whether the pattern matches anywhere in `text`.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not a String.
pub fn method_pattern_is_match(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument (text) for Pattern.is_match".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    let pat = get_pattern_str(receiver)?;
    let text = expect_string(&args[0], "Pattern.is_match")?;
    match with_regex(&pat, |re| re.is_match(text)) {
        Ok(matched) => Ok(Value::Bool(matched)),
        Err(err) => Ok(recoverable(format!("regex error: {err}"))),
    }
}

/// `pattern.find(text)` — returns the first matching substring, or `none`.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not a String.
pub fn method_pattern_find(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument (text) for Pattern.find".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    let pat = get_pattern_str(receiver)?;
    let text = expect_string(&args[0], "Pattern.find")?;
    match with_regex(&pat, |re| re.find(text).map(|m| m.as_str().to_string())) {
        Ok(Some(matched)) => Ok(Value::String(Rc::new(matched))),
        Ok(None) => Ok(Value::None),
        Err(err) => Ok(recoverable(format!("regex error: {err}"))),
    }
}

/// `pattern.find_all(text)` — returns a List of all non-overlapping matches.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not a String.
pub fn method_pattern_find_all(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument (text) for Pattern.find_all".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    let pat = get_pattern_str(receiver)?;
    let text = expect_string(&args[0], "Pattern.find_all")?;
    match with_regex(&pat, |re| {
        re.find_iter(text)
            .map(|m| Value::String(Rc::new(m.as_str().to_string())))
            .collect::<Vec<_>>()
    }) {
        Ok(matches) => Ok(Value::List(Rc::new(RefCell::new((matches).into())))),
        Err(err) => Ok(recoverable(format!("regex error: {err}"))),
    }
}

/// `pattern.replace(text, replacement)` — replaces all matches with `replacement`.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operands are not Strings.
pub fn method_pattern_replace(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 2 {
        return Err(VmFault::TypeMismatch {
            expected: "2 arguments (text, replacement) for Pattern.replace".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    if let Value::Failure(f) = &args[1] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    let pat = get_pattern_str(receiver)?;
    let text = expect_string(&args[0], "Pattern.replace text")?;
    let replacement = expect_string(&args[1], "Pattern.replace replacement")?;
    match with_regex(&pat, |re| re.replace_all(text, replacement).into_owned()) {
        Ok(replaced) => Ok(Value::String(Rc::new(replaced))),
        Err(err) => Ok(recoverable(format!("regex error: {err}"))),
    }
}

/// `pattern.split(text)` — splits `text` around matches into a List of substrings.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not a String.
pub fn method_pattern_split(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument (text) for Pattern.split".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    let pat = get_pattern_str(receiver)?;
    let text = expect_string(&args[0], "Pattern.split")?;
    match with_regex(&pat, |re| {
        re.split(text)
            .map(|s| Value::String(Rc::new(s.to_string())))
            .collect::<Vec<_>>()
    }) {
        Ok(parts) => Ok(Value::List(Rc::new(RefCell::new((parts).into())))),
        Err(err) => Ok(recoverable(format!("regex error: {err}"))),
    }
}

/// `regex.compile(pattern)` — compiles a regex string into a `Pattern` struct.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if argument is not a String.
pub fn regex_compile(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument for regex.compile".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    let pattern_str = expect_string(&args[0], "regex.compile")?;
    match regex::Regex::new(pattern_str) {
        Ok(re) => {
            let mut guard = REGEX_CACHE.lock().unwrap_or_else(|e| e.into_inner());
            let map = guard.get_or_insert_with(HashMap::new);
            map.insert(pattern_str.to_string(), re);
            Ok(create_pattern_instance(pattern_str))
        }
        Err(err) => Ok(recoverable(format!("invalid regex pattern: {err}"))),
    }
}

/// `regex.is_match(pattern, text)` — convenience function.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if arguments are not Strings.
pub fn regex_is_match(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 2 {
        return Err(VmFault::TypeMismatch {
            expected: "2 arguments for regex.is_match".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    if let Value::Failure(f) = &args[1] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    let pat = expect_string(&args[0], "regex.is_match pattern")?;
    let text = expect_string(&args[1], "regex.is_match text")?;
    match with_regex(pat, |re| re.is_match(text)) {
        Ok(matched) => Ok(Value::Bool(matched)),
        Err(err) => Ok(recoverable(format!("invalid regex pattern: {err}"))),
    }
}

/// `regex.replace(pattern, text, replacement)` — convenience function.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if arguments are not Strings.
pub fn regex_replace(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 3 {
        return Err(VmFault::TypeMismatch {
            expected: "3 arguments for regex.replace".to_string(),
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
    let pat = expect_string(&args[0], "regex.replace pattern")?;
    let text = expect_string(&args[1], "regex.replace text")?;
    let repl = expect_string(&args[2], "regex.replace replacement")?;
    match with_regex(pat, |re| re.replace_all(text, repl).into_owned()) {
        Ok(res) => Ok(Value::String(Rc::new(res))),
        Err(err) => Ok(recoverable(format!("invalid regex pattern: {err}"))),
    }
}

/// Registers methods on `Pattern` struct instances.
pub fn register_methods(vm: &mut Vm) {
    vm.register_method_native("Pattern", "is_match", 1, method_pattern_is_match);
    vm.register_method_native("Pattern", "find", 1, method_pattern_find);
    vm.register_method_native("Pattern", "find_all", 1, method_pattern_find_all);
    vm.register_method_native("Pattern", "replace", 2, method_pattern_replace);
    vm.register_method_native("Pattern", "split", 1, method_pattern_split);
}

/// Creates the canonical `regex` module dictionary.
#[must_use]
pub fn create_module() -> Value {
    let entries = vec![
        (
            Value::String(Rc::new("compile".to_string())),
            Value::native("regex.compile", 1, regex_compile),
        ),
        (
            Value::String(Rc::new("is_match".to_string())),
            Value::native("regex.is_match", 2, regex_is_match),
        ),
        (
            Value::String(Rc::new("replace".to_string())),
            Value::native("regex.replace", 3, regex_replace),
        ),
    ];
    Value::Dict(Rc::new(RefCell::new(DictMap::from_entries(entries))))
}
