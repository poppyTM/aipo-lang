//! Canonical `Bytes` packing, unpacking, decode and `String.encode` APIs.
//!
//! Storage formats are little-endian (game/binary convention; consistent on VM
//! and JS backends by construction). Out-of-bounds reads/writes fault with
//! `IndexOutOfRange`. Invalid UTF-8 decoding produces a recoverable `Failure`.

use aipo_vm::{FailureValue, Value, Vm, VmFault, check_finite_float, check_safe_int};
use std::cell::RefCell;
use std::rc::Rc;

fn expect_bytes(receiver: &Value) -> Result<Rc<RefCell<aipo_vm::Collection<u8>>>, VmFault> {
    match receiver {
        Value::Bytes(b) => Ok(Rc::clone(b)),
        other => Err(VmFault::TypeMismatch {
            expected: "Bytes receiver".to_string(),
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

fn read_index(arg: &Value, op: &str) -> Result<i64, VmFault> {
    match arg {
        Value::Int(n) => Ok(*n),
        Value::Byte(b) => Ok(i64::from(*b)),
        other => Err(VmFault::TypeMismatch {
            expected: format!("Int index for {op}"),
            actual: other.type_name().to_string(),
        }),
    }
}

fn check_bounds(index: i64, len: usize, size: usize) -> Result<usize, VmFault> {
    let at = resolve_index(index, len)?;
    if at.checked_add(size).is_none_or(|end| end > len) {
        return Err(VmFault::IndexOutOfRange { index, len });
    }
    Ok(at)
}

/// `bytes.len()` — byte count.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if receiver is not Bytes.
pub fn bytes_len(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "bytes.len")?;
    let bytes = expect_bytes(receiver)?;
    #[allow(clippy::cast_possible_wrap)]
    Ok(Value::Int(bytes.borrow().len() as i64))
}

/// `bytes.read_i8(index)` — reads a signed 8-bit integer.
///
/// # Errors
/// Returns `VmFault::IndexOutOfRange` or `VmFault::TypeMismatch`.
pub fn bytes_read_i8(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "bytes.read_i8")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.read_i8")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 1)?;
    Ok(Value::Int(i64::from(b[at] as i8)))
}

/// `bytes.read_u8(index)` — reads an unsigned 8-bit integer as `Byte`.
///
/// # Errors
/// Returns `VmFault::IndexOutOfRange` or `VmFault::TypeMismatch`.
pub fn bytes_read_u8(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "bytes.read_u8")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.read_u8")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 1)?;
    Ok(Value::Byte(b[at]))
}

/// `bytes.read_i16(index)` — reads a signed 16-bit integer (little-endian).
///
/// # Errors
/// Returns `VmFault::IndexOutOfRange` or `VmFault::TypeMismatch`.
pub fn bytes_read_i16(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "bytes.read_i16")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.read_i16")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 2)?;
    let slice: [u8; 2] = [b[at], b[at + 1]];
    Ok(Value::Int(i64::from(i16::from_le_bytes(slice))))
}

/// `bytes.read_u16(index)` — reads an unsigned 16-bit integer (little-endian).
///
/// # Errors
/// Returns `VmFault::IndexOutOfRange` or `VmFault::TypeMismatch`.
pub fn bytes_read_u16(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "bytes.read_u16")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.read_u16")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 2)?;
    let slice: [u8; 2] = [b[at], b[at + 1]];
    Ok(Value::Int(i64::from(u16::from_le_bytes(slice))))
}

/// `bytes.read_i32(index)` — reads a signed 32-bit integer (little-endian).
///
/// # Errors
/// Returns `VmFault::IndexOutOfRange` or `VmFault::TypeMismatch`.
pub fn bytes_read_i32(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "bytes.read_i32")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.read_i32")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 4)?;
    let slice: [u8; 4] = [b[at], b[at + 1], b[at + 2], b[at + 3]];
    Ok(Value::Int(i64::from(i32::from_le_bytes(slice))))
}

/// `bytes.read_u32(index)` — reads an unsigned 32-bit integer (little-endian).
///
/// # Errors
/// Returns `VmFault::IndexOutOfRange` or `VmFault::TypeMismatch`.
pub fn bytes_read_u32(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "bytes.read_u32")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.read_u32")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 4)?;
    let slice: [u8; 4] = [b[at], b[at + 1], b[at + 2], b[at + 3]];
    Ok(Value::Int(i64::from(u32::from_le_bytes(slice))))
}

/// `bytes.read_i64(index)` — reads a signed 64-bit integer (little-endian).
///
/// # Errors
/// Returns `VmFault::IndexOutOfRange` or `VmFault::TypeMismatch`.
pub fn bytes_read_i64(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "bytes.read_i64")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.read_i64")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 8)?;
    let slice: [u8; 8] = [
        b[at],
        b[at + 1],
        b[at + 2],
        b[at + 3],
        b[at + 4],
        b[at + 5],
        b[at + 6],
        b[at + 7],
    ];
    let val = i64::from_le_bytes(slice);
    match check_safe_int(val) {
        Ok(safe) => Ok(Value::Int(safe)),
        Err(_) => Ok(Value::Failure(Rc::new(FailureValue::new(format!(
            "integer {val} outside safe range"
        ))))),
    }
}

/// `bytes.read_u64(index)` — reads an unsigned 64-bit integer (little-endian).
///
/// # Errors
/// Returns `VmFault::IndexOutOfRange` or `VmFault::TypeMismatch`.
pub fn bytes_read_u64(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "bytes.read_u64")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.read_u64")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 8)?;
    let slice: [u8; 8] = [
        b[at],
        b[at + 1],
        b[at + 2],
        b[at + 3],
        b[at + 4],
        b[at + 5],
        b[at + 6],
        b[at + 7],
    ];
    let val = u64::from_le_bytes(slice);
    #[allow(clippy::cast_possible_wrap)]
    if val <= 9_007_199_254_740_991 {
        Ok(Value::Int(val as i64))
    } else {
        Ok(Value::Failure(Rc::new(FailureValue::new(format!(
            "unsigned integer {val} outside safe range"
        )))))
    }
}

/// `bytes.read_f32(index)` — reads a 32-bit IEEE float (little-endian).
///
/// # Errors
/// Returns `VmFault::IndexOutOfRange` or `VmFault::TypeMismatch`.
pub fn bytes_read_f32(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "bytes.read_f32")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.read_f32")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 4)?;
    let slice: [u8; 4] = [b[at], b[at + 1], b[at + 2], b[at + 3]];
    let val = f32::from_le_bytes(slice);
    let finite = check_finite_float(f64::from(val))?;
    Ok(Value::Float(finite))
}

/// `bytes.read_f64(index)` — reads a 64-bit IEEE float (little-endian).
///
/// # Errors
/// Returns `VmFault::IndexOutOfRange` or `VmFault::TypeMismatch`.
pub fn bytes_read_f64(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "bytes.read_f64")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.read_f64")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 8)?;
    let slice: [u8; 8] = [
        b[at],
        b[at + 1],
        b[at + 2],
        b[at + 3],
        b[at + 4],
        b[at + 5],
        b[at + 6],
        b[at + 7],
    ];
    let val = f64::from_le_bytes(slice);
    let finite = check_finite_float(val)?;
    Ok(Value::Float(finite))
}

/// `bytes.write_i8(index, value)` — writes a signed 8-bit integer.
///
/// # Errors
/// Returns `VmFault::IndexOutOfRange` or `VmFault::TypeMismatch`.
pub fn bytes_write_i8(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "bytes.write_i8")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.write_i8")?;
    let val = match &args[1] {
        Value::Int(n) if (-128..=127).contains(n) => *n as i8 as u8,
        Value::Byte(b) => *b,
        Value::Int(n) => {
            return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                "value {n} out of range for i8 (-128..=127)"
            )))));
        }
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int or Byte for bytes.write_i8".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 1)?;
    b[at] = val;
    Ok(Value::None)
}

/// `bytes.write_u8(index, value)` — writes an unsigned 8-bit integer.
///
/// # Errors
/// Returns `VmFault::IndexOutOfRange` or `VmFault::TypeMismatch`.
pub fn bytes_write_u8(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "bytes.write_u8")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.write_u8")?;
    let val = match &args[1] {
        Value::Byte(b) => *b,
        Value::Int(n) if (0..=255).contains(n) => *n as u8,
        Value::Int(n) => {
            return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                "value {n} out of range for u8 (0..=255)"
            )))));
        }
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Byte or Int for bytes.write_u8".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 1)?;
    b[at] = val;
    Ok(Value::None)
}

/// `bytes.write_i16(index, value)` — writes a signed 16-bit integer (little-endian).
///
/// # Errors
/// Returns `VmFault::IndexOutOfRange` or `VmFault::TypeMismatch`.
pub fn bytes_write_i16(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "bytes.write_i16")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.write_i16")?;
    let val = match &args[1] {
        Value::Int(n) if (i64::from(i16::MIN)..=i64::from(i16::MAX)).contains(n) => *n as i16,
        Value::Int(n) => {
            return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                "value {n} out of range for i16"
            )))));
        }
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for bytes.write_i16".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 2)?;
    let bytes_arr = val.to_le_bytes();
    b[at..at + 2].copy_from_slice(&bytes_arr);
    Ok(Value::None)
}

/// `bytes.write_u16(index, value)` — writes an unsigned 16-bit integer (little-endian).
///
/// # Errors
/// Returns `VmFault::IndexOutOfRange` or `VmFault::TypeMismatch`.
pub fn bytes_write_u16(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "bytes.write_u16")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.write_u16")?;
    let val = match &args[1] {
        Value::Int(n) if (0..=i64::from(u16::MAX)).contains(n) => *n as u16,
        Value::Int(n) => {
            return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                "value {n} out of range for u16"
            )))));
        }
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for bytes.write_u16".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 2)?;
    let bytes_arr = val.to_le_bytes();
    b[at..at + 2].copy_from_slice(&bytes_arr);
    Ok(Value::None)
}

/// `bytes.write_i32(index, value)` — writes a signed 32-bit integer (little-endian).
///
/// # Errors
/// Returns `VmFault::IndexOutOfRange` or `VmFault::TypeMismatch`.
pub fn bytes_write_i32(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "bytes.write_i32")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.write_i32")?;
    let val = match &args[1] {
        Value::Int(n) if (i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(n) => *n as i32,
        Value::Int(n) => {
            return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                "value {n} out of range for i32"
            )))));
        }
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for bytes.write_i32".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 4)?;
    let bytes_arr = val.to_le_bytes();
    b[at..at + 4].copy_from_slice(&bytes_arr);
    Ok(Value::None)
}

/// `bytes.write_u32(index, value)` — writes an unsigned 32-bit integer (little-endian).
///
/// # Errors
/// Returns `VmFault::IndexOutOfRange` or `VmFault::TypeMismatch`.
pub fn bytes_write_u32(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "bytes.write_u32")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.write_u32")?;
    let val = match &args[1] {
        Value::Int(n) if (0..=i64::from(u32::MAX)).contains(n) => *n as u32,
        Value::Int(n) => {
            return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                "value {n} out of range for u32"
            )))));
        }
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for bytes.write_u32".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 4)?;
    let bytes_arr = val.to_le_bytes();
    b[at..at + 4].copy_from_slice(&bytes_arr);
    Ok(Value::None)
}

/// `bytes.write_i64(index, value)` — writes a signed 64-bit integer (little-endian).
///
/// # Errors
/// Returns `VmFault::IndexOutOfRange` or `VmFault::TypeMismatch`.
pub fn bytes_write_i64(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "bytes.write_i64")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.write_i64")?;
    let val = match &args[1] {
        Value::Int(n) => *n,
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for bytes.write_i64".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 8)?;
    let bytes_arr = val.to_le_bytes();
    b[at..at + 8].copy_from_slice(&bytes_arr);
    Ok(Value::None)
}

/// `bytes.write_u64(index, value)` — writes an unsigned 64-bit integer (little-endian).
///
/// # Errors
/// Returns `VmFault::IndexOutOfRange` or `VmFault::TypeMismatch`.
pub fn bytes_write_u64(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "bytes.write_u64")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.write_u64")?;
    let val = match &args[1] {
        Value::Int(n) if *n >= 0 => *n as u64,
        Value::Int(n) => {
            return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                "value {n} must be >= 0 for u64"
            )))));
        }
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for bytes.write_u64".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 8)?;
    let bytes_arr = val.to_le_bytes();
    b[at..at + 8].copy_from_slice(&bytes_arr);
    Ok(Value::None)
}

/// `bytes.write_f32(index, value)` — writes a 32-bit IEEE float (little-endian).
///
/// # Errors
/// Returns `VmFault::IndexOutOfRange` or `VmFault::TypeMismatch`.
pub fn bytes_write_f32(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "bytes.write_f32")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.write_f32")?;
    let val = match &args[1] {
        Value::Float(f) => {
            let finite = check_finite_float(*f)?;
            #[allow(clippy::cast_possible_truncation)]
            {
                finite as f32
            }
        }
        Value::Int(n) => *n as f32,
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Float or Int for bytes.write_f32".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 4)?;
    let bytes_arr = val.to_le_bytes();
    b[at..at + 4].copy_from_slice(&bytes_arr);
    Ok(Value::None)
}

/// `bytes.write_f64(index, value)` — writes a 64-bit IEEE float (little-endian).
///
/// # Errors
/// Returns `VmFault::IndexOutOfRange` or `VmFault::TypeMismatch`.
pub fn bytes_write_f64(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "bytes.write_f64")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.write_f64")?;
    let val = match &args[1] {
        Value::Float(f) => check_finite_float(*f)?,
        Value::Int(n) => *n as f64,
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Float or Int for bytes.write_f64".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 8)?;
    let bytes_arr = val.to_le_bytes();
    b[at..at + 8].copy_from_slice(&bytes_arr);
    Ok(Value::None)
}

/// `bytes.decode()` — decodes UTF-8 into a String, or recoverable `Failure`.
/// `bytes.read_i16_be(index)` — reads a signed 16-bit integer (big-endian).
pub fn bytes_read_i16_be(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "bytes.read_i16_be")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.read_i16_be")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 2)?;
    let slice: [u8; 2] = [b[at], b[at + 1]];
    Ok(Value::Int(i64::from(i16::from_be_bytes(slice))))
}

/// `bytes.read_u16_be(index)` — reads an unsigned 16-bit integer (big-endian).
pub fn bytes_read_u16_be(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "bytes.read_u16_be")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.read_u16_be")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 2)?;
    let slice: [u8; 2] = [b[at], b[at + 1]];
    Ok(Value::Int(i64::from(u16::from_be_bytes(slice))))
}

/// `bytes.read_i32_be(index)` — reads a signed 32-bit integer (big-endian).
pub fn bytes_read_i32_be(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "bytes.read_i32_be")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.read_i32_be")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 4)?;
    let slice: [u8; 4] = [b[at], b[at + 1], b[at + 2], b[at + 3]];
    Ok(Value::Int(i64::from(i32::from_be_bytes(slice))))
}

/// `bytes.read_u32_be(index)` — reads an unsigned 32-bit integer (big-endian).
pub fn bytes_read_u32_be(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "bytes.read_u32_be")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.read_u32_be")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 4)?;
    let slice: [u8; 4] = [b[at], b[at + 1], b[at + 2], b[at + 3]];
    Ok(Value::Int(i64::from(u32::from_be_bytes(slice))))
}

/// `bytes.read_i64_be(index)` — reads a signed 64-bit integer (big-endian).
pub fn bytes_read_i64_be(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "bytes.read_i64_be")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.read_i64_be")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 8)?;
    let slice: [u8; 8] = [
        b[at],
        b[at + 1],
        b[at + 2],
        b[at + 3],
        b[at + 4],
        b[at + 5],
        b[at + 6],
        b[at + 7],
    ];
    let val = i64::from_be_bytes(slice);
    match check_safe_int(val) {
        Ok(safe) => Ok(Value::Int(safe)),
        Err(_) => Ok(Value::Failure(Rc::new(FailureValue::new(format!(
            "integer {val} outside safe range"
        ))))),
    }
}

/// `bytes.read_u64_be(index)` — reads an unsigned 64-bit integer (big-endian).
pub fn bytes_read_u64_be(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "bytes.read_u64_be")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.read_u64_be")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 8)?;
    let slice: [u8; 8] = [
        b[at],
        b[at + 1],
        b[at + 2],
        b[at + 3],
        b[at + 4],
        b[at + 5],
        b[at + 6],
        b[at + 7],
    ];
    let val = u64::from_be_bytes(slice);
    #[allow(clippy::cast_possible_wrap)]
    if val <= 9_007_199_254_740_991 {
        Ok(Value::Int(val as i64))
    } else {
        Ok(Value::Failure(Rc::new(FailureValue::new(format!(
            "unsigned integer {val} outside safe range"
        )))))
    }
}

/// `bytes.read_f32_be(index)` — reads a 32-bit float (big-endian).
pub fn bytes_read_f32_be(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "bytes.read_f32_be")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.read_f32_be")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 4)?;
    let slice: [u8; 4] = [b[at], b[at + 1], b[at + 2], b[at + 3]];
    let val = f32::from_be_bytes(slice);
    let finite = check_finite_float(f64::from(val))?;
    Ok(Value::Float(finite))
}

/// `bytes.read_f64_be(index)` — reads a 64-bit float (big-endian).
pub fn bytes_read_f64_be(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 1, "bytes.read_f64_be")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.read_f64_be")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 8)?;
    let slice: [u8; 8] = [
        b[at],
        b[at + 1],
        b[at + 2],
        b[at + 3],
        b[at + 4],
        b[at + 5],
        b[at + 6],
        b[at + 7],
    ];
    let val = f64::from_be_bytes(slice);
    let finite = check_finite_float(val)?;
    Ok(Value::Float(finite))
}

/// `bytes.write_i16_be(index, value)` — writes a signed 16-bit integer (big-endian).
pub fn bytes_write_i16_be(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "bytes.write_i16_be")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.write_i16_be")?;
    let val = match &args[1] {
        Value::Int(n) if (i64::from(i16::MIN)..=i64::from(i16::MAX)).contains(n) => *n as i16,
        Value::Int(n) => {
            return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                "value {n} out of range for i16"
            )))));
        }
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for bytes.write_i16_be".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 2)?;
    b[at..at + 2].copy_from_slice(&val.to_be_bytes());
    Ok(Value::None)
}

/// `bytes.write_u16_be(index, value)` — writes an unsigned 16-bit integer (big-endian).
pub fn bytes_write_u16_be(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "bytes.write_u16_be")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.write_u16_be")?;
    let val = match &args[1] {
        Value::Int(n) if (0..=i64::from(u16::MAX)).contains(n) => *n as u16,
        Value::Int(n) => {
            return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                "value {n} out of range for u16"
            )))));
        }
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for bytes.write_u16_be".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 2)?;
    b[at..at + 2].copy_from_slice(&val.to_be_bytes());
    Ok(Value::None)
}

/// `bytes.write_i32_be(index, value)` — writes a signed 32-bit integer (big-endian).
pub fn bytes_write_i32_be(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "bytes.write_i32_be")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.write_i32_be")?;
    let val = match &args[1] {
        Value::Int(n) if (i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(n) => *n as i32,
        Value::Int(n) => {
            return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                "value {n} out of range for i32"
            )))));
        }
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for bytes.write_i32_be".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 4)?;
    b[at..at + 4].copy_from_slice(&val.to_be_bytes());
    Ok(Value::None)
}

/// `bytes.write_u32_be(index, value)` — writes an unsigned 32-bit integer (big-endian).
pub fn bytes_write_u32_be(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "bytes.write_u32_be")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.write_u32_be")?;
    let val = match &args[1] {
        Value::Int(n) if (0..=4_294_967_295_i64).contains(n) => *n as u32,
        Value::Int(n) => {
            return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                "value {n} out of range for u32"
            )))));
        }
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for bytes.write_u32_be".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 4)?;
    b[at..at + 4].copy_from_slice(&val.to_be_bytes());
    Ok(Value::None)
}

/// `bytes.write_i64_be(index, value)` — writes a signed 64-bit integer (big-endian).
pub fn bytes_write_i64_be(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "bytes.write_i64_be")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.write_i64_be")?;
    let val = match &args[1] {
        Value::Int(n) => *n,
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for bytes.write_i64_be".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 8)?;
    b[at..at + 8].copy_from_slice(&val.to_be_bytes());
    Ok(Value::None)
}

/// `bytes.write_u64_be(index, value)` — writes an unsigned 64-bit integer (big-endian).
pub fn bytes_write_u64_be(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "bytes.write_u64_be")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.write_u64_be")?;
    let val = match &args[1] {
        Value::Int(n) if *n >= 0 => *n as u64,
        Value::Int(n) => {
            return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                "value {n} must be non-negative for u64"
            )))));
        }
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for bytes.write_u64_be".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 8)?;
    b[at..at + 8].copy_from_slice(&val.to_be_bytes());
    Ok(Value::None)
}

/// `bytes.write_f32_be(index, value)` — writes a 32-bit float (big-endian).
pub fn bytes_write_f32_be(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "bytes.write_f32_be")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.write_f32_be")?;
    let val = match &args[1] {
        Value::Float(f) => *f as f32,
        Value::Int(n) => *n as f32,
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Float or Int for bytes.write_f32_be".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 4)?;
    b[at..at + 4].copy_from_slice(&val.to_be_bytes());
    Ok(Value::None)
}

/// `bytes.write_f64_be(index, value)` — writes a 64-bit float (big-endian).
pub fn bytes_write_f64_be(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "bytes.write_f64_be")?;
    let bytes = expect_bytes(receiver)?;
    let index = read_index(&args[0], "bytes.write_f64_be")?;
    let val = match &args[1] {
        Value::Float(f) => *f,
        Value::Int(n) => *n as f64,
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Float or Int for bytes.write_f64_be".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 8)?;
    b[at..at + 8].copy_from_slice(&val.to_be_bytes());
    Ok(Value::None)
}

/// `bytes.slice(start, end)` — returns a new Bytes buffer sliced with tolerant bounds.
pub fn bytes_slice(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "bytes.slice")?;
    let bytes = expect_bytes(receiver)?;
    let start_idx = read_index(&args[0], "bytes.slice")?;
    let end_idx = read_index(&args[1], "bytes.slice")?;
    let b = bytes.borrow();
    let len = b.len();
    #[allow(clippy::cast_possible_wrap)]
    let len_i = len as i64;
    let actual_start = if start_idx < 0 {
        (len_i + start_idx).max(0) as usize
    } else {
        (start_idx as usize).min(len)
    };
    let actual_end = if end_idx < 0 {
        (len_i + end_idx).max(0) as usize
    } else {
        (end_idx as usize).min(len)
    };
    if actual_start >= actual_end {
        Ok(Value::Bytes(Rc::new(RefCell::new((Vec::new()).into()))))
    } else {
        let sub = b[actual_start..actual_end].to_vec();
        Ok(Value::Bytes(Rc::new(RefCell::new((sub).into()))))
    }
}

/// `bytes.decode()` — decodes UTF-8 bytes into a String, returning Failure on error.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if receiver is not Bytes.
pub fn bytes_decode(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "bytes.decode")?;
    let bytes = expect_bytes(receiver)?;
    let b = bytes.borrow();
    match std::str::from_utf8(&b) {
        Ok(s) => Ok(Value::String(Rc::new(s.to_string()))),
        Err(e) => Ok(Value::Failure(Rc::new(FailureValue::new(format!(
            "invalid utf-8: {e}"
        ))))),
    }
}

/// `string.encode()` — encodes UTF-8 String into a new Bytes buffer.
///
/// # Errors
/// Returns `VmFault::TypeMismatch` if receiver is not String.
pub fn string_encode(receiver: &Value, args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 0, "string.encode")?;
    let Value::String(s) = receiver else {
        return Err(VmFault::TypeMismatch {
            expected: "String receiver".to_string(),
            actual: receiver.type_name().to_string(),
        });
    };
    let bytes = s.as_bytes().to_vec();
    Ok(Value::Bytes(Rc::new(RefCell::new((bytes).into()))))
}

/// Registers all Bytes methods and String.encode on the VM.
pub fn register_methods(vm: &mut Vm) {
    vm.register_method_native("Bytes", "len", 0, bytes_len);
    vm.register_method_native("Bytes", "slice", 2, bytes_slice);
    vm.register_method_native("Bytes", "read_i8", 1, bytes_read_i8);
    vm.register_method_native("Bytes", "read_u8", 1, bytes_read_u8);
    vm.register_method_native("Bytes", "read_i16", 1, bytes_read_i16);
    vm.register_method_native("Bytes", "read_i16_le", 1, bytes_read_i16);
    vm.register_method_native("Bytes", "read_i16_be", 1, bytes_read_i16_be);
    vm.register_method_native("Bytes", "read_u16", 1, bytes_read_u16);
    vm.register_method_native("Bytes", "read_u16_le", 1, bytes_read_u16);
    vm.register_method_native("Bytes", "read_u16_be", 1, bytes_read_u16_be);
    vm.register_method_native("Bytes", "read_i32", 1, bytes_read_i32);
    vm.register_method_native("Bytes", "read_i32_le", 1, bytes_read_i32);
    vm.register_method_native("Bytes", "read_i32_be", 1, bytes_read_i32_be);
    vm.register_method_native("Bytes", "read_u32", 1, bytes_read_u32);
    vm.register_method_native("Bytes", "read_u32_le", 1, bytes_read_u32);
    vm.register_method_native("Bytes", "read_u32_be", 1, bytes_read_u32_be);
    vm.register_method_native("Bytes", "read_i64", 1, bytes_read_i64);
    vm.register_method_native("Bytes", "read_i64_le", 1, bytes_read_i64);
    vm.register_method_native("Bytes", "read_i64_be", 1, bytes_read_i64_be);
    vm.register_method_native("Bytes", "read_u64", 1, bytes_read_u64);
    vm.register_method_native("Bytes", "read_u64_le", 1, bytes_read_u64);
    vm.register_method_native("Bytes", "read_u64_be", 1, bytes_read_u64_be);
    vm.register_method_native("Bytes", "read_f32", 1, bytes_read_f32);
    vm.register_method_native("Bytes", "read_f32_le", 1, bytes_read_f32);
    vm.register_method_native("Bytes", "read_f32_be", 1, bytes_read_f32_be);
    vm.register_method_native("Bytes", "read_f64", 1, bytes_read_f64);
    vm.register_method_native("Bytes", "read_f64_le", 1, bytes_read_f64);
    vm.register_method_native("Bytes", "read_f64_be", 1, bytes_read_f64_be);

    vm.register_method_native("Bytes", "write_i8", 2, bytes_write_i8);
    vm.register_method_native("Bytes", "write_u8", 2, bytes_write_u8);
    vm.register_method_native("Bytes", "write_i16", 2, bytes_write_i16);
    vm.register_method_native("Bytes", "write_i16_le", 2, bytes_write_i16);
    vm.register_method_native("Bytes", "write_i16_be", 2, bytes_write_i16_be);
    vm.register_method_native("Bytes", "write_u16", 2, bytes_write_u16);
    vm.register_method_native("Bytes", "write_u16_le", 2, bytes_write_u16);
    vm.register_method_native("Bytes", "write_u16_be", 2, bytes_write_u16_be);
    vm.register_method_native("Bytes", "write_i32", 2, bytes_write_i32);
    vm.register_method_native("Bytes", "write_i32_le", 2, bytes_write_i32);
    vm.register_method_native("Bytes", "write_i32_be", 2, bytes_write_i32_be);
    vm.register_method_native("Bytes", "write_u32", 2, bytes_write_u32);
    vm.register_method_native("Bytes", "write_u32_le", 2, bytes_write_u32);
    vm.register_method_native("Bytes", "write_u32_be", 2, bytes_write_u32_be);
    vm.register_method_native("Bytes", "write_i64", 2, bytes_write_i64);
    vm.register_method_native("Bytes", "write_i64_le", 2, bytes_write_i64);
    vm.register_method_native("Bytes", "write_i64_be", 2, bytes_write_i64_be);
    vm.register_method_native("Bytes", "write_u64", 2, bytes_write_u64);
    vm.register_method_native("Bytes", "write_u64_le", 2, bytes_write_u64);
    vm.register_method_native("Bytes", "write_u64_be", 2, bytes_write_u64_be);
    vm.register_method_native("Bytes", "write_f32", 2, bytes_write_f32);
    vm.register_method_native("Bytes", "write_f32_le", 2, bytes_write_f32);
    vm.register_method_native("Bytes", "write_f32_be", 2, bytes_write_f32_be);
    vm.register_method_native("Bytes", "write_f64", 2, bytes_write_f64);
    vm.register_method_native("Bytes", "write_f64_le", 2, bytes_write_f64);
    vm.register_method_native("Bytes", "write_f64_be", 2, bytes_write_f64_be);

    vm.register_method_native("Bytes", "decode", 0, bytes_decode);
    vm.register_method_native("Bytes", "decode_utf8", 0, bytes_decode);
    vm.register_method_native("String", "encode", 0, string_encode);
    vm.register_method_native("String", "encode_utf8", 0, string_encode);
}
