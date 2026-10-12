//! Canonical `string` module for Aipo.
//!
//! Provides text manipulation and inspection operations over Unicode code points:
//! - `len`: code point count
//! - `byte_len`: UTF-8 byte length
//! - `contains`: substring check
//! - `starts_with`: prefix check
//! - `ends_with`: suffix check
//! - `find`: code point index of the first match, or `none`
//! - `lower`: lowercase conversion
//! - `upper`: uppercase conversion
//! - `capitalize`: titlecase first cased character, lowercase the rest
//! - `reverse`: reverses extended grapheme clusters
//! - `trim`: whitespace trimming
//! - `split`: splits into a List of substrings
//! - `join`: joins a List of Strings with a separator
//! - `replace`: substring replacement
//! - `slice`: substring slicing by character indices
//! - `format`: named placeholder substitution for runtime templates

use aipo_vm::{DictMap, FailureValue, Value, VmFault, check_safe_int};
use std::cell::RefCell;
use std::rc::Rc;
#[cfg(feature = "unicode")]
use unicode_normalization::UnicodeNormalization;
#[cfg(feature = "unicode")]
use unicode_segmentation::UnicodeSegmentation;

fn expect_string<'a>(val: &'a Value, op: &str) -> Result<&'a str, VmFault> {
    match val {
        Value::String(s) => Ok(s.as_str()),
        other => Err(VmFault::TypeMismatch {
            expected: format!("String for {op}"),
            actual: other.type_name().to_string(),
        }),
    }
}

/// Requires a non-empty String argument, mirroring canon argument rules.
fn expect_non_empty<'a>(text: &'a str, op: &str) -> Result<&'a str, VmFault> {
    if text.is_empty() {
        Err(VmFault::TypeMismatch {
            expected: format!("non-empty String for {op}"),
            actual: "empty String".to_string(),
        })
    } else {
        Ok(text)
    }
}

/// Re-normalizes text to NFC.
///
/// Canon makes NFC an invariant of `String` ("normalizada automaticamente ... antes de ser exposta
/// ao programa"), not an operation performed at `==`. Every operation that can produce a non-NFC
/// result — case mapping, joining, replacing, formatting — therefore normalizes its output. Pure
/// substrings (`slice`, `split`, `trim`) keep the invariant for free, because removing characters
/// never makes two previously non-adjacent characters adjacent.
/// Normalizes a string slice to NFC.
#[cfg(feature = "unicode")]
#[must_use]
pub fn normalize_nfc_str(text: &str) -> String {
    text.nfc().collect()
}

/// Passes through string when `unicode` feature is disabled.
#[cfg(not(feature = "unicode"))]
#[must_use]
pub fn normalize_nfc_str(text: &str) -> String {
    text.to_string()
}

fn nfc(text: String) -> String {
    normalize_nfc_str(&text)
}

fn arity_error(op: &str, expected: usize, actual: usize) -> VmFault {
    VmFault::TypeMismatch {
        expected: format!("{expected} argument(s) for {op}"),
        actual: format!("{actual} arguments"),
    }
}

fn recoverable(message: impl Into<String>) -> Value {
    Value::Failure(Rc::new(FailureValue::new(message.into())))
}

/// Returns character count of a string.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not String.
pub fn string_len(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }

    let s = expect_string(&args[0], "string.len")?;
    #[allow(clippy::cast_possible_wrap)]
    let count = s.chars().count() as i64;
    check_safe_int(count).map(Value::Int)
}

/// Checks if a string contains a substring.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operands are not strings.
pub fn string_contains(args: &[Value]) -> Result<Value, VmFault> {
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

    let s = expect_string(&args[0], "string.contains (haystack)")?;
    let sub = expect_string(&args[1], "string.contains (needle)")?;
    Ok(Value::Bool(s.contains(sub)))
}

/// Checks if a string begins with a prefix.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operands are not strings.
pub fn string_starts_with(args: &[Value]) -> Result<Value, VmFault> {
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

    let s = expect_string(&args[0], "string.starts_with")?;
    let prefix = expect_string(&args[1], "string.starts_with")?;
    Ok(Value::Bool(s.starts_with(prefix)))
}

/// Checks if a string ends with a suffix.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operands are not strings.
pub fn string_ends_with(args: &[Value]) -> Result<Value, VmFault> {
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

    let s = expect_string(&args[0], "string.ends_with")?;
    let suffix = expect_string(&args[1], "string.ends_with")?;
    Ok(Value::Bool(s.ends_with(suffix)))
}

/// Converts string to lowercase.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not String.
pub fn string_lower(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }

    let s = expect_string(&args[0], "string.lower")?;
    Ok(Value::String(Rc::new(nfc(s.to_lowercase()))))
}

/// Converts string to uppercase.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not String.
pub fn string_upper(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }

    let s = expect_string(&args[0], "string.upper")?;
    Ok(Value::String(Rc::new(nfc(s.to_uppercase()))))
}

/// Trims leading and trailing whitespace.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not String.
pub fn string_trim(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }

    let s = expect_string(&args[0], "string.trim")?;
    Ok(Value::String(Rc::new(s.trim().to_string())))
}

/// Splits string by a non-empty separator into a List of substrings.
///
/// Empty fields are preserved, including at the edges.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operands are not strings, or if the separator
/// is empty (an empty separator is invalid in V1).
pub fn string_split(args: &[Value]) -> Result<Value, VmFault> {
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

    let s = expect_string(&args[0], "string.split")?;
    let delim = expect_string(&args[1], "string.split separator")?;
    let delim = expect_non_empty(delim, "string.split separator")?;

    // Rust's `split` preserves empty fields, matching the canonical examples
    // `"a,,b".split(",") == ["a", "", "b"]` and `"".split(",") == [""]`.
    let parts: Vec<Value> = s
        .split(delim)
        .map(|part| Value::String(Rc::new(part.to_string())))
        .collect();

    Ok(Value::List(Rc::new(RefCell::new((parts).into()))))
}

/// Joins a List of `String` values with a separator.
///
/// Argument order follows canon: `join(separator, list)`. An empty list yields `""`,
/// a single element yields that element, and elements must already be `String` —
/// there is no implicit textual coercion (use `String(value)` explicitly).
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the separator is not a String, the second
/// argument is not a List, or any element is not a String.
pub fn string_join(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 2 {
        return Err(arity_error("string.join", 2, args.len()));
    }

    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    if let Value::Failure(f) = &args[1] {
        return Ok(Value::Failure(Rc::clone(f)));
    }

    let separator = expect_string(&args[0], "string.join separator")?;

    let list = match &args[1] {
        Value::List(l) => l.borrow().clone(),
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "List for string.join".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };

    let mut result = String::new();
    for (i, val) in list.iter().enumerate() {
        if i > 0 {
            result.push_str(separator);
        }
        match val {
            Value::String(s) => result.push_str(s),
            other => {
                return Err(VmFault::TypeMismatch {
                    expected: "List of String for string.join (use String(value) to convert)"
                        .to_string(),
                    actual: other.type_name().to_string(),
                });
            }
        }
    }

    Ok(Value::String(Rc::new(nfc(result))))
}

/// Replaces all non-overlapping matches of a non-empty substring with replacement text.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operands are not strings, or if the pattern is
/// empty (an empty pattern is invalid in V1).
pub fn string_replace(args: &[Value]) -> Result<Value, VmFault> {
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

    let s = expect_string(&args[0], "string.replace")?;
    let from = expect_string(&args[1], "string.replace (from)")?;
    let from = expect_non_empty(from, "string.replace (from)")?;
    let to = expect_string(&args[2], "string.replace (to)")?;

    Ok(Value::String(Rc::new(nfc(s.replace(from, to)))))
}

/// Slices a substring by 0-based character indices `[start, end)`.
///
/// Negative indices count from the end of the string.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if arguments are invalid types.
pub fn string_slice(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 3 {
        return Err(VmFault::TypeMismatch {
            expected: "3 arguments".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }

    let s = expect_string(&args[0], "string.slice")?;

    let start_raw = match &args[1] {
        Value::Int(n) => *n,
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int start index".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };

    let end_raw = match &args[2] {
        Value::Int(n) => *n,
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int end index".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };

    let chars: Vec<char> = s.chars().collect();
    #[allow(clippy::cast_possible_wrap)]
    let len = chars.len() as i64;

    let normalize = |idx: i64| -> usize {
        if idx < 0 {
            let pos = len + idx;
            if pos < 0 {
                0
            } else {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                (pos as usize).min(chars.len())
            }
        } else {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            (idx as usize).min(chars.len())
        }
    };

    let start = normalize(start_raw);
    let end = normalize(end_raw);

    if start >= end {
        return Ok(Value::String(Rc::new(String::new())));
    }

    let sliced: String = chars[start..end].iter().collect();
    Ok(Value::String(Rc::new(sliced)))
}

/// Returns the UTF-8 byte length of a string.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not String.
pub fn string_byte_len(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(arity_error("string.byte_len", 1, args.len()));
    }

    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }

    let s = expect_string(&args[0], "string.byte_len")?;
    #[allow(clippy::cast_possible_wrap)]
    let count = s.len() as i64;
    check_safe_int(count).map(Value::Int)
}

/// Finds the first occurrence of a substring, returning its code point index.
///
/// `find(text, "")` returns `0`; a missing substring returns `none`.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operands are not strings.
pub fn string_find(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 2 {
        return Err(arity_error("string.find", 2, args.len()));
    }

    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }
    if let Value::Failure(f) = &args[1] {
        return Ok(Value::Failure(Rc::clone(f)));
    }

    let s = expect_string(&args[0], "string.find")?;
    let needle = expect_string(&args[1], "string.find (substring)")?;

    match s.find(needle) {
        Some(byte_index) => {
            #[allow(clippy::cast_possible_wrap)]
            let code_point_index = s[..byte_index].chars().count() as i64;
            check_safe_int(code_point_index).map(Value::Int)
        }
        None => Ok(Value::None),
    }
}

/// Titlecases the first cased character and lowercases the remaining cased characters.
///
/// Leading punctuation, whitespace and symbols are preserved, as is text without any
/// cased character (for example `"123"`).
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not String.
pub fn string_capitalize(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(arity_error("string.capitalize", 1, args.len()));
    }

    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }

    let s = expect_string(&args[0], "string.capitalize")?;

    let mut result = String::with_capacity(s.len());
    let mut titlecased = false;

    for ch in s.chars() {
        let cased = ch.is_uppercase() || ch.is_lowercase();
        if !titlecased && cased {
            result.extend(ch.to_uppercase());
            titlecased = true;
        } else {
            result.extend(ch.to_lowercase());
        }
    }

    Ok(Value::String(Rc::new(nfc(result))))
}

/// Reverses a string by extended grapheme clusters, preserving composed visual units.
///
/// The result is re-normalized to NFC so the `String` invariants continue to hold.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not String.
pub fn string_reverse(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(arity_error("string.reverse", 1, args.len()));
    }

    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }

    let s = expect_string(&args[0], "string.reverse")?;
    #[cfg(feature = "unicode")]
    let reversed: String = s.graphemes(true).rev().collect();
    #[cfg(not(feature = "unicode"))]
    let reversed: String = s.chars().rev().collect();
    let normalized = normalize_nfc_str(&reversed);

    Ok(Value::String(Rc::new(normalized)))
}

/// Returns extended grapheme clusters of a string as a List of Strings.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not String.
pub fn string_graphemes(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(arity_error("string.graphemes", 1, args.len()));
    }

    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }

    let s = expect_string(&args[0], "string.graphemes")?;
    #[cfg(feature = "unicode")]
    let clusters: Vec<Value> = s
        .graphemes(true)
        .map(|g| Value::String(Rc::new(g.to_string())))
        .collect();
    #[cfg(not(feature = "unicode"))]
    let clusters: Vec<Value> = s
        .chars()
        .map(|c| Value::String(Rc::new(c.to_string())))
        .collect();

    Ok(Value::List(Rc::new(RefCell::new((clusters).into()))))
}

/// Returns words of a string as a List of Strings using Unicode word segmentation.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not String.
pub fn string_words(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(arity_error("string.words", 1, args.len()));
    }

    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }

    let s = expect_string(&args[0], "string.words")?;
    #[cfg(feature = "unicode")]
    let words: Vec<Value> = s
        .unicode_words()
        .map(|w| Value::String(Rc::new(w.to_string())))
        .collect();
    #[cfg(not(feature = "unicode"))]
    let words: Vec<Value> = s
        .split_whitespace()
        .map(|w| Value::String(Rc::new(w.to_string())))
        .collect();

    Ok(Value::List(Rc::new(RefCell::new((words).into()))))
}

/// Splits a string by line breaks (`\r\n`, `\n`, `\r`) into a List of Strings.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not String.
pub fn string_lines(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(arity_error("string.lines", 1, args.len()));
    }

    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }

    let s = expect_string(&args[0], "string.lines")?;
    let lines: Vec<Value> = s
        .lines()
        .map(|l| Value::String(Rc::new(l.to_string())))
        .collect();

    Ok(Value::List(Rc::new(RefCell::new((lines).into()))))
}

/// Converts string using locale-neutral Unicode case folding.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if operand is not String.
pub fn string_casefold(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(arity_error("string.casefold", 1, args.len()));
    }

    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }

    let s = expect_string(&args[0], "string.casefold")?;
    Ok(Value::String(Rc::new(nfc(s.to_lowercase()))))
}

/// Splits a string into `(prefix, braced)` segments for `format`.
fn is_simple_placeholder_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_alphabetic() || first == '_' => {}
        _ => return false,
    }
    chars.all(|ch| ch.is_alphanumeric() || ch == '_')
}

fn lookup_placeholder(values: &DictMap, name: &str) -> Option<Value> {
    let key = Value::String(Rc::new(name.to_string()));
    values.get(&key).cloned()
}

/// Substitutes named placeholders in a runtime template.
///
/// `format(template, values)` uses `{name}` placeholders and `{{` / `}}` for literal
/// braces. Positional placeholders (`{0}`, `{}`) are outside V1, a missing key or a
/// malformed template yields a recoverable `Failure`, and unreferenced keys are ignored.
/// Placeholder values are converted textually like `f"..."` interpolation.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the template is not a String or `values` is not a Dict.
pub fn string_format(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 2 {
        return Err(arity_error("string.format", 2, args.len()));
    }

    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }

    let template = expect_string(&args[0], "string.format template")?;
    let entries = match &args[1] {
        Value::Dict(d) => d.borrow().clone(),
        Value::Failure(f) => return Ok(Value::Failure(Rc::clone(f))),
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Dict for string.format values".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };

    let mut output = String::with_capacity(template.len());
    let mut chars = template.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '{' => {
                if chars.peek() == Some(&'{') {
                    chars.next();
                    output.push('{');
                    continue;
                }

                let mut name = String::new();
                let mut closed = false;
                for inner in chars.by_ref() {
                    if inner == '}' {
                        closed = true;
                        break;
                    }
                    if inner == '{' {
                        return Ok(recoverable(
                            "malformed format template: nested '{' is not allowed",
                        ));
                    }
                    name.push(inner);
                }

                if !closed {
                    return Ok(recoverable("malformed format template: unclosed '{'"));
                }

                if !is_simple_placeholder_name(&name) {
                    return Ok(recoverable(format!(
                        "unsupported placeholder {{{name}}}: only simple names are allowed"
                    )));
                }

                match lookup_placeholder(&entries, &name) {
                    Some(value) => match value {
                        Value::Failure(f) => return Ok(Value::Failure(Rc::clone(&f))),
                        fundamental => {
                            output.push_str(&fundamental.to_string());
                        }
                    },
                    None => {
                        return Ok(recoverable(format!(
                            "missing format value for placeholder {{{name}}}"
                        )));
                    }
                }
            }
            '}' => {
                if chars.peek() == Some(&'}') {
                    chars.next();
                    output.push('}');
                    continue;
                }
                return Ok(recoverable("malformed format template: unmatched '}'"));
            }
            other => output.push(other),
        }
    }

    Ok(Value::String(Rc::new(nfc(output))))
}

/// Constructs the canonical `string` module dictionary.
#[must_use]
pub fn create_module() -> Value {
    let entries = vec![
        (
            Value::String(Rc::new("len".to_string())),
            Value::native("string.len", 1, string_len),
        ),
        (
            Value::String(Rc::new("byte_len".to_string())),
            Value::native("string.byte_len", 1, string_byte_len),
        ),
        (
            Value::String(Rc::new("find".to_string())),
            Value::native("string.find", 2, string_find),
        ),
        (
            Value::String(Rc::new("capitalize".to_string())),
            Value::native("string.capitalize", 1, string_capitalize),
        ),
        (
            Value::String(Rc::new("reverse".to_string())),
            Value::native("string.reverse", 1, string_reverse),
        ),
        (
            Value::String(Rc::new("format".to_string())),
            Value::native("string.format", 2, string_format),
        ),
        (
            Value::String(Rc::new("contains".to_string())),
            Value::native("string.contains", 2, string_contains),
        ),
        (
            Value::String(Rc::new("starts_with".to_string())),
            Value::native("string.starts_with", 2, string_starts_with),
        ),
        (
            Value::String(Rc::new("ends_with".to_string())),
            Value::native("string.ends_with", 2, string_ends_with),
        ),
        (
            Value::String(Rc::new("lower".to_string())),
            Value::native("string.lower", 1, string_lower),
        ),
        (
            Value::String(Rc::new("upper".to_string())),
            Value::native("string.upper", 1, string_upper),
        ),
        (
            Value::String(Rc::new("trim".to_string())),
            Value::native("string.trim", 1, string_trim),
        ),
        (
            Value::String(Rc::new("split".to_string())),
            Value::native("string.split", 2, string_split),
        ),
        (
            Value::String(Rc::new("join".to_string())),
            Value::native("string.join", 2, string_join),
        ),
        (
            Value::String(Rc::new("replace".to_string())),
            Value::native("string.replace", 3, string_replace),
        ),
        (
            Value::String(Rc::new("slice".to_string())),
            Value::native("string.slice", 3, string_slice),
        ),
        (
            Value::String(Rc::new("graphemes".to_string())),
            Value::native("string.graphemes", 1, string_graphemes),
        ),
        (
            Value::String(Rc::new("words".to_string())),
            Value::native("string.words", 1, string_words),
        ),
        (
            Value::String(Rc::new("lines".to_string())),
            Value::native("string.lines", 1, string_lines),
        ),
        (
            Value::String(Rc::new("casefold".to_string())),
            Value::native("string.casefold", 1, string_casefold),
        ),
    ];

    Value::Dict(Rc::new(RefCell::new(DictMap::from_entries(entries))))
}
