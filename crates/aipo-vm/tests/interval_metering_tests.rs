//! Integration tests for periodic fuel metering / interval metering (AIPO-OMNI-004).
//!
//! Verifies that execution budgets are checked periodically at configured intervals
//! amortizing hot-path branch costs while guaranteeing strict termination on infinite loops
//! and reconciling cleanly on return.

#![forbid(unsafe_code)]

use aipo_bytecode::{
    BytecodeModule, Constant, OpCode, RegCompiledFunction, RegInstruction, RegOpCode,
};
use aipo_vm::{AipoEngineBuilder, RegVm, Vm, VmError, VmFault};

fn make_test_module(code: Vec<u8>, constants: Vec<Constant>) -> BytecodeModule {
    let mut module = BytecodeModule::new();
    module.code = code;
    module.constants = constants;
    module
}

#[test]
fn test_interval_metering_amortized_budget_trip_on_stack_vm() {
    let mut vm = Vm::new();
    // Infinite loop: Jump backwards by -3 bytes
    let mut code = vec![OpCode::Jump as u8];
    code.extend_from_slice(&(-3i16).to_be_bytes());
    let module = make_test_module(code, vec![]);

    // Configure budget of 100 and interval of 16
    vm.set_max_instructions(Some(100));
    vm.set_meter_interval(16);
    assert_eq!(vm.meter_interval(), 16);

    let err = vm
        .run(&module)
        .expect_err("infinite loop must terminate via instruction budget");

    match err {
        VmError::Fault(VmFault::Overflow { details }) => {
            assert!(
                details.contains("instruction limit of 100 reached"),
                "unexpected details: {details}"
            );
        }
        other => panic!("expected VmFault::Overflow, got {other:?}"),
    }

    // Interval metering checks every 16 instructions, so the trip occurs at the first
    // check where executed > 100, which is exactly 112 (16 * 7):
    let count = vm.instruction_count();
    assert!(count > 100, "count ({count}) must exceed limit of 100");
    assert_eq!(
        count % 16,
        0,
        "count ({count}) must align with meter_interval (16)"
    );
}

#[test]
fn test_interval_metering_amortized_budget_trip_on_reg_vm() {
    let mut vm = RegVm::new();
    // Infinite loop on RegVm: Jump to self (pc = 1 - 1 = 0)
    let code = vec![RegInstruction::encode_asbx(RegOpCode::Jump, 0, -1)];

    // Configure budget of 100 and interval of 32
    vm.set_max_instructions(Some(100));
    vm.set_meter_interval(32);
    assert_eq!(vm.meter_interval(), 32);

    let err = vm
        .run(&code)
        .expect_err("infinite loop must terminate via instruction budget");

    match err {
        VmFault::Overflow { details } => {
            assert!(
                details.contains("instruction limit of 100 reached"),
                "unexpected details: {details}"
            );
        }
        other => panic!("expected VmFault::Overflow, got {other:?}"),
    }

    let count = vm.instruction_count();
    assert!(count > 100, "count ({count}) must exceed limit of 100");
    assert_eq!(
        count % 32,
        0,
        "count ({count}) must align with meter_interval (32)"
    );
}

#[test]
fn test_interval_metering_fine_grained_single_step() {
    let mut vm = Vm::new();
    let mut code = vec![OpCode::Jump as u8];
    code.extend_from_slice(&(-3i16).to_be_bytes());
    let module = make_test_module(code, vec![]);

    // With meter_interval(1), budget checks run on every single instruction
    vm.set_max_instructions(Some(50));
    vm.set_meter_interval(1);
    assert_eq!(vm.meter_interval(), 1);

    let err = vm.run(&module).unwrap_err();
    assert!(matches!(err, VmError::Fault(VmFault::Overflow { .. })));
    assert_eq!(vm.instruction_count(), 51);
}

#[test]
fn test_interval_metering_reconciliation_on_clean_return() {
    let mut vm = RegVm::new();

    // Loop 15 times doing simple arithmetic, then Return:
    // Total executed instructions will be around 45 instructions.
    // If budget is set to 30 with interval 64:
    // The interval check (64) will NOT trigger during the loop,
    // but the final Return reconciliation MUST detect that 45 > 30 and trip!
    let looper = RegCompiledFunction {
        name: "looper".to_string(),
        arity: 0,
        is_async: false,
        instructions: vec![
            // 0: R[0] = 0
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 0, 0),
            // 1: R[1] = 15
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 1, 15),
            // 2: R[2] = 1
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 2, 1),
            // loop top (3): R[0] = R[0] + R[2]
            RegInstruction::encode_abc(RegOpCode::Add, 0, 0, 2),
            // 4: R[3] = R[0] < R[1]
            RegInstruction::encode_abc(RegOpCode::Less, 3, 0, 1),
            // 5: if R[3] goto loop top (3): offset -3 (pc is 6, 6 + (-3) = 3)
            RegInstruction::encode_asbx(RegOpCode::JumpIfTrue, 3, -3),
            // 6: return R[0]
            RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0),
        ],
        constants: vec![],
        num_registers: 4,
        contracts: vec![],
    };
    vm.functions = vec![looper];

    vm.set_max_instructions(Some(30));
    vm.set_meter_interval(64); // larger than total instruction count

    let code = vec![
        RegInstruction::encode_abx(RegOpCode::MakeFunction, 0, 0),
        RegInstruction::encode_abc(RegOpCode::Call, 0, 0, 0),
        RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0),
    ];

    let err = vm.run(&code).unwrap_err();
    assert!(
        matches!(err, VmFault::Overflow { .. }),
        "final Return reconciliation must trip budget overflow"
    );
    assert!(vm.instruction_count() > 30);
}

#[test]
fn test_interval_metering_with_aipo_engine_builder() {
    let mut code = vec![OpCode::Jump as u8];
    code.extend_from_slice(&(-3i16).to_be_bytes());
    let module = make_test_module(code, vec![]);

    let mut engine = AipoEngineBuilder::new()
        .max_instructions(200)
        .meter_interval(64)
        .build()
        .expect("engine builds cleanly");

    assert_eq!(engine.meter_interval(), 64);

    let err = engine.run(&module).unwrap_err();
    assert!(matches!(err, VmError::Fault(VmFault::Overflow { .. })));

    let count = engine.instruction_count();
    assert!(count > 200);
    assert_eq!(count % 64, 0);
}
