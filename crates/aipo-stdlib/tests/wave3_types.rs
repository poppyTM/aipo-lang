//! Comprehensive tests for Wave 3 types and operations (P02-G01):
//! Set, Sequence, Bytes packing, Task, Duration.

use aipo_runtime::NativeRegistry;
use aipo_stdlib::{bytes, collections, convert, duration, prelude, register_stdlib};
use aipo_vm::{SequenceSource, Value, Vm, VmFault};
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn test_set_creation_and_deduplication() {
    let list = Value::List(Rc::new(RefCell::new(vec![
        Value::Int(1),
        Value::Int(2),
        Value::Int(1),
        Value::Int(3),
        Value::Int(2),
    ])));

    let set_val = convert::convert_set(&[list]).expect("Set conversion should succeed");
    if let Value::Set(items) = &set_val {
        let borrowed = items.borrow();
        // First-occurrence order preserved: 1, 2, 3
        assert_eq!(borrowed.len(), 3);
        assert_eq!(borrowed[0], Value::Int(1));
        assert_eq!(borrowed[1], Value::Int(2));
        assert_eq!(borrowed[2], Value::Int(3));
    } else {
        panic!("expected Value::Set, got {set_val:?}");
    }
}

#[test]
fn test_empty_set_creation() {
    let set_val = convert::convert_set(&[]).expect("Empty Set() should succeed");
    if let Value::Set(items) = &set_val {
        assert_eq!(items.borrow().len(), 0);
    } else {
        panic!("expected Value::Set, got {set_val:?}");
    }
}

#[test]
fn test_set_methods() {
    let set_val = Value::Set(Rc::new(RefCell::new(vec![Value::Int(10), Value::Int(20)])));

    // has
    assert_eq!(
        collections::set_has(&set_val, &[Value::Int(10)]).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        collections::set_has(&set_val, &[Value::Int(30)]).unwrap(),
        Value::Bool(false)
    );

    // add new element
    assert_eq!(
        collections::set_add(&set_val, &[Value::Int(30)]).unwrap(),
        Value::None
    );
    assert_eq!(collections::set_len(&set_val, &[]).unwrap(), Value::Int(3));

    // add duplicate (no-op)
    assert_eq!(
        collections::set_add(&set_val, &[Value::Int(10)]).unwrap(),
        Value::None
    );
    assert_eq!(collections::set_len(&set_val, &[]).unwrap(), Value::Int(3));

    // remove existing
    assert_eq!(
        collections::set_remove(&set_val, &[Value::Int(20)]).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        collections::set_has(&set_val, &[Value::Int(20)]).unwrap(),
        Value::Bool(false)
    );
    assert_eq!(collections::set_len(&set_val, &[]).unwrap(), Value::Int(2));

    // remove non-existing
    assert_eq!(
        collections::set_remove(&set_val, &[Value::Int(99)]).unwrap(),
        Value::Bool(false)
    );

    // to_list
    let list_val = collections::set_to_list(&set_val, &[]).unwrap();
    if let Value::List(l) = list_val {
        assert_eq!(*l.borrow(), vec![Value::Int(10), Value::Int(30)]);
    } else {
        panic!("expected List");
    }

    // is_empty
    assert_eq!(
        collections::set_is_empty(&set_val, &[]).unwrap(),
        Value::Bool(false)
    );

    // clear
    assert_eq!(collections::set_clear(&set_val, &[]).unwrap(), Value::None);
    assert_eq!(
        collections::set_is_empty(&set_val, &[]).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(collections::set_len(&set_val, &[]).unwrap(), Value::Int(0));
}

#[test]
fn test_set_prelude_operations() {
    let set_a = Value::Set(Rc::new(RefCell::new(vec![Value::Int(1), Value::Int(2)])));
    let set_b = Value::Set(Rc::new(RefCell::new(vec![Value::Int(1), Value::Int(2)])));
    let set_ref = set_a.clone();

    // Structural equality
    assert_eq!(set_a, set_b);

    // Reference identity (same)
    assert_eq!(
        prelude::native_same(&[set_a.clone(), set_b]).unwrap(),
        Value::Bool(false)
    );
    assert_eq!(
        prelude::native_same(&[set_a.clone(), set_ref]).unwrap(),
        Value::Bool(true)
    );

    // native_len
    assert_eq!(
        prelude::native_len(std::slice::from_ref(&set_a)).unwrap(),
        Value::Int(2)
    );

    // native_copy
    let copy_set = prelude::native_copy(std::slice::from_ref(&set_a)).unwrap();
    assert_eq!(copy_set, set_a);
    if let Value::Set(c) = &copy_set {
        c.borrow_mut().push(Value::Int(3));
    }
    // original unaffected
    if let Value::Set(orig) = &set_a {
        assert_eq!(orig.borrow().len(), 2);
    }
}

#[test]
fn test_bytes_packing_and_unpacking_little_endian() {
    let buffer = Value::Bytes(Rc::new(RefCell::new(vec![0u8; 16])));

    // write_i8 / read_i8
    bytes::bytes_write_i8(&buffer, &[Value::Int(0), Value::Int(-42)]).unwrap();
    assert_eq!(
        bytes::bytes_read_i8(&buffer, &[Value::Int(0)]).unwrap(),
        Value::Int(-42)
    );

    // write_u8 / read_u8
    bytes::bytes_write_u8(&buffer, &[Value::Int(1), Value::Int(200)]).unwrap();
    assert_eq!(
        bytes::bytes_read_u8(&buffer, &[Value::Int(1)]).unwrap(),
        Value::Int(200)
    );

    // write_i16 / read_i16
    bytes::bytes_write_i16(&buffer, &[Value::Int(2), Value::Int(-1234)]).unwrap();
    assert_eq!(
        bytes::bytes_read_i16(&buffer, &[Value::Int(2)]).unwrap(),
        Value::Int(-1234)
    );

    // write_u16 / read_u16
    bytes::bytes_write_u16(&buffer, &[Value::Int(4), Value::Int(54321)]).unwrap();
    assert_eq!(
        bytes::bytes_read_u16(&buffer, &[Value::Int(4)]).unwrap(),
        Value::Int(54321)
    );

    // write_i32 / read_i32
    bytes::bytes_write_i32(&buffer, &[Value::Int(6), Value::Int(-1000000)]).unwrap();
    assert_eq!(
        bytes::bytes_read_i32(&buffer, &[Value::Int(6)]).unwrap(),
        Value::Int(-1000000)
    );

    // write_f64 / read_f64
    let f_val = std::f64::consts::PI;
    let buf8 = Value::Bytes(Rc::new(RefCell::new(vec![0u8; 8])));
    bytes::bytes_write_f64(&buf8, &[Value::Int(0), Value::Float(f_val)]).unwrap();
    assert_eq!(
        bytes::bytes_read_f64(&buf8, &[Value::Int(0)]).unwrap(),
        Value::Float(f_val)
    );

    // Little-endian check: 0x0102 in u16 writes [0x02, 0x01]
    let buf2 = Value::Bytes(Rc::new(RefCell::new(vec![0u8; 2])));
    bytes::bytes_write_u16(&buf2, &[Value::Int(0), Value::Int(0x0102)]).unwrap();
    if let Value::Bytes(b) = &buf2 {
        assert_eq!(&*b.borrow(), &[0x02, 0x01]);
    }

    // Out of bounds faults
    let err = bytes::bytes_read_i32(&buf2, &[Value::Int(0)]).unwrap_err();
    assert!(matches!(err, VmFault::IndexOutOfRange { .. }));

    // Value out of range -> recoverable failure
    let fail_res = bytes::bytes_write_i8(&buf2, &[Value::Int(0), Value::Int(300)]).unwrap();
    assert!(matches!(fail_res, Value::Failure(_)));
}

#[test]
fn test_bytes_decode_and_string_encode() {
    let text = Value::String(Rc::new("Aipo 🚀 UTF-8".to_string()));
    let encoded = bytes::string_encode(&text, &[]).unwrap();

    if let Value::Bytes(b) = &encoded {
        assert_eq!(&*b.borrow(), "Aipo 🚀 UTF-8".as_bytes());
    } else {
        panic!("expected Value::Bytes");
    }

    // Round-trip decode
    let decoded = bytes::bytes_decode(&encoded, &[]).unwrap();
    assert_eq!(decoded, text);

    // Invalid UTF-8 produces Failure
    let invalid_bytes = Value::Bytes(Rc::new(RefCell::new(vec![0xFF, 0xFE])));
    let fail = bytes::bytes_decode(&invalid_bytes, &[]).unwrap();
    assert!(matches!(fail, Value::Failure(_)));
}

#[test]
fn test_duration_operations() {
    let d1 = convert::convert_duration(&[Value::Float(1.5)]).unwrap();
    let d2 = convert::convert_duration(&[Value::Int(2)]).unwrap();

    assert_eq!(d1, Value::Duration(1.5));
    assert_eq!(d2, Value::Duration(2.0));

    // total_seconds
    assert_eq!(
        duration::duration_total_seconds(&d1, &[]).unwrap(),
        Value::Float(1.5)
    );

    // Arithmetic
    let sum = d1.add(&d2).unwrap();
    assert_eq!(sum, Value::Duration(3.5));

    let diff = d2.sub(&d1).unwrap();
    assert_eq!(diff, Value::Duration(0.5));

    let neg = d1.negate().unwrap();
    assert_eq!(neg, Value::Duration(-1.5));

    // Comparisons
    assert_eq!(d1.less(&d2).unwrap(), Value::Bool(true));
    assert_eq!(d2.less(&d1).unwrap(), Value::Bool(false));
    assert_eq!(d1.less_equal(&d1).unwrap(), Value::Bool(true));
}

#[test]
fn test_sequence_lazy_creation() {
    let list = Value::List(Rc::new(RefCell::new(vec![Value::Int(1), Value::Int(2)])));
    let seq = collections::list_lazy(&list, &[]).unwrap();

    if let Value::Sequence(pipeline) = &seq {
        assert!(matches!(pipeline.source, SequenceSource::List(_)));
        assert_eq!(pipeline.ops.len(), 0);
    } else {
        panic!("expected Value::Sequence");
    }

    let set = Value::Set(Rc::new(RefCell::new(vec![Value::Int(10)])));
    let set_seq = collections::set_lazy(&set, &[]).unwrap();
    if let Value::Sequence(pipeline) = &set_seq {
        assert!(matches!(pipeline.source, SequenceSource::Set(_)));
    } else {
        panic!("expected Value::Sequence");
    }
}

#[test]
fn test_task_module_registration() {
    let mut vm = Vm::new();
    let mut registry = NativeRegistry::new();
    register_stdlib(&mut vm, &mut registry);

    // Global task module exists
    let task_mod = vm
        .globals
        .get("task")
        .expect("task module should be defined");
    if let Value::Dict(d) = task_mod {
        let d = d.borrow();
        assert!(
            d.get(&Value::String(Rc::new("spawn".to_string())))
                .is_some()
        );
        assert!(
            d.get(&Value::String(Rc::new("sleep".to_string())))
                .is_some()
        );
        assert!(d.get(&Value::String(Rc::new("all".to_string()))).is_some());
        assert!(d.get(&Value::String(Rc::new("race".to_string()))).is_some());
        assert!(
            d.get(&Value::String(Rc::new("timeout".to_string())))
                .is_some()
        );
        assert!(
            d.get(&Value::String(Rc::new("cancel".to_string())))
                .is_some()
        );
        assert!(
            d.get(&Value::String(Rc::new("group".to_string())))
                .is_some()
        );
    } else {
        panic!("expected task to be a Dict");
    }
}
