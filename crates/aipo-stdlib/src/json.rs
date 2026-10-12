//! Canonical JSON module for Aipo.
//!
//! Provides portable, deterministic JSON serialization and deserialization:
//! - `json.parse(text)`: parses JSON text into Aipo values (`none`, `Bool`, `Int`, `Float`, `String` NFC, `List`, `Dict`)
//!   Strictly rejects duplicate object keys with recoverable `Failure`.
//! - `json.stringify(value, pretty = false)`: serializes Aipo values to JSON string.
//!   Detects cycles and rejects un-serializable types with recoverable `Failure`.

#![forbid(unsafe_code)]

use crate::string::normalize_nfc_str;
use aipo_vm::{
    DictMap, FailureValue, MAX_SAFE_INT, MIN_SAFE_INT, Value, VmFault, check_finite_float,
    check_safe_int,
};
use serde::de::{DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use std::cell::RefCell;
use std::collections::HashSet;
use std::fmt;
use std::rc::Rc;

fn normalize_nfc(s: &str) -> String {
    normalize_nfc_str(s)
}

struct AipoJsonVisitor;

impl<'de> Visitor<'de> for AipoJsonVisitor {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a valid JSON value")
    }

    fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<Self::Value, E> {
        Ok(Value::Bool(v))
    }

    fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<Self::Value, E> {
        if (MIN_SAFE_INT..=MAX_SAFE_INT).contains(&v) {
            Ok(Value::Int(v))
        } else {
            #[allow(clippy::cast_precision_loss)]
            Ok(Value::Float(v as f64))
        }
    }

    fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<Self::Value, E> {
        if v <= MAX_SAFE_INT as u64 {
            #[allow(clippy::cast_possible_wrap)]
            Ok(Value::Int(v as i64))
        } else {
            #[allow(clippy::cast_precision_loss)]
            Ok(Value::Float(v as f64))
        }
    }

    fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<Self::Value, E> {
        if !v.is_finite() {
            return Err(serde::de::Error::custom("non-finite float in JSON"));
        }
        #[allow(clippy::cast_possible_truncation)]
        if v.fract() == 0.0 && v >= MIN_SAFE_INT as f64 && v <= MAX_SAFE_INT as f64 {
            Ok(Value::Int(v as i64))
        } else {
            Ok(Value::Float(v))
        }
    }

    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<Self::Value, E> {
        Ok(Value::String(Rc::new(normalize_nfc(v))))
    }

    fn visit_string<E: serde::de::Error>(self, v: String) -> Result<Self::Value, E> {
        Ok(Value::String(Rc::new(normalize_nfc(&v))))
    }

    fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        Ok(Value::None)
    }

    fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
        let mut items = Vec::new();
        while let Some(item) = seq.next_element_seed(AipoJsonSeed)? {
            items.push(item);
        }
        Ok(Value::List(Rc::new(RefCell::new((items).into()))))
    }

    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
        let mut entries = Vec::new();
        let mut seen = HashSet::new();

        while let Some(key) = map.next_key::<String>()? {
            if !seen.insert(key.clone()) {
                return Err(serde::de::Error::custom(format!(
                    "duplicate object key: '{key}'"
                )));
            }
            let val = map.next_value_seed(AipoJsonSeed)?;
            entries.push((Value::String(Rc::new(normalize_nfc(&key))), val));
        }

        Ok(Value::Dict(Rc::new(RefCell::new(DictMap::from_entries(
            entries,
        )))))
    }
}

struct AipoJsonSeed;

impl<'de> DeserializeSeed<'de> for AipoJsonSeed {
    type Value = Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_any(AipoJsonVisitor)
    }
}

/// Parses JSON text into an Aipo value.
///
/// Returns a recoverable `Value::Failure` on syntax errors or duplicate object keys.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if argument is not a String.
pub fn json_parse(args: &[Value]) -> Result<Value, VmFault> {
    if args.len() != 1 {
        return Err(VmFault::TypeMismatch {
            expected: "1 argument (text)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    let text = match &args[0] {
        Value::String(s) => s.as_str(),
        Value::Failure(f) => return Ok(Value::Failure(Rc::clone(f))),
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "String text".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };

    let mut de = serde_json::Deserializer::from_str(text);
    match AipoJsonSeed.deserialize(&mut de) {
        Ok(val) => {
            if let Err(e) = de.end() {
                return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                    "json.parse error: {e}"
                )))));
            }
            Ok(val)
        }
        Err(e) => Ok(Value::Failure(Rc::new(FailureValue::new(format!(
            "json.parse error: {e}"
        ))))),
    }
}

fn stringify_value(
    val: &Value,
    pretty: bool,
    depth: usize,
    active_ptrs: &mut HashSet<usize>,
) -> Result<String, String> {
    match val {
        Value::None => Ok("null".to_string()),
        Value::Bool(b) => Ok(b.to_string()),
        Value::Int(n) => {
            check_safe_int(*n).map_err(|_| format!("{n} exceeds safe integer range"))?;
            Ok(n.to_string())
        }
        Value::Float(f) => {
            check_finite_float(*f)
                .map_err(|_| "non-finite float cannot be serialized to JSON".to_string())?;
            if f.fract() == 0.0 {
                Ok(format!("{f:.1}"))
            } else {
                Ok(f.to_string())
            }
        }
        Value::String(s) => {
            let nfc = normalize_nfc(s);
            serde_json::to_string(&nfc).map_err(|e| e.to_string())
        }
        Value::List(l) => {
            let ptr = Rc::as_ptr(l) as usize;
            if !active_ptrs.insert(ptr) {
                return Err("cyclic value cannot be serialized".to_string());
            }
            let items = l.borrow().clone();
            let mut out = String::new();
            if items.is_empty() {
                out.push_str("[]");
            } else if pretty {
                out.push_str("[\n");
                let indent = "  ".repeat(depth + 1);
                for (i, it) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(",\n");
                    }
                    out.push_str(&indent);
                    out.push_str(&stringify_value(it, pretty, depth + 1, active_ptrs)?);
                }
                out.push('\n');
                out.push_str(&"  ".repeat(depth));
                out.push(']');
            } else {
                out.push('[');
                for (i, it) in items.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&stringify_value(it, pretty, depth + 1, active_ptrs)?);
                }
                out.push(']');
            }
            active_ptrs.remove(&ptr);
            Ok(out)
        }
        Value::Dict(d) => {
            let ptr = Rc::as_ptr(d) as usize;
            if !active_ptrs.insert(ptr) {
                return Err("cyclic value cannot be serialized".to_string());
            }
            let entries = d.borrow().entries().to_vec();
            let mut out = String::new();
            if entries.is_empty() {
                out.push_str("{}");
            } else if pretty {
                out.push_str("{\n");
                let indent = "  ".repeat(depth + 1);
                for (i, (k, v)) in entries.iter().enumerate() {
                    let key_str = match k {
                        Value::String(s) => normalize_nfc(s),
                        other => {
                            active_ptrs.remove(&ptr);
                            return Err(format!(
                                "object keys must be strings, got {}",
                                other.type_name()
                            ));
                        }
                    };
                    if i > 0 {
                        out.push_str(",\n");
                    }
                    out.push_str(&indent);
                    out.push_str(&serde_json::to_string(&key_str).map_err(|e| e.to_string())?);
                    out.push_str(": ");
                    out.push_str(&stringify_value(v, pretty, depth + 1, active_ptrs)?);
                }
                out.push('\n');
                out.push_str(&"  ".repeat(depth));
                out.push('}');
            } else {
                out.push('{');
                for (i, (k, v)) in entries.iter().enumerate() {
                    let key_str = match k {
                        Value::String(s) => normalize_nfc(s),
                        other => {
                            active_ptrs.remove(&ptr);
                            return Err(format!(
                                "object keys must be strings, got {}",
                                other.type_name()
                            ));
                        }
                    };
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&serde_json::to_string(&key_str).map_err(|e| e.to_string())?);
                    out.push_str(": ");
                    out.push_str(&stringify_value(v, pretty, depth + 1, active_ptrs)?);
                }
                out.push('}');
            }
            active_ptrs.remove(&ptr);
            Ok(out)
        }
        Value::Struct(s) => {
            let ptr = Rc::as_ptr(s) as usize;
            if !active_ptrs.insert(ptr) {
                return Err("cyclic value cannot be serialized".to_string());
            }
            let fields = s.borrow().fields.clone();
            let mut out = String::new();
            if fields.is_empty() {
                out.push_str("{}");
            } else if pretty {
                out.push_str("{\n");
                let indent = "  ".repeat(depth + 1);
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push_str(",\n");
                    }
                    out.push_str(&indent);
                    out.push_str(&serde_json::to_string(k).map_err(|e| e.to_string())?);
                    out.push_str(": ");
                    out.push_str(&stringify_value(v, pretty, depth + 1, active_ptrs)?);
                }
                out.push('\n');
                out.push_str(&"  ".repeat(depth));
                out.push('}');
            } else {
                out.push('{');
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&serde_json::to_string(k).map_err(|e| e.to_string())?);
                    out.push_str(": ");
                    out.push_str(&stringify_value(v, pretty, depth + 1, active_ptrs)?);
                }
                out.push('}');
            }
            active_ptrs.remove(&ptr);
            Ok(out)
        }
        other => Err(format!("{} is not serializable to JSON", other.type_name())),
    }
}

/// Serializes an Aipo value into a JSON string.
///
/// Returns a recoverable `Value::Failure` on cycles or unsupported types.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if arguments are invalid.
pub fn json_stringify(args: &[Value]) -> Result<Value, VmFault> {
    if args.is_empty() || args.len() > 2 {
        return Err(VmFault::TypeMismatch {
            expected: "1 or 2 arguments (value, pretty = false)".to_string(),
            actual: format!("{} arguments", args.len()),
        });
    }

    if let Value::Failure(f) = &args[0] {
        return Ok(Value::Failure(Rc::clone(f)));
    }

    let pretty = if args.len() == 2 {
        match &args[1] {
            Value::Bool(b) => *b,
            other => {
                return Err(VmFault::TypeMismatch {
                    expected: "Bool pretty flag".to_string(),
                    actual: other.type_name().to_string(),
                });
            }
        }
    } else {
        false
    };

    let mut active = HashSet::new();
    match stringify_value(&args[0], pretty, 0, &mut active) {
        Ok(json_str) => Ok(Value::String(Rc::new(json_str))),
        Err(err) => Ok(Value::Failure(Rc::new(FailureValue::new(format!(
            "json.stringify error: {err}"
        ))))),
    }
}

/// Constructs the canonical `json` module dictionary.
#[must_use]
pub fn create_module() -> Value {
    let entries = vec![
        (
            Value::String(Rc::new("parse".to_string())),
            Value::native("json.parse", 1, json_parse),
        ),
        (
            Value::String(Rc::new("stringify".to_string())),
            Value::native("json.stringify", 1, json_stringify),
        ),
    ];
    Value::Dict(Rc::new(RefCell::new(DictMap::from_entries(entries))))
}
