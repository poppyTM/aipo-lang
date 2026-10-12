//! Automated integration tests for the Aipo WebAssembly emitter (ADP-013).

use aipo_wasm::{Instruction, WasmEmitter, WasmFnType, WasmType};
use wasm_encoder::Function;

#[test]
fn test_wasm_module_magic_and_version_header() {
    let emitter = WasmEmitter::new();
    let bytes = emitter.finish();

    assert!(bytes.len() >= 8);
    // Wasm binary magic bytes: \0asm
    assert_eq!(&bytes[0..4], b"\0asm");
    // Wasm binary version 1: 0x01, 0x00, 0x00, 0x00
    assert_eq!(&bytes[4..8], &[0x01, 0x00, 0x00, 0x00]);
}

#[test]
fn test_emit_exported_arithmetic_function() {
    let mut emitter = WasmEmitter::new();

    // Signature: (i64, i64) -> i64
    let fn_type = WasmFnType::new(vec![WasmType::I64, WasmType::I64], vec![WasmType::I64]);
    let type_idx = emitter.add_type(fn_type);

    // Body: local.get 0; local.get 1; i64.add; end
    let mut func = Function::new([]);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Add);
    func.instruction(&Instruction::End);

    let func_idx = emitter.add_function(type_idx, func);
    emitter.export_function("add", func_idx);

    let bytes = emitter.finish();

    // Verify valid WebAssembly binary header
    assert_eq!(&bytes[0..4], b"\0asm");
    assert_eq!(&bytes[4..8], &[0x01, 0x00, 0x00, 0x00]);
    // The exported name "add" should appear in the export section
    assert!(bytes.windows(3).any(|w| w == b"add"));
}

#[test]
fn test_type_deduplication_and_multiple_functions() {
    let mut emitter = WasmEmitter::new();

    // Add identical signature twice
    let sig1 = WasmFnType::new(vec![WasmType::F64, WasmType::F64], vec![WasmType::F64]);
    let sig2 = WasmFnType::new(vec![WasmType::F64, WasmType::F64], vec![WasmType::F64]);

    let type1 = emitter.add_type(sig1);
    let type2 = emitter.add_type(sig2);

    // Must be deduplicated to the same type index
    assert_eq!(type1, type2);

    // Function 1: f64_add
    let mut f1 = Function::new([]);
    f1.instruction(&Instruction::LocalGet(0));
    f1.instruction(&Instruction::LocalGet(1));
    f1.instruction(&Instruction::F64Add);
    f1.instruction(&Instruction::End);
    let idx1 = emitter.add_function(type1, f1);
    emitter.export_function("f64_add", idx1);

    // Function 2: f64_mul
    let mut f2 = Function::new([]);
    f2.instruction(&Instruction::LocalGet(0));
    f2.instruction(&Instruction::LocalGet(1));
    f2.instruction(&Instruction::F64Mul);
    f2.instruction(&Instruction::End);
    let idx2 = emitter.add_function(type1, f2);
    emitter.export_function("f64_mul", idx2);

    let bytes = emitter.finish();
    assert_eq!(&bytes[0..4], b"\0asm");
    assert!(bytes.windows(7).any(|w| w == b"f64_add"));
    assert!(bytes.windows(7).any(|w| w == b"f64_mul"));
}

#[cfg(feature = "wasmtime")]
#[test]
fn test_wasmtime_execution_of_emitted_module() {
    let mut emitter = WasmEmitter::new();

    // Exported function `add(i64, i64) -> i64`
    let fn_type = WasmFnType::new(vec![WasmType::I64, WasmType::I64], vec![WasmType::I64]);
    let type_idx = emitter.add_type(fn_type);

    let mut func = Function::new([]);
    func.instruction(&Instruction::LocalGet(0));
    func.instruction(&Instruction::LocalGet(1));
    func.instruction(&Instruction::I64Add);
    func.instruction(&Instruction::End);

    let func_idx = emitter.add_function(type_idx, func);
    emitter.export_function("add", func_idx);

    let wasm_bytes = emitter.finish();

    // Verify JIT execution via Wasmtime
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm_bytes).expect("wasm module valid");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).expect("wasm instantiation");
    let add_func = instance
        .get_typed_func::<(i64, i64), i64>(&mut store, "add")
        .expect("exported function exists");

    let result = add_func.call(&mut store, (15, 27)).expect("call succeeds");
    assert_eq!(result, 42);
}

#[cfg(feature = "wasmtime")]
#[test]
fn test_wasmtime_execution_of_call_indirect() {
    let mut emitter = WasmEmitter::new();

    // Signature 0: (i64) -> i64 (for square & double)
    let math_sig = WasmFnType::new(vec![WasmType::I64], vec![WasmType::I64]);
    let math_type_idx = emitter.add_type(math_sig);

    // Signature 1: (i32, i64) -> i64 (for invoke_indirect)
    let dispatcher_sig = WasmFnType::new(vec![WasmType::I32, WasmType::I64], vec![WasmType::I64]);
    let dispatcher_type_idx = emitter.add_type(dispatcher_sig);

    // Func 0: square(x: i64) -> i64
    let mut f0 = Function::new([]);
    f0.instruction(&Instruction::LocalGet(0));
    f0.instruction(&Instruction::LocalGet(0));
    f0.instruction(&Instruction::I64Mul);
    f0.instruction(&Instruction::End);
    let f0_idx = emitter.add_function(math_type_idx, f0);

    // Func 1: double(x: i64) -> i64
    let mut f1 = Function::new([]);
    f1.instruction(&Instruction::LocalGet(0));
    f1.instruction(&Instruction::LocalGet(0));
    f1.instruction(&Instruction::I64Add);
    f1.instruction(&Instruction::End);
    let f1_idx = emitter.add_function(math_type_idx, f1);

    // Table 0: 2 function references
    emitter.enable_table(2, Some(2));
    emitter.add_element_segment(0, 0, vec![f0_idx, f1_idx]);

    // Func 2: dispatch(table_idx: i32, arg: i64) -> i64
    let mut f2 = Function::new([]);
    f2.instruction(&Instruction::LocalGet(1)); // push argument
    f2.instruction(&Instruction::LocalGet(0)); // push table index
    f2.instruction(&Instruction::CallIndirect {
        type_index: math_type_idx,
        table_index: 0,
    });
    f2.instruction(&Instruction::End);
    let f2_idx = emitter.add_function(dispatcher_type_idx, f2);
    emitter.export_function("dispatch", f2_idx);

    let wasm_bytes = emitter.finish();

    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, &wasm_bytes).expect("wasm module valid");
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).expect("wasm instantiation");
    let dispatch_func = instance
        .get_typed_func::<(i32, i64), i64>(&mut store, "dispatch")
        .expect("exported function exists");

    // Call table element 0 (square(7)) => 49
    assert_eq!(dispatch_func.call(&mut store, (0, 7)).unwrap(), 49);

    // Call table element 1 (double(7)) => 14
    assert_eq!(dispatch_func.call(&mut store, (1, 7)).unwrap(), 14);
}
