//! The register engine accepts the same byte-slot assignment the stack VM does.

use aipo_bytecode::{RegInstruction as I, RegOpCode as O};
use aipo_vm::{Collection, RegVm, Value};
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn register_engine_stores_an_int_in_a_byte_slot() {
    // `b[0] = 65` produces an `Int`, and the stack VM accepts one in `0..=255`. The
    // register engine required a `Byte` value and silently skipped the write, so the
    // same program read back 0.
    let buffer = Rc::new(RefCell::new(Collection::from(vec![0u8, 0u8])));
    let mut vm = RegVm::new();
    vm.registers[0] = Value::Bytes(buffer.clone());
    vm.registers[1] = Value::Int(0);
    vm.registers[2] = Value::Int(65);
    vm.run(&[
        I::encode_abc(O::SetIndex, 0, 1, 2),
        I::encode_abc(O::Return, 0, 0, 0),
    ])
    .expect("the write is accepted");
    assert_eq!(buffer.borrow()[0], 65, "the byte write must land");
}

#[test]
fn register_engine_rejects_a_byte_outside_the_range() {
    // 300 does not fit a byte, and the stack VM raises a type mismatch for it.
    let buffer = Rc::new(RefCell::new(Collection::from(vec![0u8])));
    let mut vm = RegVm::new();
    vm.registers[0] = Value::Bytes(buffer.clone());
    vm.registers[1] = Value::Int(0);
    vm.registers[2] = Value::Int(300);
    assert!(
        vm.run(&[
            I::encode_abc(O::SetIndex, 0, 1, 2),
            I::encode_abc(O::Return, 0, 0, 0),
        ])
        .is_err(),
        "a value outside 0..=255 must not be stored"
    );
    assert_eq!(buffer.borrow()[0], 0);
}
