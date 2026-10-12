//! Integration tests for RegVm register windowing (AIPO-OMNI-005).
//!
//! Verifies that caller registers are strictly preserved across nested calls,
//! recursive invocations, asymmetric window sizes, uninitialized locals isolation,
//! runtime faults, and cooperative task suspensions without blind 4 KiB copies.

#![forbid(unsafe_code)]

use aipo_bytecode::{RegCompiledFunction, RegInstruction, RegOpCode};
use aipo_vm::{RegVm, Value, VmFault};

#[test]
fn test_reg_window_caller_registers_preserved_outside_callee_window() {
    let mut vm = RegVm::new();

    // Callee uses only 2 registers: R[0] = R[0] + R[1]; return R[0]
    let callee = RegCompiledFunction {
        name: "add2".to_string(),
        arity: 2,
        is_async: false,
        instructions: vec![
            RegInstruction::encode_abc(RegOpCode::Add, 0, 0, 1),
            RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0),
        ],
        constants: vec![],
        num_registers: 2,
        contracts: vec![],
    };
    vm.functions = vec![callee];

    // Setup caller with 10 registers active
    for i in 0..10 {
        vm.registers[i] = Value::Int(i as i64 * 10);
    }

    // Call add2(15, 25), storing result in R[0]
    // R[0] = function, R[1] = 15, R[2] = 25
    let code = vec![
        RegInstruction::encode_abx(RegOpCode::MakeFunction, 0, 0),
        RegInstruction::encode_asbx(RegOpCode::LoadInt, 1, 15),
        RegInstruction::encode_asbx(RegOpCode::LoadInt, 2, 25),
        RegInstruction::encode_abc(RegOpCode::Call, 0, 2, 0),
        RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0),
    ];

    let result = vm.run(&code).expect("call should succeed");
    assert_eq!(result, Value::Int(40)); // 15 + 25 = 40

    // Registers 5..10 must retain their exact caller values:
    assert_eq!(vm.registers[5], Value::Int(50));
    assert_eq!(vm.registers[6], Value::Int(60));
    assert_eq!(vm.registers[7], Value::Int(70));
    assert_eq!(vm.registers[8], Value::Int(80));
    assert_eq!(vm.registers[9], Value::Int(90));
}

#[test]
fn test_reg_window_asymmetric_nested_call_cascade() {
    let mut vm = RegVm::new();

    // fn3(x) -> num_registers: 2: R[0] = R[0] * R[0]; return R[0]
    let fn3 = RegCompiledFunction {
        name: "fn3".to_string(),
        arity: 1,
        is_async: false,
        instructions: vec![
            RegInstruction::encode_abc(RegOpCode::Mul, 0, 0, 0),
            RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0),
        ],
        constants: vec![],
        num_registers: 2,
        contracts: vec![],
    };

    // fn2(x) -> num_registers: 6 (larger than fn3 and fn1):
    // R[1] = fn3; R[2] = R[0]; R[1] = call R[1](R[2]); R[4] = 100; R[0] = R[1] + R[4]; return R[0]
    let fn2 = RegCompiledFunction {
        name: "fn2".to_string(),
        arity: 1,
        is_async: false,
        instructions: vec![
            RegInstruction::encode_abx(RegOpCode::MakeFunction, 1, 0), // fn3 is at index 0
            RegInstruction::encode_abc(RegOpCode::Move, 2, 0, 0),
            RegInstruction::encode_abc(RegOpCode::Call, 1, 1, 0), // result lands in R[1]
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 4, 100),
            RegInstruction::encode_abc(RegOpCode::Add, 0, 1, 4),
            RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0),
        ],
        constants: vec![],
        num_registers: 6,
        contracts: vec![],
    };

    // fn1(x) -> num_registers: 3:
    // R[1] = fn2; R[2] = R[0]; R[1] = call fn2(R[2]); return R[1]
    let fn1 = RegCompiledFunction {
        name: "fn1".to_string(),
        arity: 1,
        is_async: false,
        instructions: vec![
            RegInstruction::encode_abx(RegOpCode::MakeFunction, 1, 1), // fn2 is at index 1
            RegInstruction::encode_abc(RegOpCode::Move, 2, 0, 0),
            RegInstruction::encode_abc(RegOpCode::Call, 1, 1, 0),
            RegInstruction::encode_abc(RegOpCode::Return, 1, 0, 0),
        ],
        constants: vec![],
        num_registers: 3,
        contracts: vec![],
    };

    vm.functions = vec![fn3, fn2, fn1];

    // Caller with 8 registers populated
    for i in 0..8 {
        vm.registers[i] = Value::Int(i as i64 * 11);
    }

    // Call fn1(7), store into R[0]
    // Expect: fn3(7) = 7 * 7 = 49; fn2(7) = 49 + 100 = 149; fn1(7) = 149
    let code = vec![
        RegInstruction::encode_abx(RegOpCode::MakeFunction, 0, 2), // fn1 is at index 2
        RegInstruction::encode_asbx(RegOpCode::LoadInt, 1, 7),
        RegInstruction::encode_abc(RegOpCode::Call, 0, 1, 0),
        RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0),
    ];

    let result = vm.run(&code).expect("cascade should succeed");
    assert_eq!(result, Value::Int(149));

    // Registers 7 should remain unchanged
    assert_eq!(vm.registers[7], Value::Int(77));
}

#[test]
fn test_reg_window_recursive_fibonacci() {
    let mut vm = RegVm::new();

    // fib(n):
    // num_registers: 5
    // R[0] = n
    // R[1] = 2
    // R[2] = R[0] < R[1]
    // if R[2] goto base
    // R[1] = fib (MakeFunction)
    // R[3] = 1
    // R[2] = R[0] - R[3]
    // R[1] = call R[1](R[2])  [fib(n-1)]
    // R[3] = R[1]
    // R[1] = fib (MakeFunction)
    // R[4] = 2
    // R[2] = R[0] - R[4]
    // R[1] = call R[1](R[2])  [fib(n-2)]
    // R[0] = R[3] + R[1]
    // return R[0]
    // base:
    // return R[0]
    let fib = RegCompiledFunction {
        name: "fib".to_string(),
        arity: 1,
        is_async: false,
        instructions: vec![
            // 0: R[1] = 2
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 1, 2),
            // 1: R[2] = R[0] < R[1]
            RegInstruction::encode_abc(RegOpCode::Less, 2, 0, 1),
            // 2: if R[2] { jump to base: offset 11 => pc 3+11 = 14 }
            RegInstruction::encode_asbx(RegOpCode::JumpIfTrue, 2, 11),
            // 3: R[1] = MakeFunction 0
            RegInstruction::encode_abx(RegOpCode::MakeFunction, 1, 0),
            // 4: R[3] = 1
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 3, 1),
            // 5: R[2] = R[0] - R[3]
            RegInstruction::encode_abc(RegOpCode::Sub, 2, 0, 3),
            // 6: R[1] = call R[1](R[2])
            RegInstruction::encode_abc(RegOpCode::Call, 1, 1, 0),
            // 7: R[3] = R[1] (save fib(n-1))
            RegInstruction::encode_abc(RegOpCode::Move, 3, 1, 0),
            // 8: R[1] = MakeFunction 0
            RegInstruction::encode_abx(RegOpCode::MakeFunction, 1, 0),
            // 9: R[4] = 2
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 4, 2),
            // 10: R[2] = R[0] - R[4]
            RegInstruction::encode_abc(RegOpCode::Sub, 2, 0, 4),
            // 11: R[1] = call R[1](R[2])
            RegInstruction::encode_abc(RegOpCode::Call, 1, 1, 0),
            // 12: R[0] = R[3] + R[1]
            RegInstruction::encode_abc(RegOpCode::Add, 0, 3, 1),
            // 13: return R[0]
            RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0),
            // 14 (base): return R[0]
            RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0),
        ],
        constants: vec![],
        num_registers: 5,
        contracts: vec![],
    };

    vm.functions = vec![fib];

    // Compute fib(10) = 55
    let code = vec![
        RegInstruction::encode_abx(RegOpCode::MakeFunction, 0, 0),
        RegInstruction::encode_asbx(RegOpCode::LoadInt, 1, 10),
        RegInstruction::encode_abc(RegOpCode::Call, 0, 1, 0),
        RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0),
    ];

    let result = vm.run(&code).expect("fib(10) should execute successfully");
    assert_eq!(result, Value::Int(55));
    assert!(
        vm.register_stack.is_empty(),
        "register_stack should be fully drained"
    );
}

#[test]
fn test_reg_window_uninitialized_locals_isolation() {
    let mut vm = RegVm::new();

    // Function with arity 1 and 4 registers.
    // Locals R[1], R[2], R[3] must be initialized to None.
    // Return R[2] (which should be None, not caller residue!)
    let checker = RegCompiledFunction {
        name: "checker".to_string(),
        arity: 1,
        is_async: false,
        instructions: vec![
            // Return R[2]
            RegInstruction::encode_abc(RegOpCode::Return, 2, 0, 0),
        ],
        constants: vec![],
        num_registers: 4,
        contracts: vec![],
    };
    vm.functions = vec![checker];

    // Poison caller's registers 0..4 with garbage
    vm.registers[0] = Value::Int(999);
    vm.registers[1] = Value::Int(888);
    vm.registers[2] = Value::Int(777); // R[2] has 777 in caller!
    vm.registers[3] = Value::Int(666);

    // Call checker with 1 argument in R[1] = 42
    let code = vec![
        RegInstruction::encode_abx(RegOpCode::MakeFunction, 0, 0),
        RegInstruction::encode_asbx(RegOpCode::LoadInt, 1, 42),
        // invoke with 1 argument:
        RegInstruction::encode_abc(RegOpCode::Call, 0, 1, 0),
        RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0),
    ];

    let result = vm.run(&code).expect("call should succeed");
    // Callee returned R[2], which must have been zeroed to None, NOT caller's 777!
    assert_eq!(result, Value::None);
    // And caller's R[2] must be restored to 777!
    assert_eq!(vm.registers[2], Value::Int(777));
    assert_eq!(vm.registers[3], Value::Int(666));
}

#[test]
fn test_reg_window_restores_cleanly_on_callee_runtime_fault() {
    let mut vm = RegVm::new();

    // Callee divides by zero: R[0] / R[1] where R[1] = 0
    let faulty = RegCompiledFunction {
        name: "faulty".to_string(),
        arity: 2,
        is_async: false,
        instructions: vec![
            RegInstruction::encode_abc(RegOpCode::Div, 0, 0, 1),
            RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0),
        ],
        constants: vec![],
        num_registers: 3,
        contracts: vec![],
    };
    vm.functions = vec![faulty];

    // Seed caller registers
    vm.registers[0] = Value::Int(111);
    vm.registers[1] = Value::Int(222);
    vm.registers[2] = Value::Int(333);

    // Caller places function and arguments in R[3], R[4], R[5], leaving R[0]..R[2] intact
    let code = vec![
        RegInstruction::encode_abx(RegOpCode::MakeFunction, 3, 0),
        RegInstruction::encode_asbx(RegOpCode::LoadInt, 4, 10),
        RegInstruction::encode_asbx(RegOpCode::LoadInt, 5, 0),
        RegInstruction::encode_abc(RegOpCode::Call, 3, 2, 0),
        RegInstruction::encode_abc(RegOpCode::Return, 3, 0, 0),
    ];

    let err = vm.run(&code).unwrap_err();
    assert!(matches!(err, VmFault::DivisionByZero));

    // Register stack must be clean and caller registers preserved
    assert!(vm.register_stack.is_empty());
    assert_eq!(vm.registers[0], Value::Int(111));
    assert_eq!(vm.registers[1], Value::Int(222));
    assert_eq!(vm.registers[2], Value::Int(333));
}

#[test]
fn test_reg_window_cooperative_task_suspend_and_resume() {
    let mut vm = RegVm::new();

    // async worker(x): return x + 10
    let worker = RegCompiledFunction {
        name: "worker".to_string(),
        arity: 1,
        is_async: true,
        instructions: vec![
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 1, 10),
            RegInstruction::encode_abc(RegOpCode::Add, 0, 0, 1),
            RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0),
        ],
        constants: vec![],
        num_registers: 2,
        contracts: vec![],
    };
    vm.functions = vec![worker];

    // Seed caller registers
    vm.registers[0] = Value::Int(777);
    vm.registers[1] = Value::Int(888);

    // Caller:
    // R[2] = MakeFunction 0 (async fn)
    // R[3] = 32 (arg)
    // R[2] = Call R[2](R[3]) -> returns Value::Task(id)
    // R[2] = Await R[2] -> parks until worker finishes -> returns 42
    // Return R[2]
    let code = vec![
        RegInstruction::encode_abx(RegOpCode::MakeFunction, 2, 0),
        RegInstruction::encode_asbx(RegOpCode::LoadInt, 3, 32),
        RegInstruction::encode_abc(RegOpCode::Call, 2, 1, 0),
        RegInstruction::encode_abc(RegOpCode::Await, 2, 0, 0),
        RegInstruction::encode_abc(RegOpCode::Return, 2, 0, 0),
    ];

    let result = vm.run(&code).expect("async execution should succeed");
    assert_eq!(result, Value::Int(42));

    // Caller registers R[0] and R[1] must remain completely intact:
    assert_eq!(vm.registers[0], Value::Int(777));
    assert_eq!(vm.registers[1], Value::Int(888));
    assert!(vm.register_stack.is_empty());
}
