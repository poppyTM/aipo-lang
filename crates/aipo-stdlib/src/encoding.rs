//! Canonical `encoding` standard library module.
//!
//! Provides pure, portable encoding and decoding routines for UTF-8, Base64,
//! Base64URL, and Hex.
//!
//! In accordance with canon, invalid input produces a recoverable Model B [`FailureValue`],
//! never a panic, corruption, or silent lossy substitution.

#![forbid(unsafe_code)]

use crate::string::normalize_nfc_str;
use aipo_vm::{DictMap, FailureValue, Value, VmFault};
use std::cell::RefCell;
use std::rc::Rc;

const BASE64_STANDARD_TABLE: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const BASE64_URL_TABLE: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn recoverable(msg: impl Into<String>) -> Value {
    Value::Failure(Rc::new(FailureValue::new(msg.into())))
}

fn extract_input_bytes(arg: &Value, op: &str) -> Result<Vec<u8>, VmFault> {
    match arg {
        Value::Bytes(b) => Ok(b.borrow().to_vec()),
        Value::String(s) => Ok(s.as_bytes().to_vec()),
        other => Err(VmFault::TypeMismatch {
            expected: format!("Bytes or String for {op}"),
            actual: other.type_name().to_string(),
        }),
    }
}

fn expect_string<'a>(arg: &'a Value, op: &str) -> Result<&'a str, VmFault> {
    match arg {
        Value::String(s) => Ok(s.as_str()),
        other => Err(VmFault::TypeMismatch {
            expected: format!("String for {op}"),
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

fn b64_encode(data: &[u8], table: &[u8; 64], pad: bool) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i];
        let b1 = if i + 1 < data.len() { data[i + 1] } else { 0 };
        let b2 = if i + 2 < data.len() { data[i + 2] } else { 0 };

        let idx0 = (b0 >> 2) as usize;
        let idx1 = (((b0 & 0x03) << 4) | (b1 >> 4)) as usize;
        let idx2 = (((b1 & 0x0F) << 2) | (b2 >> 6)) as usize;
        let idx3 = (b2 & 0x3F) as usize;

        out.push(table[idx0] as char);
        out.push(table[idx1] as char);

        if i + 1 < data.len() {
            out.push(table[idx2] as char);
        } else if pad {
            out.push('=');
        }

        if i + 2 < data.len() {
            out.push(table[idx3] as char);
        } else if pad {
            out.push('=');
        }

        i += 3;
    }
    out
}

fn b64_decode_char(ch: u8) -> Option<u8> {
    match ch {
        b'A'..=b'Z' => Some(ch - b'A'),
        b'a'..=b'z' => Some(ch - b'a' + 26),
        b'0'..=b'9' => Some(ch - b'0' + 52),
        b'+' | b'-' => Some(62),
        b'/' | b'_' => Some(63),
        _ => None,
    }
}

fn b64_decode(input: &str) -> Result<Vec<u8>, &'static str> {
    let bytes = input.trim().as_bytes();
    if bytes.is_empty() {
        return Ok(Vec::new());
    }

    // Strip padding
    let mut len = bytes.len();
    while len > 0 && bytes[len - 1] == b'=' {
        len -= 1;
    }

    let pad_count = bytes.len() - len;
    if pad_count > 2 {
        return Err("invalid base64 padding");
    }

    let mut out = Vec::with_capacity(len * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;

    for &b in &bytes[..len] {
        let val = b64_decode_char(b).ok_or("invalid base64 character")?;
        buf = (buf << 6) | u32::from(val);
        bits += 6;

        if bits >= 8 {
            bits -= 8;
            #[allow(clippy::cast_possible_truncation)]
            out.push((buf >> bits) as u8);
        }
    }

    if bits >= 6 || (bits > 0 && (buf & ((1 << bits) - 1)) != 0) {
        return Err("invalid base64 bits");
    }

    Ok(out)
}

/// `encoding.base64_encode(data)` — encodes Bytes or String to standard Base64.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the input is not a Bytes or String.
pub fn encoding_base64_encode(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "encoding.base64_encode")?;
    let data = extract_input_bytes(&args[0], "encoding.base64_encode")?;
    let encoded = b64_encode(&data, BASE64_STANDARD_TABLE, true);
    Ok(Value::String(Rc::new(encoded)))
}

/// `encoding.base64_decode(str)` — decodes standard Base64 to Bytes.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the argument is not a String.
pub fn encoding_base64_decode(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "encoding.base64_decode")?;
    let text = expect_string(&args[0], "encoding.base64_decode")?;
    match b64_decode(text) {
        Ok(decoded) => Ok(Value::Bytes(Rc::new(RefCell::new((decoded).into())))),
        Err(reason) => Ok(recoverable(reason)),
    }
}

/// `encoding.base64url_encode(data)` — encodes Bytes or String to unpadded URL-safe Base64.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the input is not a Bytes or String.
pub fn encoding_base64url_encode(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "encoding.base64url_encode")?;
    let data = extract_input_bytes(&args[0], "encoding.base64url_encode")?;
    let encoded = b64_encode(&data, BASE64_URL_TABLE, false);
    Ok(Value::String(Rc::new(encoded)))
}

/// `encoding.base64url_decode(str)` — decodes URL-safe Base64 to Bytes.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the argument is not a String.
pub fn encoding_base64url_decode(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "encoding.base64url_decode")?;
    let text = expect_string(&args[0], "encoding.base64url_decode")?;
    match b64_decode(text) {
        Ok(decoded) => Ok(Value::Bytes(Rc::new(RefCell::new((decoded).into())))),
        Err(reason) => Ok(recoverable(reason)),
    }
}

/// `encoding.hex_encode(data)` — encodes Bytes or String to lowercase hexadecimal string.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the input is not a Bytes or String.
pub fn encoding_hex_encode(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "encoding.hex_encode")?;
    let data = extract_input_bytes(&args[0], "encoding.hex_encode")?;
    const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(data.len() * 2);
    for b in data {
        out.push(HEX_CHARS[(b >> 4) as usize] as char);
        out.push(HEX_CHARS[(b & 0x0f) as usize] as char);
    }
    Ok(Value::String(Rc::new(out)))
}

/// `encoding.hex_decode(str)` — decodes hexadecimal string to Bytes.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the argument is not a String.
pub fn encoding_hex_decode(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "encoding.hex_decode")?;
    let text = expect_string(&args[0], "encoding.hex_decode")?.trim();
    if text.len() % 2 != 0 {
        return Ok(recoverable(
            "hex string must have an even number of characters",
        ));
    }

    let mut out = Vec::with_capacity(text.len() / 2);
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let hi = match bytes[i] {
            b'0'..=b'9' => bytes[i] - b'0',
            b'a'..=b'f' => bytes[i] - b'a' + 10,
            b'A'..=b'F' => bytes[i] - b'A' + 10,
            _ => {
                return Ok(recoverable("non-hexadecimal character"));
            }
        };
        let lo = match bytes[i + 1] {
            b'0'..=b'9' => bytes[i + 1] - b'0',
            b'a'..=b'f' => bytes[i + 1] - b'a' + 10,
            b'A'..=b'F' => bytes[i + 1] - b'A' + 10,
            _ => {
                return Ok(recoverable("non-hexadecimal character"));
            }
        };
        out.push((hi << 4) | lo);
        i += 2;
    }

    Ok(Value::Bytes(Rc::new(RefCell::new((out).into()))))
}

/// `encoding.utf8_encode(str)` — encodes String to UTF-8 Bytes.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the argument is not a String.
pub fn encoding_utf8_encode(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "encoding.utf8_encode")?;
    let text = expect_string(&args[0], "encoding.utf8_encode")?;
    Ok(Value::Bytes(Rc::new(RefCell::new(
        (text.as_bytes().to_vec()).into(),
    ))))
}

/// `encoding.utf8_decode(bytes)` — decodes UTF-8 Bytes to String, normalized to NFC.
///
/// Returns a recoverable `Failure` if the bytes are not valid UTF-8.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if the argument is not a Bytes.
pub fn encoding_utf8_decode(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "encoding.utf8_decode")?;
    let Value::Bytes(b) = &args[0] else {
        return Err(VmFault::TypeMismatch {
            expected: "Bytes for encoding.utf8_decode".to_string(),
            actual: args[0].type_name().to_string(),
        });
    };
    let data = b.borrow();
    match std::str::from_utf8(&data) {
        Ok(s) => {
            let nfc_string: String = normalize_nfc_str(s);
            Ok(Value::String(Rc::new(nfc_string)))
        }
        Err(_) => Ok(recoverable("bytes are not valid UTF-8")),
    }
}

/// Constructs the canonical `encoding` module dictionary.
#[must_use]
pub fn create_module() -> Value {
    let entries = vec![
        (
            Value::String(Rc::new("base64_encode".to_string())),
            Value::native("encoding.base64_encode", 1, encoding_base64_encode),
        ),
        (
            Value::String(Rc::new("base64_decode".to_string())),
            Value::native("encoding.base64_decode", 1, encoding_base64_decode),
        ),
        (
            Value::String(Rc::new("base64url_encode".to_string())),
            Value::native("encoding.base64url_encode", 1, encoding_base64url_encode),
        ),
        (
            Value::String(Rc::new("base64url_decode".to_string())),
            Value::native("encoding.base64url_decode", 1, encoding_base64url_decode),
        ),
        (
            Value::String(Rc::new("hex_encode".to_string())),
            Value::native("encoding.hex_encode", 1, encoding_hex_encode),
        ),
        (
            Value::String(Rc::new("hex_decode".to_string())),
            Value::native("encoding.hex_decode", 1, encoding_hex_decode),
        ),
        (
            Value::String(Rc::new("utf8_encode".to_string())),
            Value::native("encoding.utf8_encode", 1, encoding_utf8_encode),
        ),
        (
            Value::String(Rc::new("utf8_decode".to_string())),
            Value::native("encoding.utf8_decode", 1, encoding_utf8_decode),
        ),
    ];
    Value::Dict(Rc::new(RefCell::new(DictMap::from_entries(entries))))
}
