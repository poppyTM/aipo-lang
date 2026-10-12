//! Canonical `binary` module for Aipo.
//!
//! Provides explicit-endian readers, writers, varints, and slicing over `Bytes`.
//! Endianness is always explicit (e.g. `read_i32_le`, `read_i32_be`).
//! Out-of-bounds access faults with `AIPO_RT_INDEX_OUT_OF_RANGE`.
//! Integers outside safe range or invalid varints return a recoverable `Failure`.

#![forbid(unsafe_code)]

use aipo_vm::{DictMap, FailureValue, Value, VmFault, check_finite_float, check_safe_int};
use std::cell::RefCell;
use std::rc::Rc;

fn expect_bytes(val: &Value, op: &str) -> Result<Rc<RefCell<aipo_vm::Collection<u8>>>, VmFault> {
    match val {
        Value::Bytes(b) => Ok(Rc::clone(b)),
        other => Err(VmFault::TypeMismatch {
            expected: format!("Bytes for {op}"),
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

// --- Readers ---

/// `binary.read_i8(bytes, offset)`
pub fn binary_read_i8(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "binary.read_i8")?;
    let bytes = expect_bytes(&args[0], "binary.read_i8")?;
    let index = read_index(&args[1], "binary.read_i8")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 1)?;
    Ok(Value::Int(i64::from(b[at] as i8)))
}

/// `binary.read_u8(bytes, offset)`
pub fn binary_read_u8(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "binary.read_u8")?;
    let bytes = expect_bytes(&args[0], "binary.read_u8")?;
    let index = read_index(&args[1], "binary.read_u8")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 1)?;
    Ok(Value::Byte(b[at]))
}

/// `binary.read_i16_le(bytes, offset)`
pub fn binary_read_i16_le(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "binary.read_i16_le")?;
    let bytes = expect_bytes(&args[0], "binary.read_i16_le")?;
    let index = read_index(&args[1], "binary.read_i16_le")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 2)?;
    let slice: [u8; 2] = [b[at], b[at + 1]];
    Ok(Value::Int(i64::from(i16::from_le_bytes(slice))))
}

/// `binary.read_i16_be(bytes, offset)`
pub fn binary_read_i16_be(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "binary.read_i16_be")?;
    let bytes = expect_bytes(&args[0], "binary.read_i16_be")?;
    let index = read_index(&args[1], "binary.read_i16_be")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 2)?;
    let slice: [u8; 2] = [b[at], b[at + 1]];
    Ok(Value::Int(i64::from(i16::from_be_bytes(slice))))
}

/// `binary.read_u16_le(bytes, offset)`
pub fn binary_read_u16_le(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "binary.read_u16_le")?;
    let bytes = expect_bytes(&args[0], "binary.read_u16_le")?;
    let index = read_index(&args[1], "binary.read_u16_le")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 2)?;
    let slice: [u8; 2] = [b[at], b[at + 1]];
    Ok(Value::Int(i64::from(u16::from_le_bytes(slice))))
}

/// `binary.read_u16_be(bytes, offset)`
pub fn binary_read_u16_be(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "binary.read_u16_be")?;
    let bytes = expect_bytes(&args[0], "binary.read_u16_be")?;
    let index = read_index(&args[1], "binary.read_u16_be")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 2)?;
    let slice: [u8; 2] = [b[at], b[at + 1]];
    Ok(Value::Int(i64::from(u16::from_be_bytes(slice))))
}

/// `binary.read_i32_le(bytes, offset)`
pub fn binary_read_i32_le(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "binary.read_i32_le")?;
    let bytes = expect_bytes(&args[0], "binary.read_i32_le")?;
    let index = read_index(&args[1], "binary.read_i32_le")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 4)?;
    let slice: [u8; 4] = [b[at], b[at + 1], b[at + 2], b[at + 3]];
    Ok(Value::Int(i64::from(i32::from_le_bytes(slice))))
}

/// `binary.read_i32_be(bytes, offset)`
pub fn binary_read_i32_be(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "binary.read_i32_be")?;
    let bytes = expect_bytes(&args[0], "binary.read_i32_be")?;
    let index = read_index(&args[1], "binary.read_i32_be")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 4)?;
    let slice: [u8; 4] = [b[at], b[at + 1], b[at + 2], b[at + 3]];
    Ok(Value::Int(i64::from(i32::from_be_bytes(slice))))
}

/// `binary.read_u32_le(bytes, offset)`
pub fn binary_read_u32_le(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "binary.read_u32_le")?;
    let bytes = expect_bytes(&args[0], "binary.read_u32_le")?;
    let index = read_index(&args[1], "binary.read_u32_le")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 4)?;
    let slice: [u8; 4] = [b[at], b[at + 1], b[at + 2], b[at + 3]];
    Ok(Value::Int(i64::from(u32::from_le_bytes(slice))))
}

/// `binary.read_u32_be(bytes, offset)`
pub fn binary_read_u32_be(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "binary.read_u32_be")?;
    let bytes = expect_bytes(&args[0], "binary.read_u32_be")?;
    let index = read_index(&args[1], "binary.read_u32_be")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 4)?;
    let slice: [u8; 4] = [b[at], b[at + 1], b[at + 2], b[at + 3]];
    Ok(Value::Int(i64::from(u32::from_be_bytes(slice))))
}

/// `binary.read_i64_le(bytes, offset)`
pub fn binary_read_i64_le(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "binary.read_i64_le")?;
    let bytes = expect_bytes(&args[0], "binary.read_i64_le")?;
    let index = read_index(&args[1], "binary.read_i64_le")?;
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

/// `binary.read_i64_be(bytes, offset)`
pub fn binary_read_i64_be(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "binary.read_i64_be")?;
    let bytes = expect_bytes(&args[0], "binary.read_i64_be")?;
    let index = read_index(&args[1], "binary.read_i64_be")?;
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

/// `binary.read_u64_le(bytes, offset)`
pub fn binary_read_u64_le(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "binary.read_u64_le")?;
    let bytes = expect_bytes(&args[0], "binary.read_u64_le")?;
    let index = read_index(&args[1], "binary.read_u64_le")?;
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

/// `binary.read_u64_be(bytes, offset)`
pub fn binary_read_u64_be(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "binary.read_u64_be")?;
    let bytes = expect_bytes(&args[0], "binary.read_u64_be")?;
    let index = read_index(&args[1], "binary.read_u64_be")?;
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

/// `binary.read_f32_le(bytes, offset)`
pub fn binary_read_f32_le(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "binary.read_f32_le")?;
    let bytes = expect_bytes(&args[0], "binary.read_f32_le")?;
    let index = read_index(&args[1], "binary.read_f32_le")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 4)?;
    let slice: [u8; 4] = [b[at], b[at + 1], b[at + 2], b[at + 3]];
    let val = f32::from_le_bytes(slice);
    let finite = check_finite_float(f64::from(val))?;
    Ok(Value::Float(finite))
}

/// `binary.read_f32_be(bytes, offset)`
pub fn binary_read_f32_be(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "binary.read_f32_be")?;
    let bytes = expect_bytes(&args[0], "binary.read_f32_be")?;
    let index = read_index(&args[1], "binary.read_f32_be")?;
    let b = bytes.borrow();
    let at = check_bounds(index, b.len(), 4)?;
    let slice: [u8; 4] = [b[at], b[at + 1], b[at + 2], b[at + 3]];
    let val = f32::from_be_bytes(slice);
    let finite = check_finite_float(f64::from(val))?;
    Ok(Value::Float(finite))
}

/// `binary.read_f64_le(bytes, offset)`
pub fn binary_read_f64_le(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "binary.read_f64_le")?;
    let bytes = expect_bytes(&args[0], "binary.read_f64_le")?;
    let index = read_index(&args[1], "binary.read_f64_le")?;
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

/// `binary.read_f64_be(bytes, offset)`
pub fn binary_read_f64_be(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "binary.read_f64_be")?;
    let bytes = expect_bytes(&args[0], "binary.read_f64_be")?;
    let index = read_index(&args[1], "binary.read_f64_be")?;
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

// --- Writers ---

/// `binary.write_i8(bytes, offset, value)`
pub fn binary_write_i8(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 3, "binary.write_i8")?;
    let bytes = expect_bytes(&args[0], "binary.write_i8")?;
    let index = read_index(&args[1], "binary.write_i8")?;
    let val = match &args[2] {
        Value::Int(n) if (-128..=127).contains(n) => *n as i8 as u8,
        Value::Byte(b) => *b,
        Value::Int(n) => {
            return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                "value {n} out of range for i8 (-128..=127)"
            )))));
        }
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int or Byte for binary.write_i8".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 1)?;
    b[at] = val;
    Ok(Value::None)
}

/// `binary.write_u8(bytes, offset, value)`
pub fn binary_write_u8(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 3, "binary.write_u8")?;
    let bytes = expect_bytes(&args[0], "binary.write_u8")?;
    let index = read_index(&args[1], "binary.write_u8")?;
    let val = match &args[2] {
        Value::Byte(b) => *b,
        Value::Int(n) if (0..=255).contains(n) => *n as u8,
        Value::Int(n) => {
            return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                "value {n} out of range for u8 (0..=255)"
            )))));
        }
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Byte or Int for binary.write_u8".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 1)?;
    b[at] = val;
    Ok(Value::None)
}

/// `binary.write_i16_le(bytes, offset, value)`
pub fn binary_write_i16_le(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 3, "binary.write_i16_le")?;
    let bytes = expect_bytes(&args[0], "binary.write_i16_le")?;
    let index = read_index(&args[1], "binary.write_i16_le")?;
    let val = match &args[2] {
        Value::Int(n) if (i64::from(i16::MIN)..=i64::from(i16::MAX)).contains(n) => *n as i16,
        Value::Int(n) => {
            return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                "value {n} out of range for i16"
            )))));
        }
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for binary.write_i16_le".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 2)?;
    b[at..at + 2].copy_from_slice(&val.to_le_bytes());
    Ok(Value::None)
}

/// `binary.write_i16_be(bytes, offset, value)`
pub fn binary_write_i16_be(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 3, "binary.write_i16_be")?;
    let bytes = expect_bytes(&args[0], "binary.write_i16_be")?;
    let index = read_index(&args[1], "binary.write_i16_be")?;
    let val = match &args[2] {
        Value::Int(n) if (i64::from(i16::MIN)..=i64::from(i16::MAX)).contains(n) => *n as i16,
        Value::Int(n) => {
            return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                "value {n} out of range for i16"
            )))));
        }
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for binary.write_i16_be".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 2)?;
    b[at..at + 2].copy_from_slice(&val.to_be_bytes());
    Ok(Value::None)
}

/// `binary.write_u16_le(bytes, offset, value)`
pub fn binary_write_u16_le(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 3, "binary.write_u16_le")?;
    let bytes = expect_bytes(&args[0], "binary.write_u16_le")?;
    let index = read_index(&args[1], "binary.write_u16_le")?;
    let val = match &args[2] {
        Value::Int(n) if (0..=65535).contains(n) => *n as u16,
        Value::Int(n) => {
            return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                "value {n} out of range for u16 (0..=65535)"
            )))));
        }
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for binary.write_u16_le".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 2)?;
    b[at..at + 2].copy_from_slice(&val.to_le_bytes());
    Ok(Value::None)
}

/// `binary.write_u16_be(bytes, offset, value)`
pub fn binary_write_u16_be(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 3, "binary.write_u16_be")?;
    let bytes = expect_bytes(&args[0], "binary.write_u16_be")?;
    let index = read_index(&args[1], "binary.write_u16_be")?;
    let val = match &args[2] {
        Value::Int(n) if (0..=65535).contains(n) => *n as u16,
        Value::Int(n) => {
            return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                "value {n} out of range for u16 (0..=65535)"
            )))));
        }
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for binary.write_u16_be".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 2)?;
    b[at..at + 2].copy_from_slice(&val.to_be_bytes());
    Ok(Value::None)
}

/// `binary.write_i32_le(bytes, offset, value)`
pub fn binary_write_i32_le(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 3, "binary.write_i32_le")?;
    let bytes = expect_bytes(&args[0], "binary.write_i32_le")?;
    let index = read_index(&args[1], "binary.write_i32_le")?;
    let val = match &args[2] {
        Value::Int(n) if (i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(n) => *n as i32,
        Value::Int(n) => {
            return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                "value {n} out of range for i32"
            )))));
        }
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for binary.write_i32_le".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 4)?;
    b[at..at + 4].copy_from_slice(&val.to_le_bytes());
    Ok(Value::None)
}

/// `binary.write_i32_be(bytes, offset, value)`
pub fn binary_write_i32_be(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 3, "binary.write_i32_be")?;
    let bytes = expect_bytes(&args[0], "binary.write_i32_be")?;
    let index = read_index(&args[1], "binary.write_i32_be")?;
    let val = match &args[2] {
        Value::Int(n) if (i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(n) => *n as i32,
        Value::Int(n) => {
            return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                "value {n} out of range for i32"
            )))));
        }
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for binary.write_i32_be".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 4)?;
    b[at..at + 4].copy_from_slice(&val.to_be_bytes());
    Ok(Value::None)
}

/// `binary.write_u32_le(bytes, offset, value)`
pub fn binary_write_u32_le(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 3, "binary.write_u32_le")?;
    let bytes = expect_bytes(&args[0], "binary.write_u32_le")?;
    let index = read_index(&args[1], "binary.write_u32_le")?;
    let val = match &args[2] {
        Value::Int(n) if (0..=4_294_967_295_i64).contains(n) => *n as u32,
        Value::Int(n) => {
            return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                "value {n} out of range for u32"
            )))));
        }
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for binary.write_u32_le".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 4)?;
    b[at..at + 4].copy_from_slice(&val.to_le_bytes());
    Ok(Value::None)
}

/// `binary.write_u32_be(bytes, offset, value)`
pub fn binary_write_u32_be(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 3, "binary.write_u32_be")?;
    let bytes = expect_bytes(&args[0], "binary.write_u32_be")?;
    let index = read_index(&args[1], "binary.write_u32_be")?;
    let val = match &args[2] {
        Value::Int(n) if (0..=4_294_967_295_i64).contains(n) => *n as u32,
        Value::Int(n) => {
            return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                "value {n} out of range for u32"
            )))));
        }
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for binary.write_u32_be".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 4)?;
    b[at..at + 4].copy_from_slice(&val.to_be_bytes());
    Ok(Value::None)
}

/// `binary.write_i64_le(bytes, offset, value)`
pub fn binary_write_i64_le(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 3, "binary.write_i64_le")?;
    let bytes = expect_bytes(&args[0], "binary.write_i64_le")?;
    let index = read_index(&args[1], "binary.write_i64_le")?;
    let val = match &args[2] {
        Value::Int(n) => *n,
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for binary.write_i64_le".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 8)?;
    b[at..at + 8].copy_from_slice(&val.to_le_bytes());
    Ok(Value::None)
}

/// `binary.write_i64_be(bytes, offset, value)`
pub fn binary_write_i64_be(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 3, "binary.write_i64_be")?;
    let bytes = expect_bytes(&args[0], "binary.write_i64_be")?;
    let index = read_index(&args[1], "binary.write_i64_be")?;
    let val = match &args[2] {
        Value::Int(n) => *n,
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for binary.write_i64_be".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 8)?;
    b[at..at + 8].copy_from_slice(&val.to_be_bytes());
    Ok(Value::None)
}

/// `binary.write_u64_le(bytes, offset, value)`
pub fn binary_write_u64_le(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 3, "binary.write_u64_le")?;
    let bytes = expect_bytes(&args[0], "binary.write_u64_le")?;
    let index = read_index(&args[1], "binary.write_u64_le")?;
    let val = match &args[2] {
        Value::Int(n) if *n >= 0 => *n as u64,
        Value::Int(n) => {
            return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                "value {n} must be non-negative for u64"
            )))));
        }
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for binary.write_u64_le".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 8)?;
    b[at..at + 8].copy_from_slice(&val.to_le_bytes());
    Ok(Value::None)
}

/// `binary.write_u64_be(bytes, offset, value)`
pub fn binary_write_u64_be(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 3, "binary.write_u64_be")?;
    let bytes = expect_bytes(&args[0], "binary.write_u64_be")?;
    let index = read_index(&args[1], "binary.write_u64_be")?;
    let val = match &args[2] {
        Value::Int(n) if *n >= 0 => *n as u64,
        Value::Int(n) => {
            return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                "value {n} must be non-negative for u64"
            )))));
        }
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Int for binary.write_u64_be".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 8)?;
    b[at..at + 8].copy_from_slice(&val.to_be_bytes());
    Ok(Value::None)
}

/// `binary.write_f32_le(bytes, offset, value)`
pub fn binary_write_f32_le(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 3, "binary.write_f32_le")?;
    let bytes = expect_bytes(&args[0], "binary.write_f32_le")?;
    let index = read_index(&args[1], "binary.write_f32_le")?;
    let val = match &args[2] {
        Value::Float(f) => *f as f32,
        Value::Int(n) => *n as f32,
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Float or Int for binary.write_f32_le".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 4)?;
    b[at..at + 4].copy_from_slice(&val.to_le_bytes());
    Ok(Value::None)
}

/// `binary.write_f32_be(bytes, offset, value)`
pub fn binary_write_f32_be(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 3, "binary.write_f32_be")?;
    let bytes = expect_bytes(&args[0], "binary.write_f32_be")?;
    let index = read_index(&args[1], "binary.write_f32_be")?;
    let val = match &args[2] {
        Value::Float(f) => *f as f32,
        Value::Int(n) => *n as f32,
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Float or Int for binary.write_f32_be".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 4)?;
    b[at..at + 4].copy_from_slice(&val.to_be_bytes());
    Ok(Value::None)
}

/// `binary.write_f64_le(bytes, offset, value)`
pub fn binary_write_f64_le(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 3, "binary.write_f64_le")?;
    let bytes = expect_bytes(&args[0], "binary.write_f64_le")?;
    let index = read_index(&args[1], "binary.write_f64_le")?;
    let val = match &args[2] {
        Value::Float(f) => *f,
        Value::Int(n) => *n as f64,
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Float or Int for binary.write_f64_le".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 8)?;
    b[at..at + 8].copy_from_slice(&val.to_le_bytes());
    Ok(Value::None)
}

/// `binary.write_f64_be(bytes, offset, value)`
pub fn binary_write_f64_be(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 3, "binary.write_f64_be")?;
    let bytes = expect_bytes(&args[0], "binary.write_f64_be")?;
    let index = read_index(&args[1], "binary.write_f64_be")?;
    let val = match &args[2] {
        Value::Float(f) => *f,
        Value::Int(n) => *n as f64,
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "Float or Int for binary.write_f64_be".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };
    let mut b = bytes.borrow_mut();
    let at = check_bounds(index, b.len(), 8)?;
    b[at..at + 8].copy_from_slice(&val.to_be_bytes());
    Ok(Value::None)
}

// --- Varints ---

/// `binary.read_varint(bytes, offset)`
///
/// Decodes an unsigned LEB128 varint. Returns `[value, bytes_read]`, or `Failure` if
/// incomplete or overflow.
pub fn binary_read_varint(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 2, "binary.read_varint")?;
    let bytes = expect_bytes(&args[0], "binary.read_varint")?;
    let index = read_index(&args[1], "binary.read_varint")?;
    let b = bytes.borrow();
    let at = resolve_index(index, b.len())?;

    let mut result: u64 = 0;
    let mut shift: u32 = 0;
    let mut bytes_read: usize = 0;

    while at + bytes_read < b.len() {
        let byte = b[at + bytes_read];
        bytes_read += 1;
        let payload = (byte & 0x7F) as u64;

        if shift >= 64 || (shift == 63 && payload > 1) {
            return Ok(Value::Failure(Rc::new(FailureValue::new(
                "varint overflow: exceeds 64-bit integer".to_string(),
            ))));
        }

        result |= payload << shift;
        if (byte & 0x80) == 0 {
            #[allow(clippy::cast_possible_wrap)]
            if result > 9_007_199_254_740_991 {
                return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                    "varint value {result} exceeds safe integer range"
                )))));
            }
            #[allow(clippy::cast_possible_wrap)]
            let res_int = result as i64;
            #[allow(clippy::cast_possible_wrap)]
            let read_int = bytes_read as i64;
            let pair = vec![Value::Int(res_int), Value::Int(read_int)];
            return Ok(Value::List(Rc::new(RefCell::new((pair).into()))));
        }
        shift += 7;
    }

    Ok(Value::Failure(Rc::new(FailureValue::new(
        "unexpected end of bytes while reading varint".to_string(),
    ))))
}

/// `binary.write_varint(bytes, offset, value)`
///
/// Encodes a non-negative integer as an unsigned LEB128 varint into the Bytes buffer.
/// Returns the number of bytes written, or `Failure` on out of bounds or negative value.
pub fn binary_write_varint(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 3, "binary.write_varint")?;
    let bytes = expect_bytes(&args[0], "binary.write_varint")?;
    let index = read_index(&args[1], "binary.write_varint")?;
    let val = match &args[2] {
        Value::Int(n) if *n >= 0 => *n as u64,
        Value::Int(n) => {
            return Ok(Value::Failure(Rc::new(FailureValue::new(format!(
                "varint value {n} must be non-negative"
            )))));
        }
        other => {
            return Err(VmFault::TypeMismatch {
                expected: "non-negative Int for binary.write_varint".to_string(),
                actual: other.type_name().to_string(),
            });
        }
    };

    let mut b = bytes.borrow_mut();
    let at = resolve_index(index, b.len())?;

    let mut cur = val;
    let mut encoded = [0u8; 10];
    let mut count = 0;
    loop {
        let byte = (cur & 0x7F) as u8;
        cur >>= 7;
        if cur == 0 {
            encoded[count] = byte;
            count += 1;
            break;
        }
        encoded[count] = byte | 0x80;
        count += 1;
    }

    if at + count > b.len() {
        return Err(VmFault::IndexOutOfRange {
            index,
            len: b.len(),
        });
    }

    b[at..at + count].copy_from_slice(&encoded[..count]);
    #[allow(clippy::cast_possible_wrap)]
    Ok(Value::Int(count as i64))
}

/// `binary.slice(bytes, start, end)`
pub fn binary_slice(args: &[Value]) -> Result<Value, VmFault> {
    require_arity(args, 3, "binary.slice")?;
    let bytes = expect_bytes(&args[0], "binary.slice")?;
    let start_idx = read_index(&args[1], "binary.slice")?;
    let end_idx = read_index(&args[2], "binary.slice")?;
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

/// Constructs the canonical `binary` module dictionary.
#[must_use]
pub fn create_module() -> Value {
    let mut entries = Vec::new();
    macro_rules! reg {
        ($name:expr, $arity:expr, $func:expr) => {
            entries.push((
                Value::String(Rc::new($name.to_string())),
                Value::native(concat!("binary.", $name), $arity, $func),
            ));
        };
    }

    reg!("read_i8", 2, binary_read_i8);
    reg!("read_u8", 2, binary_read_u8);
    reg!("read_i16_le", 2, binary_read_i16_le);
    reg!("read_i16_be", 2, binary_read_i16_be);
    reg!("read_u16_le", 2, binary_read_u16_le);
    reg!("read_u16_be", 2, binary_read_u16_be);
    reg!("read_i32_le", 2, binary_read_i32_le);
    reg!("read_i32_be", 2, binary_read_i32_be);
    reg!("read_u32_le", 2, binary_read_u32_le);
    reg!("read_u32_be", 2, binary_read_u32_be);
    reg!("read_i64_le", 2, binary_read_i64_le);
    reg!("read_i64_be", 2, binary_read_i64_be);
    reg!("read_u64_le", 2, binary_read_u64_le);
    reg!("read_u64_be", 2, binary_read_u64_be);
    reg!("read_f32_le", 2, binary_read_f32_le);
    reg!("read_f32_be", 2, binary_read_f32_be);
    reg!("read_f64_le", 2, binary_read_f64_le);
    reg!("read_f64_be", 2, binary_read_f64_be);

    reg!("write_i8", 3, binary_write_i8);
    reg!("write_u8", 3, binary_write_u8);
    reg!("write_i16_le", 3, binary_write_i16_le);
    reg!("write_i16_be", 3, binary_write_i16_be);
    reg!("write_u16_le", 3, binary_write_u16_le);
    reg!("write_u16_be", 3, binary_write_u16_be);
    reg!("write_i32_le", 3, binary_write_i32_le);
    reg!("write_i32_be", 3, binary_write_i32_be);
    reg!("write_u32_le", 3, binary_write_u32_le);
    reg!("write_u32_be", 3, binary_write_u32_be);
    reg!("write_i64_le", 3, binary_write_i64_le);
    reg!("write_i64_be", 3, binary_write_i64_be);
    reg!("write_u64_le", 3, binary_write_u64_le);
    reg!("write_u64_be", 3, binary_write_u64_be);
    reg!("write_f32_le", 3, binary_write_f32_le);
    reg!("write_f32_be", 3, binary_write_f32_be);
    reg!("write_f64_le", 3, binary_write_f64_le);
    reg!("write_f64_be", 3, binary_write_f64_be);

    reg!("read_varint", 2, binary_read_varint);
    reg!("write_varint", 3, binary_write_varint);
    reg!("slice", 3, binary_slice);

    Value::Dict(Rc::new(RefCell::new(DictMap::from_entries(entries))))
}
