//! Canonical `path` standard library module.
//!
//! Provides pure, logical path manipulation routines. Normalizes all path
//! separators to forward slashes (`/`) for cross-platform portability across
//! Linux, macOS, Windows, and JavaScript backends.

#![forbid(unsafe_code)]

use aipo_vm::{DictMap, Value, VmFault};
use std::cell::RefCell;
use std::rc::Rc;

fn expect_string<'a>(arg: &'a Value, op: &str) -> Result<&'a str, VmFault> {
    match arg {
        Value::String(s) => Ok(s.as_str()),
        other => Err(VmFault::TypeMismatch {
            expected: format!("String for {op}"),
            actual: other.type_name().to_string(),
        }),
    }
}

/// Checks whether a path string is absolute (starts with `/` or Windows drive letter).
#[must_use]
pub fn is_absolute_path(path: &str) -> bool {
    if path.starts_with('/') || path.starts_with('\\') {
        return true;
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 3 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        let sep = bytes[2];
        if sep == b'/' || sep == b'\\' {
            return true;
        }
    }
    false
}

/// Normalizes a path logically: converts `\` to `/`, collapses duplicate slashes,
/// and resolves `.` and `..` segments.
#[must_use]
pub fn normalize_path(path: &str) -> String {
    if path.is_empty() {
        return ".".to_string();
    }

    let is_abs = path.starts_with('/') || path.starts_with('\\');
    let has_trailing_slash = path.ends_with('/') || path.ends_with('\\');

    // Handle Windows drive prefix e.g. "C:\"
    let (prefix, rest) = {
        let bytes = path.as_bytes();
        if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
            (&path[..2], &path[2..])
        } else {
            ("", path)
        }
    };

    let segments: Vec<&str> = rest
        .split(['/', '\\'])
        .filter(|s| !s.is_empty() && *s != ".")
        .collect();

    let mut stack: Vec<&str> = Vec::new();
    for seg in segments {
        if seg == ".." {
            if let Some(top) = stack.last()
                && *top != ".."
            {
                stack.pop();
                continue;
            }
            if !is_abs {
                stack.push("..");
            }
        } else {
            stack.push(seg);
        }
    }

    let mut out = String::new();
    if !prefix.is_empty() {
        out.push_str(prefix);
        out.push('/');
    } else if is_abs {
        out.push('/');
    }

    if stack.is_empty() {
        if out.is_empty() {
            return ".".to_string();
        }
    } else {
        out.push_str(&stack.join("/"));
        if has_trailing_slash && !out.ends_with('/') {
            out.push('/');
        }
    }

    out
}

/// `path.is_absolute(p)` — whether the path is absolute.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the argument is not a String.
pub fn path_is_absolute(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument for path.is_absolute".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let p = expect_string(&args[0], "path.is_absolute")?;
    Ok(Value::Bool(is_absolute_path(p)))
}

/// `path.normalize(p)` — normalizes a path logically.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the argument is not a String.
pub fn path_normalize(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument for path.normalize".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let p = expect_string(&args[0], "path.normalize")?;
    Ok(Value::String(Rc::new(normalize_path(p))))
}

/// `path.join(...)` — joins multiple path segments into a normalized path.
///
/// If an argument is a List, its items are treated as segments.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if any segment is not a String.
pub fn path_join(args: &[Value]) -> Result<Value, VmFault> {
    let mut parts: Vec<String> = Vec::new();

    for arg in args {
        match arg {
            Value::List(l) => {
                for item in l.borrow().iter() {
                    let s = expect_string(item, "path.join list element")?;
                    parts.push(s.to_string());
                }
            }
            Value::String(s) => {
                parts.push(s.as_str().to_string());
            }
            other => {
                return Err(VmFault::TypeMismatch {
                    expected: "String or List of Strings for path.join".to_string(),
                    actual: other.type_name().to_string(),
                });
            }
        }
    }

    if parts.is_empty() {
        return Ok(Value::String(Rc::new(".".to_string())));
    }

    let mut joined = String::new();
    for part in parts {
        if is_absolute_path(&part) || joined.is_empty() || joined == "." {
            joined = part;
        } else {
            if !joined.ends_with('/') && !joined.ends_with('\\') {
                joined.push('/');
            }
            joined.push_str(&part);
        }
    }

    Ok(Value::String(Rc::new(normalize_path(&joined))))
}

/// `path.basename(p, ext = "")` — returns the last portion of a path.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if arguments are not Strings.
pub fn path_basename(args: &[Value]) -> Result<Value, VmFault> {
    if args.is_empty() || args.len() > 2 {
        return Err(VmFault::TypeMismatch {
            expected: "1 or 2 arguments for path.basename".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let p = expect_string(&args[0], "path.basename")?;
    let ext = if args.len() == 2 {
        expect_string(&args[1], "path.basename ext")?
    } else {
        ""
    };

    let clean = p.trim_end_matches(['/', '\\']);
    let base = match clean.rfind(['/', '\\']) {
        Some(idx) => &clean[idx + 1..],
        None => clean,
    };

    let result = if !ext.is_empty() && base.ends_with(ext) && base.len() > ext.len() {
        &base[..base.len() - ext.len()]
    } else {
        base
    };

    Ok(Value::String(Rc::new(result.to_string())))
}

/// `path.dirname(p)` — returns the directory name of a path.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if argument is not a String.
pub fn path_dirname(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument for path.dirname".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let p = expect_string(&args[0], "path.dirname")?;
    let clean = p.trim_end_matches(['/', '\\']);
    let dir = match clean.rfind(['/', '\\']) {
        Some(0) => "/",
        Some(idx) => &clean[..idx],
        None => {
            if is_absolute_path(p) {
                "/"
            } else {
                "."
            }
        }
    };

    Ok(Value::String(Rc::new(dir.to_string())))
}

/// `path.ext(p)` — returns the extension of the path (including the `.`).
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if argument is not a String.
pub fn path_ext(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument for path.ext".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }
    let p = expect_string(&args[0], "path.ext")?;
    let clean = p.trim_end_matches(['/', '\\']);
    let base = match clean.rfind(['/', '\\']) {
        Some(idx) => &clean[idx + 1..],
        None => clean,
    };

    // Hidden files like ".gitignore" without further dots have no extension
    if let Some(idx) = base.rfind('.')
        && idx > 0
    {
        return Ok(Value::String(Rc::new(base[idx..].to_string())));
    }

    Ok(Value::String(Rc::new(String::new())))
}

/// Constructs the canonical `path` module dictionary.
#[must_use]
pub fn create_module() -> Value {
    let entries = vec![
        (
            Value::String(Rc::new("join".to_string())),
            Value::native("path.join", usize::MAX, path_join),
        ),
        (
            Value::String(Rc::new("normalize".to_string())),
            Value::native("path.normalize", 1, path_normalize),
        ),
        (
            Value::String(Rc::new("is_absolute".to_string())),
            Value::native("path.is_absolute", 1, path_is_absolute),
        ),
        (
            Value::String(Rc::new("basename".to_string())),
            Value::native("path.basename", usize::MAX, path_basename),
        ),
        (
            Value::String(Rc::new("dirname".to_string())),
            Value::native("path.dirname", 1, path_dirname),
        ),
        (
            Value::String(Rc::new("ext".to_string())),
            Value::native("path.ext", 1, path_ext),
        ),
    ];
    Value::Dict(Rc::new(RefCell::new(DictMap::from_entries(entries))))
}
