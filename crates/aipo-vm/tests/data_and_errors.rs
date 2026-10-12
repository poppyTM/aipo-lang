//! Comprehensive unit tests for Data & Errors (Slice S8).

use aipo_bytecode::opcode::{Constant, OpCode};
use aipo_bytecode::{BytecodeModule, FunctionInfo};
use aipo_diagnostics::DiagnosticCode;
use aipo_vm::{Value, Vm, VmError, VmFault, execute};
use byteorder::{BigEndian, ByteOrder};

fn make_test_module(code: Vec<u8>, constants: Vec<Constant>, names: Vec<String>) -> BytecodeModule {
    let mut module = BytecodeModule::new();
    module.code = code;
    module.constants = constants;
    module.names = names;
    module
}

#[test]
fn test_list_creation_and_negative_indexing() {
    let mut code = Vec::new();
    // Push 10, 20, 30
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&0u16.to_be_bytes());
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&1u16.to_be_bytes());
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&2u16.to_be_bytes());
    // BuildList 3 -> [10, 20, 30]
    code.push(OpCode::BuildList as u8);
    code.extend_from_slice(&3u16.to_be_bytes());

    // Duplicate list
    code.push(OpCode::Dup as u8);
    // Index with -1 (idx 3) -> should retrieve 30
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&3u16.to_be_bytes());
    code.push(OpCode::GetIndex as u8);

    // Stack is now: [list, 30]
    // Verify 30
    let module = make_test_module(
        code,
        vec![
            Constant::Int(10),
            Constant::Int(20),
            Constant::Int(30),
            Constant::Int(-1),
        ],
        vec![],
    );

    let mut vm = Vm::new();
    let result = vm.run(&module).expect("execution should succeed");
    assert_eq!(result, Value::Int(30));
}

#[test]
fn test_list_index_out_of_bounds_fault() {
    let mut code = Vec::new();
    // BuildList [10]
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&0u16.to_be_bytes());
    code.push(OpCode::BuildList as u8);
    code.extend_from_slice(&1u16.to_be_bytes());

    // Index 5 (out of bounds)
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&1u16.to_be_bytes());
    code.push(OpCode::GetIndex as u8);

    let module = make_test_module(code, vec![Constant::Int(10), Constant::Int(5)], vec![]);

    let err = execute(&module).expect_err("out of bounds index must fault");
    assert_eq!(
        err.diagnostic_code(),
        DiagnosticCode::AIPO_RT_INDEX_OUT_OF_RANGE
    );
}

#[test]
fn test_dict_operations_and_key_lookup() {
    let mut code = Vec::new();
    // Key "a" (const 0), Val 100 (const 1)
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&0u16.to_be_bytes());
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&1u16.to_be_bytes());

    // Key "b" (const 2), Val 200 (const 3)
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&2u16.to_be_bytes());
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&3u16.to_be_bytes());

    // BuildDict 2 -> #{"a": 100, "b": 200}
    code.push(OpCode::BuildDict as u8);
    code.extend_from_slice(&2u16.to_be_bytes());

    // Lookup "b"
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&2u16.to_be_bytes());
    code.push(OpCode::GetIndex as u8);

    let module = make_test_module(
        code,
        vec![
            Constant::String("a".to_string()),
            Constant::Int(100),
            Constant::String("b".to_string()),
            Constant::Int(200),
        ],
        vec![],
    );

    let result = execute(&module).expect("dict lookup should succeed");
    assert_eq!(result, Value::Int(200));
}

#[test]
fn test_struct_construction_and_field_access() {
    let mut code = Vec::new();
    // Push field values: id = 1, name = "Aipo"
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&0u16.to_be_bytes());
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&1u16.to_be_bytes());

    // BuildStruct Player (name 0), 2 fields, published immediately
    code.push(OpCode::BuildStruct as u8);
    code.extend_from_slice(&0u16.to_be_bytes());
    code.extend_from_slice(&2u16.to_be_bytes());
    code.push(0);

    // GetField "name" (name 2)
    code.push(OpCode::GetField as u8);
    code.extend_from_slice(&2u16.to_be_bytes());

    let module = make_test_module(
        code,
        vec![Constant::Int(1), Constant::String("Aipo".to_string())],
        vec!["Player".to_string(), "id".to_string(), "name".to_string()],
    );

    let mut vm = Vm::new();
    vm.register_struct("Player", vec![("id", true), ("name", false)]);

    let result = vm.run(&module).expect("struct field get should succeed");
    assert_eq!(result, Value::String(std::rc::Rc::new("Aipo".to_string())));
}

#[test]
fn test_struct_fixed_field_mutation_fault() {
    let mut code = Vec::new();
    // id = 1, name = "Hero"
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&0u16.to_be_bytes());
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&1u16.to_be_bytes());

    // BuildStruct Player, published immediately
    code.push(OpCode::BuildStruct as u8);
    code.extend_from_slice(&0u16.to_be_bytes());
    code.extend_from_slice(&2u16.to_be_bytes());
    code.push(0);

    // Attempt to set fixed field "id" (name 1) to 99 (const 2)
    // Stack for SetField: target, new_val
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&2u16.to_be_bytes());
    code.push(OpCode::SetField as u8);
    code.extend_from_slice(&1u16.to_be_bytes());

    let module = make_test_module(
        code,
        vec![
            Constant::Int(1),
            Constant::String("Hero".to_string()),
            Constant::Int(99),
        ],
        vec!["Player".to_string(), "id".to_string(), "name".to_string()],
    );

    let mut vm = Vm::new();
    vm.register_struct("Player", vec![("id", true), ("name", false)]);

    let err = vm
        .run(&module)
        .expect_err("mutating fixed field must fault");
    assert_eq!(err.diagnostic_code(), DiagnosticCode::AIPO_RT_TYPE_MISMATCH);
    assert!(matches!(
        err,
        VmError::Fault(VmFault::FixedFieldMutation { .. })
    ));
}

#[test]
fn test_struct_invariant_validation_fault() {
    let mut code = Vec::new();
    // count = -5
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&0u16.to_be_bytes());

    // BuildStruct Inventory (name 0), 1 field, published immediately
    code.push(OpCode::BuildStruct as u8);
    code.extend_from_slice(&0u16.to_be_bytes());
    code.extend_from_slice(&1u16.to_be_bytes());
    code.push(0);

    let module = make_test_module(
        code,
        vec![Constant::Int(-5)],
        vec!["Inventory".to_string(), "count".to_string()],
    );

    let mut vm = Vm::new();
    vm.register_struct("Inventory", vec![("count", false)]);
    vm.register_struct_invariant("Inventory", |inst| {
        if let Some(Value::Int(c)) = inst.get_field("count")
            && *c < 0
        {
            return Err("inventory count must be non-negative".to_string());
        }
        Ok(())
    });

    let err = vm.run(&module).expect_err("invariant violation must fault");
    assert!(matches!(
        err,
        VmError::Fault(VmFault::InvariantViolation { .. })
    ));
}

#[test]
fn test_failure_model_b_propagation() {
    let mut code = Vec::new();
    // Fail with "network timeout"
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&0u16.to_be_bytes());
    code.push(OpCode::Fail as u8);

    // Attempt addition with 10: Failure + 10 -> should propagate Failure
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&1u16.to_be_bytes());
    code.push(OpCode::Add as u8);

    let module = make_test_module(
        code,
        vec![
            Constant::String("network timeout".to_string()),
            Constant::Int(10),
        ],
        vec![],
    );

    let err =
        execute(&module).expect_err("unhandled failure at top level must return UncaughtFailure");
    assert_eq!(
        err.diagnostic_code(),
        DiagnosticCode::AIPO_RT_FAILURE_UNCAUGHT
    );
    assert_eq!(err, VmError::UncaughtFailure("network timeout".to_string()));
}

/// Builds a `Failure` value the way the language does: by calling `fail("message")`.
///
/// `OpCode::Fail` is the *statement* form and terminates the path immediately, so the
/// expression form used with `or_else` goes through the `fail` native, which yields the
/// `Failure` as an ordinary value.
fn fail_native(args: &[Value]) -> Result<Value, VmFault> {
    let message = match args.first() {
        Some(Value::String(text)) => text.as_str().to_string(),
        other => other.map_or_else(|| "failure".to_string(), Value::to_string),
    };
    Ok(Value::Failure(std::rc::Rc::new(
        aipo_vm::FailureValue::new(message),
    )))
}

#[test]
fn test_or_else_fallback_recovery() {
    let mut code = Vec::new();
    // Left: fail("not found")
    code.push(OpCode::GetGlobal as u8);
    code.extend_from_slice(&0u16.to_be_bytes());
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&1u16.to_be_bytes());
    code.push(OpCode::Call as u8);
    code.push(1);

    // Right: Int(404)
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&2u16.to_be_bytes());

    // OrElse -> should choose right
    code.push(OpCode::OrElse as u8);

    let module = make_test_module(
        code,
        vec![
            Constant::String("fail".to_string()),
            Constant::String("not found".to_string()),
            Constant::Int(404),
        ],
        vec!["fail".to_string()],
    );

    let mut vm = Vm::new();
    vm.globals
        .insert("fail".to_string(), Value::native("fail", 1, fail_native));
    let result = vm.run(&module).expect("or_else should recover");
    assert_eq!(result, Value::Int(404));
}

#[test]
fn test_attempt_failed_block_recovery() {
    let mut code = Vec::new();
    // Offset 0: PushHandler -> target is offset 10 (failed block)
    code.push(OpCode::PushHandler as u8);
    let mut buf = [0u8; 2];
    BigEndian::write_i16(&mut buf, 7); // 0 + 3 + 7 = 10
    code.extend_from_slice(&buf);

    // Offset 3: Fail("disk full")
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&0u16.to_be_bytes());
    code.push(OpCode::Fail as u8);

    // Offset 7: PopHandler & Jump past failed block (skipped on failure)
    code.push(OpCode::PopHandler as u8);
    code.push(OpCode::Jump as u8);
    BigEndian::write_i16(&mut buf, 6);
    code.extend_from_slice(&buf);

    // Offset 10: Failed block!
    // Stack contains the FailureValue!
    // Extract .message from failure (name idx 0: "message")
    code.push(OpCode::GetField as u8);
    code.extend_from_slice(&0u16.to_be_bytes());

    let module = make_test_module(
        code,
        vec![Constant::String("disk full".to_string())],
        vec!["message".to_string()],
    );

    let result =
        execute(&module).expect("attempt/failed should catch failure and retrieve message");
    assert_eq!(
        result,
        Value::String(std::rc::Rc::new("disk full".to_string()))
    );
}

#[test]
fn test_attempt_failed_block_recovery_with_payload() {
    let mut code = Vec::new();
    // Offset 0: PushHandler -> target is offset 10 (failed block)
    code.push(OpCode::PushHandler as u8);
    let mut buf = [0u8; 2];
    BigEndian::write_i16(&mut buf, 7); // 0 + 3 + 7 = 10
    code.extend_from_slice(&buf);

    // Offset 3: Fail(404)
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&0u16.to_be_bytes());
    code.push(OpCode::Fail as u8);

    // Offset 7: PopHandler & Jump past failed block (skipped on failure)
    code.push(OpCode::PopHandler as u8);
    code.push(OpCode::Jump as u8);
    BigEndian::write_i16(&mut buf, 6);
    code.extend_from_slice(&buf);

    // Offset 10: Failed block!
    // Stack contains the FailureValue!
    // Extract .payload from failure (name idx 0: "payload")
    code.push(OpCode::GetField as u8);
    code.extend_from_slice(&0u16.to_be_bytes());

    let module = make_test_module(code, vec![Constant::Int(404)], vec!["payload".to_string()]);

    let result =
        execute(&module).expect("attempt/failed should catch failure and retrieve payload");
    assert_eq!(result, Value::Int(404));
}

#[test]
fn test_runtime_fault_bypasses_attempt_handler() {
    let mut code = Vec::new();
    // PushHandler
    code.push(OpCode::PushHandler as u8);
    let mut buf = [0u8; 2];
    BigEndian::write_i16(&mut buf, 7);
    code.extend_from_slice(&buf);

    // Divide by zero inside attempt block!
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&0u16.to_be_bytes());
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&1u16.to_be_bytes());
    code.push(OpCode::IntDiv as u8);

    // Handler target
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&2u16.to_be_bytes());

    let module = make_test_module(
        code,
        vec![Constant::Int(10), Constant::Int(0), Constant::Int(999)],
        vec![],
    );

    let err = execute(&module).expect_err("fault must NOT be caught by attempt handler");
    assert_eq!(err.diagnostic_code(), DiagnosticCode::AIPO_RT_DIV_ZERO);
    assert_eq!(err, VmError::Fault(VmFault::DivisionByZero));
}

/// Canon validates `invariant()` at a stable mutable boundary, so a guarded assignment is
/// provisional: when the boundary rejects it, the direct field returns to its entry value and
/// the operation produces a recoverable `Failure` instead of a fault.
#[test]
fn test_guarded_mutation_rolls_back_at_boundary() {
    let mut code = Vec::new();

    // [Bounds{count: 5}]
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&0u16.to_be_bytes());
    code.push(OpCode::BuildStruct as u8);
    code.extend_from_slice(&0u16.to_be_bytes());
    code.extend_from_slice(&1u16.to_be_bytes());
    code.push(0);

    // attempt
    let handler_operand = code.len() + 1;
    code.push(OpCode::PushHandler as u8);
    code.extend_from_slice(&0i16.to_be_bytes());

    // instance.count = -1, applied provisionally until the boundary below
    code.push(OpCode::Dup as u8);
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&1u16.to_be_bytes());
    code.push(OpCode::SetField as u8);
    code.extend_from_slice(&2u16.to_be_bytes());

    // The stable mutable boundary verifies the journaled assignment.
    code.push(OpCode::CheckMutations as u8);
    code.push(OpCode::PopHandler as u8);
    let success_jump = code.len() + 1;
    code.push(OpCode::Jump as u8);
    code.extend_from_slice(&0i16.to_be_bytes());

    // failed: the handler receives the recoverable Failure above the instance it kept
    let handler_ip = code.len();
    BigEndian::write_i16(
        &mut code[handler_operand..handler_operand + 2],
        (handler_ip as i32 - (handler_operand + 2) as i32) as i16,
    );
    code.push(OpCode::Pop as u8);
    code.push(OpCode::GetField as u8);
    code.extend_from_slice(&2u16.to_be_bytes());

    let success_end = code.len();
    BigEndian::write_i16(
        &mut code[success_jump..success_jump + 2],
        (success_end as i32 - (success_jump + 2) as i32) as i16,
    );

    let module = make_test_module(
        code,
        vec![Constant::Int(5), Constant::Int(-1)],
        vec![
            "Bounds".to_string(),
            "count".to_string(),
            "count".to_string(),
        ],
    );

    let mut vm = Vm::new();
    vm.register_struct("Bounds", vec![("count", false)]);
    vm.register_struct_invariant("Bounds", |inst| match inst.get_field("count") {
        Some(Value::Int(count)) if *count >= 0 => Ok(()),
        _ => Err("count must be non-negative".to_string()),
    });

    let result = vm
        .run(&module)
        .expect("a violated mutation is a recoverable failure");
    assert_eq!(
        result,
        Value::Int(5),
        "rollback must preserve the entry value"
    );
}

/// Canon checks `name: Type` at the call boundary and classifies a violation discovered only
/// at runtime as a fault, so `attempt` cannot capture it.
#[test]
fn test_signature_contract_violation_is_a_fault() {
    let mut code = Vec::new();
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&0u16.to_be_bytes());
    code.push(OpCode::AssertContract as u8);
    code.extend_from_slice(&1u16.to_be_bytes());
    code.push(0);
    code.extend_from_slice(&2u16.to_be_bytes());
    code.push(0); // no interface operations required

    let module = make_test_module(
        code,
        vec![Constant::String("no".to_string())],
        vec![
            "no".to_string(),
            "Int".to_string(),
            "parameter `x`".to_string(),
        ],
    );

    let mut vm = Vm::new();
    let err = vm
        .run(&module)
        .expect_err("a contract violation discovered at runtime is a fault");
    assert_eq!(err.diagnostic_code(), DiagnosticCode::AIPO_RT_TYPE_MISMATCH);
    assert!(matches!(
        err,
        VmError::Fault(VmFault::ContractViolation { .. })
    ));
}

/// `T?` means exactly `T` or `none`, so the same instruction accepts both.
#[test]
fn test_nullable_contract_accepts_none() {
    let mut code = Vec::new();
    code.push(OpCode::Nil as u8);
    code.push(OpCode::AssertContract as u8);
    code.extend_from_slice(&0u16.to_be_bytes());
    code.push(1);
    code.extend_from_slice(&1u16.to_be_bytes());
    code.push(0); // no interface operations required

    let module = make_test_module(
        code,
        vec![],
        vec!["Int".to_string(), "parameter `x`".to_string()],
    );

    let result = execute(&module).expect("none satisfies `Int?`");
    assert_eq!(result, Value::None);
}

/// Canon makes an interface structural, so an interface contract asks whether the value
/// exposes the operations it declares — and the receiver is not an argument at a call site,
/// so the declared arity is the caller-visible one.
#[test]
fn test_interface_contract_accepts_a_conforming_operation() {
    let mut code = Vec::new();
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&0u16.to_be_bytes());
    // BuildStruct Drawable's consumer value: one field, published immediately.
    code.push(OpCode::BuildStruct as u8);
    code.extend_from_slice(&1u16.to_be_bytes());
    code.extend_from_slice(&1u16.to_be_bytes());
    code.push(0);
    code.push(OpCode::AssertContract as u8);
    code.extend_from_slice(&2u16.to_be_bytes());
    code.push(0);
    code.extend_from_slice(&3u16.to_be_bytes());
    code.push(1); // one required operation
    code.extend_from_slice(&4u16.to_be_bytes());
    code.push(0); // `draw` takes no argument besides the receiver

    let module = make_test_module(
        code,
        vec![Constant::Int(1)],
        vec![
            "n".to_string(),
            "Thing".to_string(),
            "Drawable".to_string(),
            "parameter `item`".to_string(),
            "draw".to_string(),
        ],
    );

    let mut vm = Vm::new();
    vm.register_struct("Thing", vec![("n", false)]);
    // `total_arity` counts the receiver, so a `fn draw(self)` method is arity 1.
    vm.register_struct_method("Thing", "draw", 0, 1, false);

    let result = vm
        .run(&module)
        .expect("a value that exposes the operation satisfies the interface");
    assert!(matches!(result, Value::Struct(_)));
}

/// A value that does not expose the operation is a contract fault, and the fault names the
/// user type that failed instead of the generic runtime kind.
#[test]
fn test_interface_contract_names_the_failing_struct() {
    let mut code = Vec::new();
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&0u16.to_be_bytes());
    code.push(OpCode::BuildStruct as u8);
    code.extend_from_slice(&1u16.to_be_bytes());
    code.extend_from_slice(&1u16.to_be_bytes());
    code.push(0);
    code.push(OpCode::AssertContract as u8);
    code.extend_from_slice(&2u16.to_be_bytes());
    code.push(0);
    code.extend_from_slice(&3u16.to_be_bytes());
    code.push(1);
    code.extend_from_slice(&4u16.to_be_bytes());
    code.push(0);

    let module = make_test_module(
        code,
        vec![Constant::Int(1)],
        vec![
            "n".to_string(),
            "Blank".to_string(),
            "Drawable".to_string(),
            "parameter `item`".to_string(),
            "draw".to_string(),
        ],
    );

    let mut vm = Vm::new();
    vm.register_struct("Blank", vec![("n", false)]);

    let err = vm
        .run(&module)
        .expect_err("a value without the operation violates the interface contract");
    assert_eq!(err.diagnostic_code(), DiagnosticCode::AIPO_RT_TYPE_MISMATCH);
    let message = err.to_string();
    assert!(
        message.contains("Drawable.draw/0") && message.contains("Blank without 'draw'"),
        "the fault must name the operation and the failing type, got: {message}"
    );
}

/// Canon makes NFC an invariant of `String`, and concatenation is the operation that can join a
/// base character with a combining mark — interpolation lowers to it — so the result is
/// normalized before the program can observe it (ADP-001 Q5, closed by `P00-G15`).
#[test]
fn test_string_concatenation_preserves_nfc() {
    let base = Value::String(std::rc::Rc::new("e".to_string()));
    let mark = Value::String(std::rc::Rc::new("\u{0301}".to_string()));

    let combined = base.add(&mark).expect("String + String is defined");
    assert_eq!(combined, Value::String(std::rc::Rc::new("é".to_string())));
    assert_eq!(
        combined.type_name(),
        "String",
        "the normalized result is still a String"
    );
}

#[test]
fn test_higher_order_method_propagates_fault() {
    let mut code = Vec::new();

    // 1. [1, 2, 3]
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&0u16.to_be_bytes());
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&1u16.to_be_bytes());
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&2u16.to_be_bytes());
    code.push(OpCode::BuildList as u8);
    code.extend_from_slice(&3u16.to_be_bytes());

    // 2. items.filter(predicate)
    code.push(OpCode::GetField as u8);
    code.extend_from_slice(&0u16.to_be_bytes()); // "filter"
    code.push(OpCode::MakeFunction as u8);
    code.extend_from_slice(&0u16.to_be_bytes()); // function 0: predicate
    code.push(OpCode::Call as u8);
    code.push(1);
    code.push(OpCode::Return as u8);

    // Predicate function body: 10 / 0 (div by zero fault!)
    let pred_entry = code.len();
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&0u16.to_be_bytes()); // 10
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&3u16.to_be_bytes()); // 0
    code.push(OpCode::IntDiv as u8);
    code.push(OpCode::Return as u8);

    let mut module = make_test_module(
        code,
        vec![
            Constant::Int(10),
            Constant::Int(2),
            Constant::Int(3),
            Constant::Int(0),
        ],
        vec!["filter".to_string()],
    );
    module.functions.push(FunctionInfo {
        name: "pred".to_string(),
        entry_ip: pred_entry,
        params: 1,
        locals: 0,
        upvalues: 0,
        is_async: false,
    });

    let err = execute(&module).expect_err("predicate division by zero must propagate fault");
    assert_eq!(err.diagnostic_code(), DiagnosticCode::AIPO_RT_DIV_ZERO);
    assert_eq!(err, VmError::Fault(VmFault::DivisionByZero));
}

#[test]
fn test_reduce_propagates_fault_and_releases_iteration_guard() {
    let mut code = Vec::new();

    // 1. var items = [1, 2]
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&0u16.to_be_bytes()); // 1
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&1u16.to_be_bytes()); // 2
    code.push(OpCode::BuildList as u8);
    code.extend_from_slice(&2u16.to_be_bytes());
    code.push(OpCode::SetGlobal as u8);
    code.extend_from_slice(&0u16.to_be_bytes()); // "items"

    // 2. items.reduce(0, fn(acc, item): acc + (10 / 0))
    code.push(OpCode::GetGlobal as u8);
    code.extend_from_slice(&0u16.to_be_bytes()); // "items"
    code.push(OpCode::GetField as u8);
    code.extend_from_slice(&1u16.to_be_bytes()); // "reduce"
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&2u16.to_be_bytes()); // 0 (initial)
    code.push(OpCode::MakeFunction as u8);
    code.extend_from_slice(&0u16.to_be_bytes()); // function 0: callback
    code.push(OpCode::Call as u8);
    code.push(2);
    code.push(OpCode::Return as u8);

    // Callback function body: 10 / 0
    let fn_entry = code.len();
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&3u16.to_be_bytes()); // 10
    code.push(OpCode::Constant as u8);
    code.extend_from_slice(&2u16.to_be_bytes()); // 0
    code.push(OpCode::IntDiv as u8);
    code.push(OpCode::Return as u8);

    let mut module = make_test_module(
        code,
        vec![
            Constant::Int(1),
            Constant::Int(2),
            Constant::Int(0),
            Constant::Int(10),
        ],
        vec!["items".to_string(), "reduce".to_string(), "add".to_string()],
    );
    module.functions.push(FunctionInfo {
        name: "cb".to_string(),
        entry_ip: fn_entry,
        params: 2,
        locals: 0,
        upvalues: 0,
        is_async: false,
    });

    let mut vm = Vm::new();
    let err = vm
        .run(&module)
        .expect_err("reduce with div by zero must fault");
    assert_eq!(err.diagnostic_code(), DiagnosticCode::AIPO_RT_DIV_ZERO);

    // Verify that the iteration guard on `items` was released even after the fault:
    let items_val = vm
        .globals
        .get("items")
        .cloned()
        .expect("items global exists");
    if let Value::List(l) = items_val {
        // Mutating the list directly must succeed without MutationDuringIteration
        l.borrow_mut().push(Value::Int(99));
        assert_eq!(l.borrow().len(), 3);
    } else {
        panic!("expected list in global");
    }
}
