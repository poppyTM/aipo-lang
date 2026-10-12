//! Register runtime regression cases; no execution was performed in the reconstruction delivery.
use aipo_bytecode::{RegInstruction as I, RegOpCode as O};
use aipo_vm::{DictMap, RegVm, Value, VmFault};
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn mixed_numeric_arithmetic_uses_shared_value_semantics() {
    let mut vm = RegVm::new();
    vm.registers[1] = Value::Byte(2);
    vm.registers[2] = Value::Float(0.5);
    assert_eq!(
        vm.run(&[
            I::encode_abc(O::Add, 0, 1, 2),
            I::encode_abc(O::Return, 0, 0, 0)
        ])
        .unwrap(),
        Value::Float(2.5)
    );
}

#[test]
fn invalid_register_constant_and_call_operands_return_faults() {
    for instruction in [
        I::encode_abc(O::Move, 0, 511, 0),
        I::encode_abx(O::LoadConst, 0, 999),
        I::encode_abc(O::Call, 255, 1, 0),
        I::encode_asbx(O::Jump, 0, 10),
    ] {
        assert!(matches!(
            RegVm::new().run(&[instruction]),
            Err(VmFault::CorruptedBytecode { .. })
        ));
    }
}

#[test]
fn budget_is_cumulative_and_reset_is_explicit() {
    let mut vm = RegVm::new();
    vm.set_instruction_budget(Some(2));
    let code = [
        I::encode_abc(O::LoadNil, 0, 0, 0),
        I::encode_abc(O::Return, 0, 0, 0),
    ];
    vm.run(&code).unwrap();
    assert_eq!(vm.instruction_count(), 2);
    assert!(matches!(vm.run(&code), Err(VmFault::Overflow { .. })));
    vm.reset_instruction_count();
    vm.run(&code).unwrap();
}

#[test]
fn unicode_negative_indices_and_extremes_do_not_overflow() {
    let mut vm = RegVm::new();
    vm.registers[1] = Value::String(Rc::new("é🦀".to_string()));
    let code = [
        I::encode_abc(O::GetIndex, 0, 1, 2),
        I::encode_abc(O::Return, 0, 0, 0),
    ];
    vm.registers[2] = Value::Int(-1);
    assert_eq!(
        vm.run(&code).unwrap(),
        Value::String(Rc::new("🦀".to_string()))
    );
    vm.registers[2] = Value::Int(i64::MIN);
    assert!(matches!(
        vm.run(&code),
        Err(VmFault::IndexOutOfRange { .. })
    ));
}

#[test]
fn iteration_uses_full_index_register_and_primary_dict_keys() {
    let mut vm = RegVm::new();
    vm.registers[1] = Value::Dict(Rc::new(RefCell::new(DictMap::from_entries(vec![(
        Value::String(Rc::new("key".to_string())),
        Value::Int(42),
    )]))));
    vm.registers[200] = Value::Int(0);
    let code = [
        I::encode_abc(O::IterPrimary, 0, 1, 200),
        I::encode_abc(O::Return, 0, 0, 0),
    ];
    assert_eq!(
        vm.run(&code).unwrap(),
        Value::String(Rc::new("key".to_string()))
    );
}
