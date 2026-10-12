//! Exhaustive integration and unit tests for aipo-stdlib.
//!
//! Covers Prelude V1, math module, string module, io module,
//! and end-to-end VM execution calling stdlib routines.

use aipo_bytecode::{BytecodeModule, Constant, OpCode};
use aipo_runtime::NativeRegistry;
use aipo_stdlib::{
    binary, collections, convert, encoding, io, log, math, path, prelude, regex, register_stdlib,
    string, testing, time, url,
};
use aipo_vm::{FailureValue, TypeTag, Value, Vm};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

fn range_global_module() -> BytecodeModule {
    let mut module = BytecodeModule::new();
    module.names = vec!["Range".to_string()];
    module.code = vec![OpCode::GetGlobal as u8, 0, 0, OpCode::Return as u8];
    module
}

#[test]
fn test_prelude_binds_range_as_type_value() {
    let mut vm = Vm::new();
    let mut registry = NativeRegistry::new();
    register_stdlib(&mut vm, &mut registry);

    // §5.1 (check vs run): `Range` is accepted by `aipo check` but was missing as a
    // runtime global, so `is Range` / `Range(..)` failed at run time with "undefined
    // global variable 'Range'". It is now bound as a type value, matching `Bool`/`List`
    // (usable with `is`, not a constructor).
    assert_eq!(vm.get_global("Range"), Some(&Value::Type(TypeTag::Range)));
    let result = vm
        .run(&range_global_module())
        .expect("Range global resolves at run time");
    assert_eq!(result, Value::Type(TypeTag::Range));
}

#[test]
fn test_prelude_len() {
    // String
    let str_val = Value::String(Rc::new("café🚀".to_string()));
    let len_res = prelude::native_len(&[str_val]).unwrap();
    assert_eq!(len_res, Value::Int(5));

    // List
    let list_val = Value::List(Rc::new(RefCell::new(vec![
        Value::Int(1),
        Value::Int(2),
        Value::Int(3),
    ])));
    let len_res = prelude::native_len(&[list_val]).unwrap();
    assert_eq!(len_res, Value::Int(3));

    // Dict
    let dict_val = Value::Dict(Rc::new(RefCell::new(
        [
            (Value::Int(1), Value::String(Rc::new("a".to_string()))),
            (Value::Int(2), Value::String(Rc::new("b".to_string()))),
        ]
        .into_iter()
        .collect(),
    )));
    let len_res = prelude::native_len(&[dict_val]).unwrap();
    assert_eq!(len_res, Value::Int(2));

    // Failure propagation
    let fail_val = Value::Failure(Rc::new(FailureValue::new("bad".to_string())));
    let len_res = prelude::native_len(std::slice::from_ref(&fail_val)).unwrap();
    assert_eq!(len_res, fail_val);

    // Invalid type
    let err = prelude::native_len(&[Value::Int(42)]).unwrap_err();
    assert!(matches!(err, aipo_vm::VmFault::TypeMismatch { .. }));
}

#[test]
fn test_prelude_copy() {
    // List shallow copy: modification to clone does not mutate original
    let orig_list = Value::List(Rc::new(RefCell::new(vec![Value::Int(1), Value::Int(2)])));
    let copy_val = prelude::native_copy(std::slice::from_ref(&orig_list)).unwrap();

    assert_eq!(orig_list, copy_val);
    if let Value::List(cloned_rc) = &copy_val {
        cloned_rc.borrow_mut().push(Value::Int(3));
    }
    if let Value::List(orig_rc) = &orig_list {
        assert_eq!(orig_rc.borrow().len(), 2);
    }
    if let Value::List(cloned_rc) = &copy_val {
        assert_eq!(cloned_rc.borrow().len(), 3);
    }

    // Scalar copy is identity
    let int_val = Value::Int(42);
    assert_eq!(prelude::native_copy(&[int_val]).unwrap(), Value::Int(42));
}

#[test]
fn test_prelude_same() {
    let list_a = Value::List(Rc::new(RefCell::new(vec![Value::Int(1)])));
    let list_b = Value::List(Rc::new(RefCell::new(vec![Value::Int(1)])));
    let list_c = list_a.clone();

    // Two distinct lists with same contents have distinct identities
    assert_eq!(
        prelude::native_same(&[list_a.clone(), list_b]).unwrap(),
        Value::Bool(false)
    );

    // Same reference
    assert_eq!(
        prelude::native_same(&[list_a, list_c]).unwrap(),
        Value::Bool(true)
    );

    // Scalar equality
    assert_eq!(
        prelude::native_same(&[Value::Int(5), Value::Int(5)]).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        prelude::native_same(&[Value::Int(5), Value::Int(6)]).unwrap(),
        Value::Bool(false)
    );
}

#[test]
fn test_prelude_some() {
    assert_eq!(
        prelude::native_some(&[Value::None]).unwrap(),
        Value::Bool(false)
    );
    assert_eq!(
        prelude::native_some(&[Value::Int(0)]).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        prelude::native_some(&[Value::Bool(false)]).unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn test_prelude_fail() {
    let res =
        prelude::native_fail(&[Value::String(Rc::new("database offline".to_string()))]).unwrap();
    match res {
        Value::Failure(f) => assert_eq!(f.message, "database offline"),
        other => panic!("expected Failure, got {other:?}"),
    }
}

#[test]
fn test_math_abs() {
    assert_eq!(math::math_abs(&[Value::Int(-42)]).unwrap(), Value::Int(42));
    assert_eq!(math::math_abs(&[Value::Int(42)]).unwrap(), Value::Int(42));
    assert_eq!(
        math::math_abs(&[Value::Float(-3.5)]).unwrap(),
        Value::Float(3.5)
    );
}

#[test]
fn test_math_min_max() {
    assert_eq!(
        math::math_min(&[Value::Int(10), Value::Int(20)]).unwrap(),
        Value::Int(10)
    );
    assert_eq!(
        math::math_max(&[Value::Int(10), Value::Int(20)]).unwrap(),
        Value::Int(20)
    );
    assert_eq!(
        math::math_min(&[Value::Int(10), Value::Float(5.5)]).unwrap(),
        Value::Float(5.5)
    );
    assert_eq!(
        math::math_max(&[Value::Int(10), Value::Float(5.5)]).unwrap(),
        Value::Float(10.0)
    );
}

#[test]
fn test_math_floor_ceil_round() {
    assert_eq!(
        math::math_floor(&[Value::Float(3.7)]).unwrap(),
        Value::Int(3)
    );
    assert_eq!(
        math::math_ceil(&[Value::Float(3.2)]).unwrap(),
        Value::Int(4)
    );
    assert_eq!(
        math::math_round(&[Value::Float(3.4)]).unwrap(),
        Value::Int(3)
    );
    assert_eq!(
        math::math_round(&[Value::Float(3.5)]).unwrap(),
        Value::Int(4)
    );
}

#[test]
fn test_math_sqrt() {
    assert_eq!(
        math::math_sqrt(&[Value::Int(16)]).unwrap(),
        Value::Float(4.0)
    );
    assert_eq!(
        math::math_sqrt(&[Value::Float(25.0)]).unwrap(),
        Value::Float(5.0)
    );

    // Negative sqrt produces recoverable Failure
    let neg_res = math::math_sqrt(&[Value::Int(-1)]).unwrap();
    assert!(neg_res.is_failure());
}

#[test]
fn test_math_pow() {
    assert_eq!(
        math::math_pow(&[Value::Int(2), Value::Int(8)]).unwrap(),
        Value::Int(256)
    );
    assert_eq!(
        math::math_pow(&[Value::Float(4.0), Value::Float(0.5)]).unwrap(),
        Value::Float(2.0)
    );
}

#[test]
fn test_math_constants() {
    let mut vm = Vm::new();
    let mut registry = NativeRegistry::new();
    register_stdlib(&mut vm, &mut registry);

    let math_val = vm.get_global("math").expect("math global bound");
    if let Value::Dict(dict) = math_val {
        let map = dict.borrow();
        let tau_key = Value::String(Rc::new("tau".to_string()));
        let pi_key = Value::String(Rc::new("pi".to_string()));
        let e_key = Value::String(Rc::new("e".to_string()));
        assert_eq!(
            map.get(&tau_key),
            Some(&Value::Float(std::f64::consts::TAU))
        );
        assert_eq!(map.get(&pi_key), Some(&Value::Float(std::f64::consts::PI)));
        assert_eq!(map.get(&e_key), Some(&Value::Float(std::f64::consts::E)));
    } else {
        panic!("math global is not a Dict");
    }
}

#[test]
fn test_string_operations() {
    let s = Value::String(Rc::new("  Hello, Aipo World!  ".to_string()));

    // Trim
    let trimmed = string::string_trim(&[s]).unwrap();
    assert_eq!(
        trimmed,
        Value::String(Rc::new("Hello, Aipo World!".to_string()))
    );

    // Lower and Upper
    let lower = string::string_lower(std::slice::from_ref(&trimmed)).unwrap();
    assert_eq!(
        lower,
        Value::String(Rc::new("hello, aipo world!".to_string()))
    );

    let upper = string::string_upper(std::slice::from_ref(&trimmed)).unwrap();
    assert_eq!(
        upper,
        Value::String(Rc::new("HELLO, AIPO WORLD!".to_string()))
    );

    // Contains, StartsWith, EndsWith
    let needle = Value::String(Rc::new("Aipo".to_string()));
    assert_eq!(
        string::string_contains(&[trimmed.clone(), needle.clone()]).unwrap(),
        Value::Bool(true)
    );

    let prefix = Value::String(Rc::new("Hello".to_string()));
    assert_eq!(
        string::string_starts_with(&[trimmed.clone(), prefix]).unwrap(),
        Value::Bool(true)
    );

    let suffix = Value::String(Rc::new("World!".to_string()));
    assert_eq!(
        string::string_ends_with(&[trimmed.clone(), suffix]).unwrap(),
        Value::Bool(true)
    );

    // Replace
    let replaced = string::string_replace(&[
        trimmed,
        Value::String(Rc::new("World".to_string())),
        Value::String(Rc::new("Universe".to_string())),
    ])
    .unwrap();
    assert_eq!(
        replaced,
        Value::String(Rc::new("Hello, Aipo Universe!".to_string()))
    );

    // Split and Join
    let csv = Value::String(Rc::new("a,b,c".to_string()));
    let comma = Value::String(Rc::new(",".to_string()));
    let split_res = string::string_split(&[csv, comma.clone()]).unwrap();
    if let Value::List(parts) = &split_res {
        assert_eq!(parts.borrow().len(), 3);
    } else {
        panic!("expected List");
    }
    let joined =
        string::string_join(&[Value::String(Rc::new(" - ".to_string())), split_res]).unwrap();
    assert_eq!(joined, Value::String(Rc::new("a - b - c".to_string())));

    // Slice
    let orig = Value::String(Rc::new("0123456789".to_string()));
    let sliced = string::string_slice(&[orig.clone(), Value::Int(2), Value::Int(5)]).unwrap();
    assert_eq!(sliced, Value::String(Rc::new("234".to_string())));

    // Negative indexing in slice
    let sliced_tail = string::string_slice(&[orig, Value::Int(-3), Value::Int(10)]).unwrap();
    assert_eq!(sliced_tail, Value::String(Rc::new("789".to_string())));
}

/// Canon makes NFC an invariant of `String` and puts normalization at the construction
/// boundaries, so every operation that can join a base character with a combining mark
/// normalizes its output (ADP-001 Q5, closed by `P00-G15`).
#[test]
fn test_string_operations_preserve_nfc() {
    let mark = Value::String(Rc::new("\u{0301}".to_string()));
    let composed = Value::String(Rc::new("é".to_string()));

    // `join` brings the mark next to a base character.
    let joined = string::string_join(&[
        Value::String(Rc::new(String::new())),
        Value::List(Rc::new(RefCell::new(vec![
            Value::String(Rc::new("e".to_string())),
            mark.clone(),
        ]))),
    ])
    .unwrap();
    assert_eq!(joined, composed, "join must store the composed character");

    // `replace` and `format` alike.
    let replaced = string::string_replace(&[
        Value::String(Rc::new("e.".to_string())),
        Value::String(Rc::new(".".to_string())),
        mark.clone(),
    ])
    .unwrap();
    assert_eq!(replaced, composed);

    let formatted = string::string_format(&[
        Value::String(Rc::new("e{v}".to_string())),
        dict_with(vec![("v", mark)]),
    ])
    .unwrap();
    assert_eq!(formatted, composed);

    // A substring keeps the invariant for free: removing characters never makes two
    // previously non-adjacent characters adjacent.
    let sliced = string::string_slice(&[composed.clone(), Value::Int(0), Value::Int(1)]).unwrap();
    assert_eq!(sliced, composed);

    // A mark with no precomposed form is left alone rather than dropped: `e` + two combining
    // acutes normalizes to the composed pair plus one surviving mark.
    let doubled = string::string_format(&[
        Value::String(Rc::new("e{v}".to_string())),
        dict_with(vec![(
            "v",
            Value::String(Rc::new("\u{0301}\u{0301}".to_string())),
        )]),
    ])
    .unwrap();
    assert_eq!(
        string::string_len(&[doubled]).unwrap(),
        Value::Int(2),
        "normalization composes what it can and keeps the rest"
    );
}

#[test]
fn test_string_byte_len_and_len() {
    // "café🚀": 5 code points, 1 + 1 + 1 + 2 + 4 = 9 UTF-8 bytes.
    let text = Value::String(Rc::new("café🚀".to_string()));
    assert_eq!(
        string::string_len(std::slice::from_ref(&text)).unwrap(),
        Value::Int(5)
    );
    assert_eq!(
        string::string_byte_len(std::slice::from_ref(&text)).unwrap(),
        Value::Int(9)
    );
    assert_eq!(
        string::string_byte_len(&[Value::String(Rc::new(String::new()))]).unwrap(),
        Value::Int(0)
    );
}

#[test]
fn test_string_find() {
    let text = Value::String(Rc::new("café🚀café".to_string()));

    // Index is measured in code points, not bytes.
    let rocket = Value::String(Rc::new("🚀".to_string()));
    assert_eq!(
        string::string_find(&[text.clone(), rocket]).unwrap(),
        Value::Int(4)
    );

    // Empty substring is found at index 0 (canon edge case).
    assert_eq!(
        string::string_find(&[text.clone(), Value::String(Rc::new(String::new()))]).unwrap(),
        Value::Int(0)
    );

    // Missing substring yields none.
    assert_eq!(
        string::string_find(&[text, Value::String(Rc::new("zzz".to_string()))]).unwrap(),
        Value::None
    );
}

#[test]
fn test_string_capitalize() {
    assert_eq!(
        string::string_capitalize(&[Value::String(Rc::new("hELLO world".to_string()))]).unwrap(),
        Value::String(Rc::new("Hello world".to_string()))
    );

    // Leading non-cased characters are preserved.
    assert_eq!(
        string::string_capitalize(&[Value::String(Rc::new("123abc".to_string()))]).unwrap(),
        Value::String(Rc::new("123Abc".to_string()))
    );

    // Text without any cased character is unchanged (canon edge case).
    assert_eq!(
        string::string_capitalize(&[Value::String(Rc::new("123".to_string()))]).unwrap(),
        Value::String(Rc::new("123".to_string()))
    );

    assert_eq!(
        string::string_capitalize(&[Value::String(Rc::new(String::new()))]).unwrap(),
        Value::String(Rc::new(String::new()))
    );
}

#[test]
fn test_string_reverse_graphemes() {
    // "e\u{301}" is a single extended grapheme cluster (e + combining acute).
    // Reversing keeps the cluster intact and the result is re-normalized to NFC,
    // so the trailing cluster comes back as the precomposed "é".
    let composed = Value::String(Rc::new("e\u{301}ab".to_string()));
    let reversed = string::string_reverse(&[composed]).unwrap();
    assert_eq!(reversed, Value::String(Rc::new("baé".to_string())));

    // Emoji with variation selector stays a single visual unit.
    let flag = Value::String(Rc::new("🇧🇷".to_string()));
    let reversed = string::string_reverse(std::slice::from_ref(&flag)).unwrap();
    assert_eq!(reversed, flag);

    assert_eq!(
        string::string_reverse(&[Value::String(Rc::new(String::new()))]).unwrap(),
        Value::String(Rc::new(String::new()))
    );
}

fn dict_with(entries: Vec<(&str, Value)>) -> Value {
    Value::Dict(Rc::new(RefCell::new(
        entries
            .into_iter()
            .map(|(k, v)| (Value::String(Rc::new(k.to_string())), v))
            .collect(),
    )))
}

#[test]
fn test_string_format() {
    let template = Value::String(Rc::new("idade: {age}, nome: {name}".to_string()));
    let values = dict_with(vec![
        ("age", Value::Int(25)),
        ("name", Value::String(Rc::new("Ada".to_string()))),
    ]);

    let rendered = string::string_format(&[template, values.clone()]).unwrap();
    assert_eq!(
        rendered,
        Value::String(Rc::new("idade: 25, nome: Ada".to_string()))
    );

    // Templates without placeholders are returned unchanged.
    let plain = Value::String(Rc::new("sem placeholders".to_string()));
    assert_eq!(
        string::string_format(&[plain, values.clone()]).unwrap(),
        Value::String(Rc::new("sem placeholders".to_string()))
    );

    // Extra keys are allowed and ignored.
    let only_age = Value::String(Rc::new("age={age}".to_string()));
    assert_eq!(
        string::string_format(&[only_age, values.clone()]).unwrap(),
        Value::String(Rc::new("age=25".to_string()))
    );

    // Escaped braces produce literal braces.
    let escaped = Value::String(Rc::new("{{age}} {age}".to_string()));
    assert_eq!(
        string::string_format(&[escaped, values.clone()]).unwrap(),
        Value::String(Rc::new("{age} 25".to_string()))
    );

    // Missing key is a recoverable Failure.
    let missing = Value::String(Rc::new("{absent}".to_string()));
    assert!(
        string::string_format(&[missing, values.clone()])
            .unwrap()
            .is_failure()
    );

    // Malformed template is a recoverable Failure.
    let unclosed = Value::String(Rc::new("{age".to_string()));
    assert!(
        string::string_format(&[unclosed, values.clone()])
            .unwrap()
            .is_failure()
    );

    // Positional placeholders are outside V1 and produce a recoverable Failure.
    let positional = Value::String(Rc::new("{0}".to_string()));
    assert!(
        string::string_format(&[positional, values.clone()])
            .unwrap()
            .is_failure()
    );

    // values must be a Dict.
    let err = string::string_format(&[Value::String(Rc::new("{age}".to_string())), Value::Int(1)])
        .unwrap_err();
    assert!(matches!(err, aipo_vm::VmFault::TypeMismatch { .. }));
}

#[test]
fn test_string_canon_argument_rules() {
    // split with an empty separator is invalid in V1.
    let err = string::string_split(&[
        Value::String(Rc::new("abc".to_string())),
        Value::String(Rc::new(String::new())),
    ])
    .unwrap_err();
    assert!(matches!(err, aipo_vm::VmFault::TypeMismatch { .. }));

    // split preserves empty fields, including at the edges.
    let parts = string::string_split(&[
        Value::String(Rc::new(",a,b,".to_string())),
        Value::String(Rc::new(",".to_string())),
    ])
    .unwrap();
    assert_eq!(
        parts,
        Value::List(Rc::new(RefCell::new(vec![
            Value::String(Rc::new(String::new())),
            Value::String(Rc::new("a".to_string())),
            Value::String(Rc::new("b".to_string())),
            Value::String(Rc::new(String::new())),
        ])))
    );

    let empty_parts = string::string_split(&[
        Value::String(Rc::new(String::new())),
        Value::String(Rc::new(",".to_string())),
    ])
    .unwrap();
    assert_eq!(
        empty_parts,
        Value::List(Rc::new(RefCell::new(vec![Value::String(Rc::new(
            String::new()
        ))])))
    );

    // replace with an empty pattern is invalid in V1.
    let err = string::string_replace(&[
        Value::String(Rc::new("abc".to_string())),
        Value::String(Rc::new(String::new())),
        Value::String(Rc::new("x".to_string())),
    ])
    .unwrap_err();
    assert!(matches!(err, aipo_vm::VmFault::TypeMismatch { .. }));
}

#[test]
fn test_string_join_canon_rules() {
    let comma = Value::String(Rc::new(",".to_string()));

    // join(separator, []) == ""
    let empty = string::string_join(&[
        comma.clone(),
        Value::List(Rc::new(RefCell::new(Vec::new()))),
    ])
    .unwrap();
    assert_eq!(empty, Value::String(Rc::new(String::new())));

    // A single element returns that element.
    let single = string::string_join(&[
        comma.clone(),
        Value::List(Rc::new(RefCell::new(vec![Value::String(Rc::new(
            "only".to_string(),
        ))]))),
    ])
    .unwrap();
    assert_eq!(single, Value::String(Rc::new("only".to_string())));

    // Elements must already be String: no implicit coercion.
    let err = string::string_join(&[
        comma,
        Value::List(Rc::new(RefCell::new(vec![Value::Int(1)]))),
    ])
    .unwrap_err();
    assert!(matches!(err, aipo_vm::VmFault::TypeMismatch { .. }));
}

#[test]
fn test_math_truncate() {
    assert_eq!(
        math::math_truncate(&[Value::Float(3.9)]).unwrap(),
        Value::Int(3)
    );
    assert_eq!(
        math::math_truncate(&[Value::Float(-3.9)]).unwrap(),
        Value::Int(-3)
    );
    assert_eq!(
        math::math_truncate(&[Value::Int(7)]).unwrap(),
        Value::Int(7)
    );
}

#[test]
fn test_math_clamp() {
    assert_eq!(
        math::math_clamp(&[Value::Int(15), Value::Int(0), Value::Int(10)]).unwrap(),
        Value::Int(10)
    );
    assert_eq!(
        math::math_clamp(&[Value::Int(-5), Value::Int(0), Value::Int(10)]).unwrap(),
        Value::Int(0)
    );
    assert_eq!(
        math::math_clamp(&[Value::Int(5), Value::Int(0), Value::Int(10)]).unwrap(),
        Value::Int(5)
    );

    // Mixed operands follow Int -> Float promotion.
    assert_eq!(
        math::math_clamp(&[Value::Float(7.5), Value::Int(0), Value::Float(5.5)]).unwrap(),
        Value::Float(5.5)
    );

    // Inverted bounds are a value outside a valid range, so they produce a recoverable
    // Failure (ADP-001 Q3, closed by `P00-G15`).
    let inverted = math::math_clamp(&[Value::Int(5), Value::Int(10), Value::Int(0)]).unwrap();
    assert!(inverted.is_failure());
}

#[test]
fn test_math_round_half_away_from_zero() {
    assert_eq!(
        math::math_round(&[Value::Float(2.5)]).unwrap(),
        Value::Int(3)
    );
    assert_eq!(
        math::math_round(&[Value::Float(-2.5)]).unwrap(),
        Value::Int(-3)
    );
}

#[test]
fn test_convert_int() {
    assert_eq!(
        convert::convert_int(&[Value::Int(42)]).unwrap(),
        Value::Int(42)
    );
    // Float truncates toward zero.
    assert_eq!(
        convert::convert_int(&[Value::Float(3.8)]).unwrap(),
        Value::Int(3)
    );
    assert_eq!(
        convert::convert_int(&[Value::Float(-3.8)]).unwrap(),
        Value::Int(-3)
    );
    // String parses the whole text.
    assert_eq!(
        convert::convert_int(&[Value::String(Rc::new("8080".to_string()))]).unwrap(),
        Value::Int(8080)
    );
    // Invalid or out-of-range text produces a recoverable Failure.
    assert!(
        convert::convert_int(&[Value::String(Rc::new("abc".to_string()))])
            .unwrap()
            .is_failure()
    );
    assert!(
        convert::convert_int(&[Value::String(Rc::new("4.5".to_string()))])
            .unwrap()
            .is_failure()
    );
    assert!(
        convert::convert_int(&[Value::String(Rc::new("999999999999999999999".to_string()))])
            .unwrap()
            .is_failure()
    );
    // Unsupported category is a fault, not a Failure.
    let err = convert::convert_int(&[Value::Bool(true)]).unwrap_err();
    assert!(matches!(err, aipo_vm::VmFault::TypeMismatch { .. }));
}

#[test]
fn test_convert_float() {
    assert_eq!(
        convert::convert_float(&[Value::Float(1.5)]).unwrap(),
        Value::Float(1.5)
    );
    assert_eq!(
        convert::convert_float(&[Value::Int(10)]).unwrap(),
        Value::Float(10.0)
    );
    // Canon example: Float("4.5") or_else 1.0
    assert_eq!(
        convert::convert_float(&[Value::String(Rc::new("4.5".to_string()))]).unwrap(),
        Value::Float(4.5)
    );
    assert!(
        convert::convert_float(&[Value::String(Rc::new("x".to_string()))])
            .unwrap()
            .is_failure()
    );
}

#[test]
fn test_convert_byte() {
    // Canon example: Byte("255") or_else Byte(255)
    assert_eq!(
        convert::convert_byte(&[Value::String(Rc::new("255".to_string()))]).unwrap(),
        Value::Int(255)
    );
    assert_eq!(
        convert::convert_byte(&[Value::Int(255)]).unwrap(),
        Value::Int(255)
    );
    assert_eq!(
        convert::convert_byte(&[Value::Float(0.0)]).unwrap(),
        Value::Int(0)
    );

    // Out of range never wraps around or saturates.
    assert!(
        convert::convert_byte(&[Value::Int(256)])
            .unwrap()
            .is_failure()
    );
    assert!(
        convert::convert_byte(&[Value::Int(-1)])
            .unwrap()
            .is_failure()
    );
    assert!(
        convert::convert_byte(&[Value::String(Rc::new("256".to_string()))])
            .unwrap()
            .is_failure()
    );
    assert!(
        convert::convert_byte(&[Value::Float(1.5)])
            .unwrap()
            .is_failure()
    );
}

#[test]
fn test_convert_bytes() {
    // Canon example: `let data = Bytes(32)` is a mutable zero-filled block.
    let bytes = convert::convert_bytes(&[Value::Int(32)]).unwrap();
    let Value::Bytes(buffer) = &bytes else {
        panic!("Bytes(32) must produce a Bytes value, got {bytes:?}");
    };
    assert_eq!(buffer.borrow().len(), 32);
    assert!(buffer.borrow().iter().all(|byte| *byte == 0));

    // Zero is a valid (empty) block, and negative or oversized counts are recoverable.
    let empty = convert::convert_bytes(&[Value::Int(0)]).unwrap();
    assert_eq!(empty, Value::Bytes(Rc::new(RefCell::new(Vec::new()))));
    assert!(
        convert::convert_bytes(&[Value::Int(-1)])
            .unwrap()
            .is_failure()
    );
    assert!(
        convert::convert_bytes(&[Value::Int(i64::MAX)])
            .unwrap()
            .is_failure()
    );

    // Only `Int` is accepted; other categories are a fault.
    let err = convert::convert_bytes(&[Value::String(Rc::new("32".to_string()))]).unwrap_err();
    assert!(matches!(err, aipo_vm::VmFault::TypeMismatch { .. }));
}

#[test]
fn test_convert_string() {
    assert_eq!(
        convert::convert_string(&[Value::String(Rc::new("abc".to_string()))]).unwrap(),
        Value::String(Rc::new("abc".to_string()))
    );
    // Canon example: String(port)
    assert_eq!(
        convert::convert_string(&[Value::Int(8080)]).unwrap(),
        Value::String(Rc::new("8080".to_string()))
    );
    assert_eq!(
        convert::convert_string(&[Value::Float(1.5)]).unwrap(),
        Value::String(Rc::new("1.5".to_string()))
    );
    assert_eq!(
        convert::convert_string(&[Value::Float(2.0)]).unwrap(),
        Value::String(Rc::new("2.0".to_string()))
    );
    assert_eq!(
        convert::convert_string(&[Value::Bool(false)]).unwrap(),
        Value::String(Rc::new("false".to_string()))
    );
    assert_eq!(
        convert::convert_string(&[Value::None]).unwrap(),
        Value::String(Rc::new("none".to_string()))
    );

    // No magic stringification for collections.
    let list = Value::List(Rc::new(RefCell::new(vec![Value::Int(1)])));
    let err = convert::convert_string(&[list]).unwrap_err();
    assert!(matches!(err, aipo_vm::VmFault::TypeMismatch { .. }));
}

#[test]
fn test_vm_execution_with_prelude_conversion() {
    let mut vm = Vm::new();
    let mut registry = NativeRegistry::new();
    register_stdlib(&mut vm, &mut registry);

    // Bytecode: GetGlobal "Int"; Constant "8080"; Call 1; Return
    let mut module = BytecodeModule::new();
    module.names.push("Int".to_string());
    let c0 = module.constants.len() as u16;
    module.constants.push(Constant::String("8080".to_string()));

    module.code.push(OpCode::GetGlobal as u8);
    module.code.extend_from_slice(&0u16.to_be_bytes());
    module.code.push(OpCode::Constant as u8);
    module.code.extend_from_slice(&c0.to_be_bytes());
    module.code.push(OpCode::Call as u8);
    module.code.push(1);
    module.code.push(OpCode::Return as u8);

    let res = vm.run(&module).unwrap();
    assert_eq!(res, Value::Int(8080));
}

#[test]
fn test_registry_covers_new_surface() {
    let mut vm = Vm::new();
    let mut registry = NativeRegistry::new();
    register_stdlib(&mut vm, &mut registry);

    for name in [
        "Int", "Float", "Byte", "String", "len", "copy", "some", "fail",
    ] {
        assert!(registry.get(None, name).is_some(), "missing prelude {name}");
    }

    for name in ["truncate", "clamp"] {
        assert!(
            registry.get(Some("math"), name).is_some(),
            "missing math.{name}"
        );
    }

    for name in ["byte_len", "find", "capitalize", "reverse", "format"] {
        assert!(
            registry.get(Some("string"), name).is_some(),
            "missing string.{name}"
        );
    }
}

#[derive(Default, Clone)]
struct BufferSink(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for BufferSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn test_io_sink_capture() {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let sink = BufferSink(Arc::clone(&buf));
    io::set_output_sink(Some(Box::new(sink)));

    io::io_print(&[Value::String(Rc::new("Hello".to_string()))]).unwrap();
    io::io_println(&[Value::String(Rc::new(" World".to_string()))]).unwrap();

    let output = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
    assert_eq!(output, "Hello World\n");

    // Clean up sink
    io::set_output_sink(None);
}

#[test]
fn test_vm_execution_with_stdlib_prelude() {
    let mut vm = Vm::new();
    let mut registry = NativeRegistry::new();
    register_stdlib(&mut vm, &mut registry);

    // Bytecode program:
    // 1. GetGlobal "len"
    // 2. Constant "apple"
    // 3. Constant "banana"
    // 4. BuildList 2
    // 5. Call 1
    // 6. Return
    let mut module = BytecodeModule::new();
    module.names.push("len".to_string());
    #[allow(clippy::cast_possible_truncation)]
    let c0 = module.constants.len() as u16;
    module.constants.push(Constant::String("apple".to_string()));
    #[allow(clippy::cast_possible_truncation)]
    let c1 = module.constants.len() as u16;
    module
        .constants
        .push(Constant::String("banana".to_string()));

    // GetGlobal "len" (name index 0)
    module.code.push(OpCode::GetGlobal as u8);
    module.code.extend_from_slice(&0u16.to_be_bytes());

    // Push constant "apple"
    module.code.push(OpCode::Constant as u8);
    module.code.extend_from_slice(&c0.to_be_bytes());

    // Push constant "banana"
    module.code.push(OpCode::Constant as u8);
    module.code.extend_from_slice(&c1.to_be_bytes());

    // BuildList 2
    module.code.push(OpCode::BuildList as u8);
    module.code.extend_from_slice(&2u16.to_be_bytes());

    // Call 1
    module.code.push(OpCode::Call as u8);
    module.code.push(1);

    // Return
    module.code.push(OpCode::Return as u8);

    let res = vm.run(&module).unwrap();
    assert_eq!(res, Value::Int(2));
}

#[test]
fn test_vm_execution_with_stdlib_math_module() {
    let mut vm = Vm::new();
    let mut registry = NativeRegistry::new();
    register_stdlib(&mut vm, &mut registry);

    // Bytecode program:
    // 1. GetGlobal "math"
    // 2. GetField "abs"
    // 3. Constant -99
    // 4. Call 1
    // 5. Return
    let mut module = BytecodeModule::new();
    module.names.push("math".to_string());
    module.names.push("abs".to_string());
    #[allow(clippy::cast_possible_truncation)]
    let c0 = module.constants.len() as u16;
    module.constants.push(Constant::Int(-99));

    // GetGlobal "math" (name index 0)
    module.code.push(OpCode::GetGlobal as u8);
    module.code.extend_from_slice(&0u16.to_be_bytes());

    // GetField "abs" (name index 1)
    module.code.push(OpCode::GetField as u8);
    module.code.extend_from_slice(&1u16.to_be_bytes());

    // Constant -99
    module.code.push(OpCode::Constant as u8);
    module.code.extend_from_slice(&c0.to_be_bytes());

    // Call 1
    module.code.push(OpCode::Call as u8);
    module.code.push(1);

    // Return
    module.code.push(OpCode::Return as u8);

    let res = vm.run(&module).unwrap();
    assert_eq!(res, Value::Int(99));
}

#[test]
fn test_math_trigonometry_and_logs() {
    // sin, cos, tan
    let zero = Value::Int(0);
    assert_eq!(
        math::math_sin(std::slice::from_ref(&zero)).unwrap(),
        Value::Float(0.0)
    );
    assert_eq!(
        math::math_cos(std::slice::from_ref(&zero)).unwrap(),
        Value::Float(1.0)
    );
    assert_eq!(
        math::math_tan(std::slice::from_ref(&zero)).unwrap(),
        Value::Float(0.0)
    );

    // asin, acos, atan
    let one = Value::Int(1);
    assert_eq!(
        math::math_asin(std::slice::from_ref(&zero)).unwrap(),
        Value::Float(0.0)
    );
    assert_eq!(
        math::math_acos(std::slice::from_ref(&one)).unwrap(),
        Value::Float(0.0)
    );
    assert_eq!(
        math::math_atan(std::slice::from_ref(&zero)).unwrap(),
        Value::Float(0.0)
    );

    // asin / acos domain errors
    let out_of_bounds = Value::Float(1.5);
    assert!(
        math::math_asin(std::slice::from_ref(&out_of_bounds))
            .unwrap()
            .is_failure()
    );
    assert!(
        math::math_acos(std::slice::from_ref(&out_of_bounds))
            .unwrap()
            .is_failure()
    );

    // atan2, hypot
    let three = Value::Float(3.0);
    let four = Value::Float(4.0);
    assert_eq!(math::math_hypot(&[three, four]).unwrap(), Value::Float(5.0));
    assert_eq!(
        math::math_atan2(&[Value::Float(0.0), Value::Float(1.0)]).unwrap(),
        Value::Float(0.0)
    );

    // log, log2, log10, exp
    let e_val = Value::Float(std::f64::consts::E);
    let ln_e = math::math_log(&[e_val]).unwrap();
    if let Value::Float(f) = ln_e {
        assert!((f - 1.0).abs() < 1e-10);
    } else {
        panic!("expected float");
    }

    let eight = Value::Int(8);
    assert_eq!(math::math_log2(&[eight]).unwrap(), Value::Float(3.0));

    let hundred = Value::Int(100);
    assert_eq!(math::math_log10(&[hundred]).unwrap(), Value::Float(2.0));

    assert_eq!(
        math::math_exp(std::slice::from_ref(&zero)).unwrap(),
        Value::Float(1.0)
    );

    // log domain error
    let neg = Value::Int(-5);
    assert!(
        math::math_log(std::slice::from_ref(&neg))
            .unwrap()
            .is_failure()
    );
    assert!(
        math::math_log2(std::slice::from_ref(&neg))
            .unwrap()
            .is_failure()
    );
    assert!(math::math_log10(&[neg]).unwrap().is_failure());
    assert!(
        math::math_log(std::slice::from_ref(&zero))
            .unwrap()
            .is_failure()
    );

    // sign
    assert_eq!(math::math_sign(&[Value::Int(42)]).unwrap(), Value::Int(1));
    assert_eq!(math::math_sign(&[Value::Int(-42)]).unwrap(), Value::Int(-1));
    assert_eq!(math::math_sign(&[Value::Int(0)]).unwrap(), Value::Int(0));
    assert_eq!(
        math::math_sign(&[Value::Float(3.5)]).unwrap(),
        Value::Float(1.0)
    );
    assert_eq!(
        math::math_sign(&[Value::Float(-3.5)]).unwrap(),
        Value::Float(-1.0)
    );
    assert_eq!(
        math::math_sign(&[Value::Float(0.0)]).unwrap(),
        Value::Float(0.0)
    );

    // rad & deg
    let deg180 = Value::Float(180.0);
    let rad_pi = math::math_rad(&[deg180]).unwrap();
    if let Value::Float(f) = rad_pi {
        assert!((f - std::f64::consts::PI).abs() < 1e-10);
    } else {
        panic!("expected float");
    }

    let deg_result = math::math_deg(&[Value::Float(std::f64::consts::PI)]).unwrap();
    if let Value::Float(f) = deg_result {
        assert!((f - 180.0).abs() < 1e-10);
    } else {
        panic!("expected float");
    }
}

#[test]
fn test_deterministic_random_prng() {
    use aipo_stdlib::random;

    let rng1 = random::random_create(&[Value::Int(12345)]).unwrap();
    let rng2 = random::random_create(&[Value::Int(12345)]).unwrap();

    let int_args = vec![Value::Int(1), Value::Int(100)];
    let val1 = random::method_rng_int(&rng1, &int_args).unwrap();
    let val2 = random::method_rng_int(&rng2, &int_args).unwrap();
    assert_eq!(val1, val2);

    let f1 = random::method_rng_float(&rng1, &[]).unwrap();
    let f2 = random::method_rng_float(&rng2, &[]).unwrap();
    assert_eq!(f1, f2);
    if let Value::Float(f) = f1 {
        assert!((0.0..1.0).contains(&f));
    } else {
        panic!("expected float");
    }

    let b1 = random::method_rng_bool(&rng1, &[]).unwrap();
    let b2 = random::method_rng_bool(&rng2, &[]).unwrap();
    assert_eq!(b1, b2);

    let list = Value::List(Rc::new(RefCell::new(vec![
        Value::Int(10),
        Value::Int(20),
        Value::Int(30),
    ])));
    let c1 = random::method_rng_choice(&rng1, std::slice::from_ref(&list)).unwrap();
    let c2 = random::method_rng_choice(&rng2, std::slice::from_ref(&list)).unwrap();
    assert_eq!(c1, c2);

    let s1 = random::method_rng_shuffle(&rng1, std::slice::from_ref(&list)).unwrap();
    let s2 = random::method_rng_shuffle(&rng2, &[list]).unwrap();
    assert_eq!(s1, s2);

    // Empty list choice fails
    let empty_list = Value::List(Rc::new(RefCell::new(vec![])));
    assert!(
        random::method_rng_choice(&rng1, &[empty_list])
            .unwrap()
            .is_failure()
    );

    // Inverted bounds fail
    let inv_args = vec![Value::Int(100), Value::Int(1)];
    assert!(
        random::method_rng_int(&rng1, &inv_args)
            .unwrap()
            .is_failure()
    );
}

#[test]
fn test_json_module() {
    use aipo_stdlib::json;

    // Parse valid JSON
    let sample = r#"{"name": "Aipo", "version": 1, "features": ["fast", "safe"], "pi": 3.14, "enabled": true, "nothing": null}"#;
    let parsed = json::json_parse(&[Value::String(Rc::new(sample.to_string()))]).unwrap();
    if let Value::Dict(d) = &parsed {
        assert_eq!(d.borrow().len(), 6);
        let name_key = Value::String(Rc::new("name".to_string()));
        assert_eq!(
            d.borrow().get(&name_key),
            Some(&Value::String(Rc::new("Aipo".to_string())))
        );
        let ver_key = Value::String(Rc::new("version".to_string()));
        assert_eq!(d.borrow().get(&ver_key), Some(&Value::Int(1)));
        let nothing_key = Value::String(Rc::new("nothing".to_string()));
        assert_eq!(d.borrow().get(&nothing_key), Some(&Value::None));
    } else {
        panic!("expected dict");
    }

    // Parse rejects duplicate keys
    let dup_json = r#"{"a": 1, "a": 2}"#;
    let dup_res = json::json_parse(&[Value::String(Rc::new(dup_json.to_string()))]).unwrap();
    assert!(dup_res.is_failure());

    // Parse rejects syntax errors
    let bad_json = r#"{"a": 1"#;
    let bad_res = json::json_parse(&[Value::String(Rc::new(bad_json.to_string()))]).unwrap();
    assert!(bad_res.is_failure());

    // Stringify simple values
    let s = json::json_stringify(std::slice::from_ref(&parsed)).unwrap();
    if let Value::String(text) = s {
        assert!(text.contains("\"name\": \"Aipo\""));
        assert!(text.contains("\"version\": 1"));
    } else {
        panic!("expected string");
    }

    // Stringify pretty
    let s_pretty = json::json_stringify(&[parsed, Value::Bool(true)]).unwrap();
    if let Value::String(text) = s_pretty {
        assert!(text.contains('\n'));
        assert!(text.contains("  \"name\": \"Aipo\""));
    } else {
        panic!("expected string");
    }

    // Cycle detection in stringify
    let cyclic_list = Rc::new(RefCell::new(Vec::new()));
    cyclic_list
        .borrow_mut()
        .push(Value::List(Rc::clone(&cyclic_list)));
    let cycle_res = json::json_stringify(&[Value::List(cyclic_list)]).unwrap();
    assert!(cycle_res.is_failure());
}

#[test]
fn test_list_first_or_last_or() {
    let empty_list = Value::List(Rc::new(RefCell::new(Vec::new())));
    let non_empty = Value::List(Rc::new(RefCell::new(vec![
        Value::Int(10),
        Value::Int(20),
        Value::Int(30),
    ])));
    let default_val = Value::Int(-1);

    // Empty list
    assert_eq!(
        collections::list_first_or(&empty_list, std::slice::from_ref(&default_val)).unwrap(),
        Value::Int(-1)
    );
    assert_eq!(
        collections::list_last_or(&empty_list, std::slice::from_ref(&default_val)).unwrap(),
        Value::Int(-1)
    );

    // Non-empty list
    assert_eq!(
        collections::list_first_or(&non_empty, std::slice::from_ref(&default_val)).unwrap(),
        Value::Int(10)
    );
    assert_eq!(
        collections::list_last_or(&non_empty, std::slice::from_ref(&default_val)).unwrap(),
        Value::Int(30)
    );

    // find_index
    assert_eq!(
        collections::list_find_index(&non_empty, &[Value::Int(20)]).unwrap(),
        Value::Int(1)
    );
    assert_eq!(
        collections::list_find_index(&non_empty, &[Value::Int(99)]).unwrap(),
        Value::None
    );
}

#[test]
fn test_encoding_module() {
    let hello = Value::String(Rc::new("Hello, Aipo!".to_string()));

    // Base64 encode & decode
    let b64 = encoding::encoding_base64_encode(std::slice::from_ref(&hello)).unwrap();
    assert_eq!(b64, Value::String(Rc::new("SGVsbG8sIEFpcG8h".to_string())));

    let dec_bytes = encoding::encoding_base64_decode(std::slice::from_ref(&b64)).unwrap();
    if let Value::Bytes(b) = &dec_bytes {
        assert_eq!(*b.borrow(), b"Hello, Aipo!");
    } else {
        panic!("expected bytes from base64_decode");
    }

    // Invalid base64 produces Failure
    let bad_b64 = Value::String(Rc::new("!invalid base64!".to_string()));
    let bad_res = encoding::encoding_base64_decode(std::slice::from_ref(&bad_b64)).unwrap();
    assert!(bad_res.is_failure());

    // Base64URL encode & decode
    let b64url = encoding::encoding_base64url_encode(std::slice::from_ref(&hello)).unwrap();
    assert_eq!(
        b64url,
        Value::String(Rc::new("SGVsbG8sIEFpcG8h".to_string()))
    );

    let dec_url = encoding::encoding_base64url_decode(std::slice::from_ref(&b64url)).unwrap();
    if let Value::Bytes(b) = &dec_url {
        assert_eq!(*b.borrow(), b"Hello, Aipo!");
    } else {
        panic!("expected bytes from base64url_decode");
    }

    // Hex encode & decode
    let hex = encoding::encoding_hex_encode(std::slice::from_ref(&hello)).unwrap();
    assert_eq!(
        hex,
        Value::String(Rc::new("48656c6c6f2c204169706f21".to_string()))
    );

    let dec_hex = encoding::encoding_hex_decode(std::slice::from_ref(&hex)).unwrap();
    if let Value::Bytes(b) = &dec_hex {
        assert_eq!(*b.borrow(), b"Hello, Aipo!");
    } else {
        panic!("expected bytes from hex_decode");
    }

    // Invalid hex produces Failure
    let odd_hex = Value::String(Rc::new("123".to_string()));
    assert!(
        encoding::encoding_hex_decode(std::slice::from_ref(&odd_hex))
            .unwrap()
            .is_failure()
    );
    let non_hex = Value::String(Rc::new("123g".to_string()));
    assert!(
        encoding::encoding_hex_decode(std::slice::from_ref(&non_hex))
            .unwrap()
            .is_failure()
    );

    // UTF-8 encode & decode
    let utf8_bytes = encoding::encoding_utf8_encode(std::slice::from_ref(&hello)).unwrap();
    let utf8_str = encoding::encoding_utf8_decode(std::slice::from_ref(&utf8_bytes)).unwrap();
    assert_eq!(utf8_str, hello);

    // Invalid UTF-8 produces Failure
    let invalid_utf8 = Value::Bytes(Rc::new(RefCell::new(vec![0xff, 0xfe])));
    assert!(
        encoding::encoding_utf8_decode(std::slice::from_ref(&invalid_utf8))
            .unwrap()
            .is_failure()
    );
}

#[test]
fn test_path_module() {
    // is_absolute
    let abs_unix = Value::String(Rc::new("/usr/local/bin".to_string()));
    let rel_unix = Value::String(Rc::new("src/main.aipo".to_string()));
    assert_eq!(
        path::path_is_absolute(std::slice::from_ref(&abs_unix)).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        path::path_is_absolute(std::slice::from_ref(&rel_unix)).unwrap(),
        Value::Bool(false)
    );

    // normalize
    let unnormalized = Value::String(Rc::new("a/b/../c/./d/".to_string()));
    assert_eq!(
        path::path_normalize(std::slice::from_ref(&unnormalized)).unwrap(),
        Value::String(Rc::new("a/c/d/".to_string()))
    );

    // join
    let part1 = Value::String(Rc::new("config".to_string()));
    let part2 = Value::String(Rc::new("app.json".to_string()));
    assert_eq!(
        path::path_join(&[part1, part2]).unwrap(),
        Value::String(Rc::new("config/app.json".to_string()))
    );

    // basename & dirname & ext
    let full = Value::String(Rc::new("/home/user/project/main.aipo".to_string()));
    assert_eq!(
        path::path_basename(std::slice::from_ref(&full)).unwrap(),
        Value::String(Rc::new("main.aipo".to_string()))
    );
    assert_eq!(
        path::path_basename(&[full.clone(), Value::String(Rc::new(".aipo".to_string()))]).unwrap(),
        Value::String(Rc::new("main".to_string()))
    );
    assert_eq!(
        path::path_dirname(std::slice::from_ref(&full)).unwrap(),
        Value::String(Rc::new("/home/user/project".to_string()))
    );
    assert_eq!(
        path::path_ext(std::slice::from_ref(&full)).unwrap(),
        Value::String(Rc::new(".aipo".to_string()))
    );
}

#[test]
fn test_string_human_facing_operations() {
    // graphemes
    let s = Value::String(Rc::new("café".to_string()));
    let g = string::string_graphemes(&[s]).unwrap();
    if let Value::List(items) = g {
        let borrowed = items.borrow();
        assert_eq!(borrowed.len(), 4);
        assert_eq!(borrowed[0], Value::String(Rc::new("c".to_string())));
        assert_eq!(borrowed[1], Value::String(Rc::new("a".to_string())));
        assert_eq!(borrowed[2], Value::String(Rc::new("f".to_string())));
        assert_eq!(borrowed[3], Value::String(Rc::new("é".to_string())));
    } else {
        panic!("expected List");
    }

    // words
    let sentence = Value::String(Rc::new("Hello, world! 123".to_string()));
    let w = string::string_words(&[sentence]).unwrap();
    if let Value::List(items) = w {
        let borrowed = items.borrow();
        assert_eq!(borrowed.len(), 3);
        assert_eq!(borrowed[0], Value::String(Rc::new("Hello".to_string())));
        assert_eq!(borrowed[1], Value::String(Rc::new("world".to_string())));
        assert_eq!(borrowed[2], Value::String(Rc::new("123".to_string())));
    } else {
        panic!("expected List");
    }

    // lines
    let multiline = Value::String(Rc::new("line1\r\nline2\nline3\n".to_string()));
    let l = string::string_lines(&[multiline]).unwrap();
    if let Value::List(items) = l {
        let borrowed = items.borrow();
        assert_eq!(borrowed.len(), 3);
        assert_eq!(borrowed[0], Value::String(Rc::new("line1".to_string())));
        assert_eq!(borrowed[1], Value::String(Rc::new("line2".to_string())));
        assert_eq!(borrowed[2], Value::String(Rc::new("line3".to_string())));
    } else {
        panic!("expected List");
    }

    // casefold
    let upper = Value::String(Rc::new("CAFÉ".to_string()));
    assert_eq!(
        string::string_casefold(&[upper]).unwrap(),
        Value::String(Rc::new("café".to_string()))
    );
}

#[test]
fn test_url_module() {
    let url_str = Value::String(Rc::new(
        "https://user:secret@example.com:8080/api/v1?page=2#results".to_string(),
    ));
    let parsed = url::url_parse(&[url_str]).unwrap();
    if let Value::Dict(map) = parsed {
        let borrowed = map.borrow();
        let get = |k: &str| -> String {
            if let Some(Value::String(s)) = borrowed.get(&Value::String(Rc::new(k.to_string()))) {
                s.to_string()
            } else {
                panic!("missing or non-string key {k}");
            }
        };

        assert_eq!(get("protocol"), "https:");
        assert_eq!(get("username"), "user");
        assert_eq!(get("password"), "secret");
        assert_eq!(get("host"), "example.com:8080");
        assert_eq!(get("hostname"), "example.com");
        assert_eq!(get("port"), "8080");
        assert_eq!(get("pathname"), "/api/v1");
        assert_eq!(get("search"), "?page=2");
        assert_eq!(get("hash"), "#results");
        assert_eq!(get("origin"), "https://example.com:8080");
        assert_eq!(
            get("href"),
            "https://user:secret@example.com:8080/api/v1?page=2#results"
        );
    } else {
        panic!("expected Dict");
    }

    // Default port stripped
    let def_port = Value::String(Rc::new("https://example.com:443/home".to_string()));
    let parsed_def = url::url_parse(&[def_port]).unwrap();
    if let Value::Dict(map) = parsed_def {
        let borrowed = map.borrow();
        let get = |k: &str| -> String {
            if let Some(Value::String(s)) = borrowed.get(&Value::String(Rc::new(k.to_string()))) {
                s.to_string()
            } else {
                panic!("missing key {k}");
            }
        };
        assert_eq!(get("port"), "");
        assert_eq!(get("host"), "example.com");
        assert_eq!(get("href"), "https://example.com/home");
    }

    // Invalid URL -> Failure
    let invalid = Value::String(Rc::new("not_a_valid_url".to_string()));
    let res = url::url_parse(&[invalid]).unwrap();
    assert!(matches!(res, Value::Failure(_)));
}

#[test]
fn test_eager_list_methods() {
    let list = Value::List(Rc::new(RefCell::new(vec![
        Value::Int(1),
        Value::Int(2),
        Value::Int(2),
        Value::Int(3),
        Value::Int(4),
    ])));

    // take
    let taken = collections::list_take(&list, &[Value::Int(3)]).unwrap();
    if let Value::List(items) = taken {
        assert_eq!(
            *items.borrow(),
            vec![Value::Int(1), Value::Int(2), Value::Int(2)]
        );
    } else {
        panic!("expected List");
    }

    // skip
    let skipped = collections::list_skip(&list, &[Value::Int(3)]).unwrap();
    if let Value::List(items) = skipped {
        assert_eq!(*items.borrow(), vec![Value::Int(3), Value::Int(4)]);
    } else {
        panic!("expected List");
    }

    // distinct
    let distinct = collections::list_distinct(&list, &[]).unwrap();
    if let Value::List(items) = distinct {
        assert_eq!(
            *items.borrow(),
            vec![Value::Int(1), Value::Int(2), Value::Int(3), Value::Int(4)]
        );
    } else {
        panic!("expected List");
    }

    // zip
    let other = Value::List(Rc::new(RefCell::new(vec![
        Value::String(Rc::new("a".to_string())),
        Value::String(Rc::new("b".to_string())),
    ])));
    let zipped = collections::list_zip(&list, &[other]).unwrap();
    if let Value::List(items) = zipped {
        let b = items.borrow();
        assert_eq!(b.len(), 2);
    } else {
        panic!("expected List");
    }

    // chain
    let chained = collections::list_chain(
        &list,
        &[Value::List(Rc::new(RefCell::new(vec![Value::Int(5)])))],
    )
    .unwrap();
    if let Value::List(items) = chained {
        assert_eq!(items.borrow().len(), 6);
    } else {
        panic!("expected List");
    }

    // chunk
    let chunked = collections::list_chunk(&list, &[Value::Int(2)]).unwrap();
    if let Value::List(items) = chunked {
        assert_eq!(items.borrow().len(), 3);
    } else {
        panic!("expected List");
    }

    // window
    let windowed = collections::list_window(&list, &[Value::Int(3)]).unwrap();
    if let Value::List(items) = windowed {
        assert_eq!(items.borrow().len(), 3);
    } else {
        panic!("expected List");
    }

    // enumerate
    let enumerated = collections::list_enumerate(&list, &[]).unwrap();
    if let Value::List(items) = enumerated {
        let b = items.borrow();
        assert_eq!(b.len(), 5);
        if let Value::List(pair) = &b[0] {
            assert_eq!(*pair.borrow(), vec![Value::Int(0), Value::Int(1)]);
        }
    } else {
        panic!("expected List");
    }
}

#[test]
fn test_regex_module() {
    let pat_str = Value::String(Rc::new("^[a-z0-9_]+$".to_string()));
    let pattern = regex::regex_compile(&[pat_str]).unwrap();
    assert!(matches!(pattern, Value::Struct(_)));

    let good_text = Value::String(Rc::new("hello_world_123".to_string()));
    let bad_text = Value::String(Rc::new("hello world!".to_string()));

    assert_eq!(
        regex::method_pattern_is_match(&pattern, &[good_text]).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        regex::method_pattern_is_match(&pattern, &[bad_text]).unwrap(),
        Value::Bool(false)
    );

    let digits_pattern =
        regex::regex_compile(&[Value::String(Rc::new(r"\d+".to_string()))]).unwrap();
    let text = Value::String(Rc::new("a123b456c".to_string()));

    // find
    assert_eq!(
        regex::method_pattern_find(&digits_pattern, std::slice::from_ref(&text)).unwrap(),
        Value::String(Rc::new("123".to_string()))
    );

    // find_all
    let all = regex::method_pattern_find_all(&digits_pattern, std::slice::from_ref(&text)).unwrap();
    if let Value::List(items) = all {
        assert_eq!(
            *items.borrow(),
            vec![
                Value::String(Rc::new("123".to_string())),
                Value::String(Rc::new("456".to_string())),
            ]
        );
    } else {
        panic!("expected List");
    }

    // replace
    let replaced = regex::method_pattern_replace(
        &digits_pattern,
        &[text.clone(), Value::String(Rc::new("#".to_string()))],
    )
    .unwrap();
    assert_eq!(replaced, Value::String(Rc::new("a#b#c".to_string())));

    // split
    let split = regex::method_pattern_split(&digits_pattern, &[text]).unwrap();
    if let Value::List(items) = split {
        assert_eq!(
            *items.borrow(),
            vec![
                Value::String(Rc::new("a".to_string())),
                Value::String(Rc::new("b".to_string())),
                Value::String(Rc::new("c".to_string())),
            ]
        );
    } else {
        panic!("expected List");
    }

    // invalid pattern -> Failure
    let invalid = regex::regex_compile(&[Value::String(Rc::new("[invalid".to_string()))]).unwrap();
    assert!(matches!(invalid, Value::Failure(_)));
}

#[test]
fn test_binary_module_operations() {
    let buf = Value::Bytes(Rc::new(RefCell::new(vec![0u8; 32])));

    // i8 & u8
    binary::binary_write_i8(&[buf.clone(), Value::Int(0), Value::Int(-120)]).unwrap();
    assert_eq!(
        binary::binary_read_i8(&[buf.clone(), Value::Int(0)]).unwrap(),
        Value::Int(-120)
    );
    binary::binary_write_u8(&[buf.clone(), Value::Int(1), Value::Int(240)]).unwrap();
    assert_eq!(
        binary::binary_read_u8(&[buf.clone(), Value::Int(1)]).unwrap(),
        Value::Int(240)
    );

    // i16 LE & BE
    binary::binary_write_i16_le(&[buf.clone(), Value::Int(2), Value::Int(-1234)]).unwrap();
    assert_eq!(
        binary::binary_read_i16_le(&[buf.clone(), Value::Int(2)]).unwrap(),
        Value::Int(-1234)
    );
    binary::binary_write_i16_be(&[buf.clone(), Value::Int(4), Value::Int(-1234)]).unwrap();
    assert_eq!(
        binary::binary_read_i16_be(&[buf.clone(), Value::Int(4)]).unwrap(),
        Value::Int(-1234)
    );

    // u16 LE & BE
    binary::binary_write_u16_le(&[buf.clone(), Value::Int(6), Value::Int(54321)]).unwrap();
    assert_eq!(
        binary::binary_read_u16_le(&[buf.clone(), Value::Int(6)]).unwrap(),
        Value::Int(54321)
    );
    binary::binary_write_u16_be(&[buf.clone(), Value::Int(8), Value::Int(54321)]).unwrap();
    assert_eq!(
        binary::binary_read_u16_be(&[buf.clone(), Value::Int(8)]).unwrap(),
        Value::Int(54321)
    );

    // i32 LE & BE
    binary::binary_write_i32_le(&[buf.clone(), Value::Int(10), Value::Int(-1234567)]).unwrap();
    assert_eq!(
        binary::binary_read_i32_le(&[buf.clone(), Value::Int(10)]).unwrap(),
        Value::Int(-1234567)
    );
    binary::binary_write_i32_be(&[buf.clone(), Value::Int(14), Value::Int(-1234567)]).unwrap();
    assert_eq!(
        binary::binary_read_i32_be(&[buf.clone(), Value::Int(14)]).unwrap(),
        Value::Int(-1234567)
    );

    // u32 LE & BE
    binary::binary_write_u32_le(&[buf.clone(), Value::Int(18), Value::Int(3000000000)]).unwrap();
    assert_eq!(
        binary::binary_read_u32_le(&[buf.clone(), Value::Int(18)]).unwrap(),
        Value::Int(3000000000)
    );
    binary::binary_write_u32_be(&[buf.clone(), Value::Int(22), Value::Int(3000000000)]).unwrap();
    assert_eq!(
        binary::binary_read_u32_be(&[buf.clone(), Value::Int(22)]).unwrap(),
        Value::Int(3000000000)
    );

    // f64 LE & BE
    let fbuf = Value::Bytes(Rc::new(RefCell::new(vec![0u8; 16])));
    binary::binary_write_f64_le(&[fbuf.clone(), Value::Int(0), Value::Float(123.456)]).unwrap();
    assert_eq!(
        binary::binary_read_f64_le(&[fbuf.clone(), Value::Int(0)]).unwrap(),
        Value::Float(123.456)
    );
    binary::binary_write_f64_be(&[fbuf.clone(), Value::Int(8), Value::Float(789.012)]).unwrap();
    assert_eq!(
        binary::binary_read_f64_be(&[fbuf.clone(), Value::Int(8)]).unwrap(),
        Value::Float(789.012)
    );

    // varint
    let vbuf = Value::Bytes(Rc::new(RefCell::new(vec![0u8; 10])));
    let w =
        binary::binary_write_varint(&[vbuf.clone(), Value::Int(0), Value::Int(624485)]).unwrap();
    assert_eq!(w, Value::Int(3));
    let r = binary::binary_read_varint(&[vbuf.clone(), Value::Int(0)]).unwrap();
    if let Value::List(items) = r {
        assert_eq!(*items.borrow(), vec![Value::Int(624485), Value::Int(3)]);
    } else {
        panic!("expected list");
    }

    // slice
    let sl = binary::binary_slice(&[buf.clone(), Value::Int(2), Value::Int(6)]).unwrap();
    if let Value::Bytes(b) = sl {
        assert_eq!(b.borrow().len(), 4);
    } else {
        panic!("expected bytes");
    }
}

#[test]
fn test_time_pure_types_operations() {
    // Leap year and days in month
    assert!(time::is_leap_year(2000));
    assert!(time::is_leap_year(2024));
    assert!(!time::is_leap_year(1900));
    assert!(!time::is_leap_year(2023));
    assert_eq!(time::days_in_month(2024, 2), 29);
    assert_eq!(time::days_in_month(2023, 2), 28);
    assert_eq!(time::days_in_month(2024, 4), 30);
    assert_eq!(time::days_in_month(2024, 12), 31);

    // Date
    let d = time::time_date(&[Value::Int(2026), Value::Int(9), Value::Int(22)]).unwrap();
    assert!(matches!(d, Value::Struct(_)));
    let d_iso = time::method_date_to_iso(&d, &[]).unwrap();
    assert_eq!(d_iso, Value::String(Rc::new("2026-09-22".to_string())));

    // Out of range date
    let bad_d = time::time_date(&[Value::Int(2026), Value::Int(2), Value::Int(30)]).unwrap();
    assert!(matches!(bad_d, Value::Failure(_)));

    // TimeOfDay
    let t = time::time_time_of_day(&[
        Value::Int(14),
        Value::Int(30),
        Value::Int(45),
        Value::Int(500),
    ])
    .unwrap();
    let t_iso = time::method_time_to_iso(&t, &[]).unwrap();
    assert_eq!(t_iso, Value::String(Rc::new("14:30:45.500".to_string())));

    // DateTime
    let dt = time::time_date_time(&[d.clone(), t.clone(), Value::Int(0)]).unwrap();
    let dt_iso = time::method_datetime_to_iso(&dt, &[]).unwrap();
    assert_eq!(
        dt_iso,
        Value::String(Rc::new("2026-09-22T14:30:45.500Z".to_string()))
    );

    let dt_date = time::method_datetime_date(&dt, &[]).unwrap();
    assert_eq!(time::method_date_to_iso(&dt_date, &[]).unwrap(), d_iso);

    let dt_time = time::method_datetime_time(&dt, &[]).unwrap();
    assert_eq!(time::method_time_to_iso(&dt_time, &[]).unwrap(), t_iso);

    let ep = time::method_datetime_epoch_seconds(&dt, &[]).unwrap();
    assert_eq!(ep, Value::Float(1790087445.5));

    // Parsers
    let p_d = time::time_parse_date(&[Value::String(Rc::new("2026-09-22".to_string()))]).unwrap();
    assert_eq!(time::method_date_to_iso(&p_d, &[]).unwrap(), d_iso);

    let p_t = time::time_parse_time(&[Value::String(Rc::new("14:30:45.500".to_string()))]).unwrap();
    assert_eq!(time::method_time_to_iso(&p_t, &[]).unwrap(), t_iso);

    let p_iso = time::time_parse_iso(&[Value::String(Rc::new(
        "2026-09-22T14:30:45.500Z".to_string(),
    ))])
    .unwrap();
    assert_eq!(time::method_datetime_to_iso(&p_iso, &[]).unwrap(), dt_iso);
}

#[test]
fn test_expect_assertions_operations() {
    // Equal & NotEqual
    assert_eq!(
        testing::expect_equal(&[Value::Int(10), Value::Int(10)]).unwrap(),
        Value::None
    );
    assert!(matches!(
        testing::expect_equal(&[Value::Int(10), Value::Int(20)]).unwrap(),
        Value::Failure(_)
    ));
    assert_eq!(
        testing::expect_not_equal(&[Value::Int(10), Value::Int(20)]).unwrap(),
        Value::None
    );
    assert!(matches!(
        testing::expect_not_equal(&[Value::Int(10), Value::Int(10)]).unwrap(),
        Value::Failure(_)
    ));

    // True & False
    assert_eq!(
        testing::expect_true(&[Value::Bool(true)]).unwrap(),
        Value::None
    );
    assert!(matches!(
        testing::expect_true(&[Value::Bool(false)]).unwrap(),
        Value::Failure(_)
    ));
    assert_eq!(
        testing::expect_false(&[Value::Bool(false)]).unwrap(),
        Value::None
    );
    assert!(matches!(
        testing::expect_false(&[Value::Bool(true)]).unwrap(),
        Value::Failure(_)
    ));

    // None & Some
    assert_eq!(testing::expect_none(&[Value::None]).unwrap(), Value::None);
    assert!(matches!(
        testing::expect_none(&[Value::Int(1)]).unwrap(),
        Value::Failure(_)
    ));
    assert_eq!(
        testing::expect_some(&[Value::Int(42)]).unwrap(),
        Value::None
    );
    assert!(matches!(
        testing::expect_some(&[Value::None]).unwrap(),
        Value::Failure(_)
    ));

    // Failure
    let fail_val = Value::Failure(Rc::new(FailureValue::new("err".to_string())));
    assert_eq!(testing::expect_failure(&[fail_val]).unwrap(), Value::None);
    assert!(matches!(
        testing::expect_failure(&[Value::Int(42)]).unwrap(),
        Value::Failure(_)
    ));

    // Contains
    let list_val = Value::List(Rc::new(RefCell::new(vec![
        Value::Int(1),
        Value::Int(2),
        Value::Int(3),
    ])));
    assert_eq!(
        testing::expect_contains(&[list_val.clone(), Value::Int(2)]).unwrap(),
        Value::None
    );
    assert!(matches!(
        testing::expect_contains(&[list_val, Value::Int(99)]).unwrap(),
        Value::Failure(_)
    ));

    // Approx
    assert_eq!(
        testing::expect_approx(&[
            Value::Float(10.005),
            Value::Float(10.004),
            Value::Float(0.01)
        ])
        .unwrap(),
        Value::None
    );
    assert!(matches!(
        testing::expect_approx(&[
            Value::Float(10.005),
            Value::Float(10.05),
            Value::Float(0.01)
        ])
        .unwrap(),
        Value::Failure(_)
    ));
}

#[test]
fn test_log_module_operations() {
    let captured = Arc::new(Mutex::new(Vec::<u8>::new()));
    let sink = {
        struct TestLogSink(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for TestLogSink {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(buf);
                Ok(buf.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        TestLogSink(Arc::clone(&captured))
    };

    log::set_log_sink(Some(Box::new(sink)));

    log::log_info(&[Value::String(Rc::new("service initialized".to_string()))]).unwrap();
    log::log_error(&[
        Value::String(Rc::new("network error".to_string())),
        Value::Int(500),
    ])
    .unwrap();

    log::set_log_sink(None);

    let output = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
    assert!(output.contains("[INFO] service initialized"));
    assert!(output.contains("[ERROR] network error {500}"));
}
