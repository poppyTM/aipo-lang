//! Value-model properties: numeric edges, Byte bounds, finiteness, order,
//! copy independence and identity semantics, asserted directly against the
//! `Value` API (no pipeline involved).

#![forbid(unsafe_code)]

use aipo_vm::{MAX_SAFE_INT, MIN_SAFE_INT, Value, check_finite_float, check_safe_int};
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn test_int_edges_and_overflow() {
    assert_eq!(check_safe_int(MAX_SAFE_INT).unwrap(), MAX_SAFE_INT);
    assert_eq!(check_safe_int(MIN_SAFE_INT).unwrap(), MIN_SAFE_INT);
    assert!(check_safe_int(MAX_SAFE_INT + 1).is_err());
    assert!(check_safe_int(MIN_SAFE_INT - 1).is_err());
    assert!(Value::Int(MAX_SAFE_INT).add(&Value::Int(1)).is_err());
    assert!(Value::Int(MIN_SAFE_INT).sub(&Value::Int(1)).is_err());
    assert!(Value::Int(MAX_SAFE_INT).mul(&Value::Int(2)).is_err());
    assert_eq!(
        Value::Int(7).div(&Value::Int(2)).unwrap(),
        Value::Float(3.5)
    );
}

#[test]
fn test_division_by_zero_always_faults() {
    for zero in [Value::Int(0), Value::Float(0.0)] {
        assert!(Value::Int(1).div(&zero).is_err());
        assert!(Value::Int(1).int_div(&zero).is_err());
        assert!(Value::Int(1).modulo(&zero).is_err());
    }
}

#[test]
fn test_non_finite_floats_are_faults_not_values() {
    assert!(check_finite_float(f64::NAN).is_err());
    assert!(check_finite_float(f64::INFINITY).is_err());
    assert!(Value::Float(1.0).add(&Value::Float(f64::INFINITY)).is_err());
    // A NaN smuggled past construction still faults on use, never compares.
    let nan = Value::Float(f64::NAN);
    assert!(nan.add(&Value::Float(1.0)).is_err());
}

#[test]
fn test_byte_identity_and_promotion() {
    assert_eq!(
        Value::Byte(255).add(&Value::Byte(1)).unwrap(),
        Value::Int(256)
    );
    assert_eq!(Value::Byte(2).mul(&Value::Int(3)).unwrap(), Value::Int(6));
    assert_ne!(Value::Byte(1), Value::Int(2));
}

#[test]
fn test_list_order_and_copy_independence() {
    let original = Value::List(Rc::new(RefCell::new(
        (vec![Value::Int(3), Value::Int(1)]).into(),
    )));
    let rendered = format!("{original}");
    assert_eq!(rendered, "[3, 1]");
    if let Value::List(items) = &original {
        items.borrow_mut().push(Value::Int(2));
    }
    assert_eq!(format!("{original}"), "[3, 1, 2]");
}

#[test]
fn test_equality_is_structural_and_total() {
    assert_eq!(Value::Int(1), Value::Float(1.0));
    assert_ne!(Value::Int(1), Value::String(Rc::new("1".to_string())));
    assert_ne!(Value::None, Value::Bool(false));
    assert_eq!(Value::None, Value::None);
}

#[test]
fn test_strict_bool_conditions() {
    assert!(Value::Int(1).as_bool().is_err());
    assert!(Value::None.as_bool().is_err());
    assert_eq!(Value::Bool(true).not().unwrap(), Value::Bool(false));
}

#[test]
fn test_value_size_bounds() {
    // Assert that Value memory footprint is exactly 16 bytes (Marco 2 / ADP-014).
    assert_eq!(std::mem::size_of::<Value>(), 16);
}
