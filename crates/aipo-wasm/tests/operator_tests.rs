#![cfg(feature = "wasmtime")]

//! Tests for logical and range operators in the Wasm backend.
//!
//! Covers `and`, `or` (with short-circuit evaluation) and `..` range construction.

use aipo_hir::lower;
use aipo_source::{Source, SourceId};
use aipo_syntax::parse;
use aipo_wasm::compile_hir;
use wasmtime::{Engine, Instance, Module, Store};

fn instantiate_aipo(source_code: &str) -> (Store<()>, Instance) {
    let source = Source::new(SourceId::next(), "test.aipo", source_code);
    let (ast, diags) = parse(&source);
    assert!(diags.is_empty(), "parse diagnostics: {diags:?}");

    let hir = lower(ast);
    let wasm_bytes = compile_hir(&hir).expect("compilation to Wasm succeeds");

    let engine = Engine::default();
    let module = Module::new(&engine, &wasm_bytes).expect("valid WebAssembly module");
    let mut store = Store::new(&engine, ());
    let instance =
        Instance::new(&mut store, &module, &[]).expect("WebAssembly instantiation succeeds");

    (store, instance)
}

#[test]
fn test_and_operator_truth_table() {
    let code = r#"
fn both(a: Bool, b: Bool) -> Bool {
  return a and b
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "both")
        .expect("exported `both`");

    assert_eq!(f.call(&mut store, (1, 1)).unwrap(), 1);
    assert_eq!(f.call(&mut store, (1, 0)).unwrap(), 0);
    assert_eq!(f.call(&mut store, (0, 1)).unwrap(), 0);
    assert_eq!(f.call(&mut store, (0, 0)).unwrap(), 0);
}

#[test]
fn test_or_operator_truth_table() {
    let code = r#"
fn either(a: Bool, b: Bool) -> Bool {
  return a or b
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(i32, i32), i32>(&mut store, "either")
        .expect("exported `either`");

    assert_eq!(f.call(&mut store, (1, 1)).unwrap(), 1);
    assert_eq!(f.call(&mut store, (1, 0)).unwrap(), 1);
    assert_eq!(f.call(&mut store, (0, 1)).unwrap(), 1);
    assert_eq!(f.call(&mut store, (0, 0)).unwrap(), 0);
}

#[test]
fn test_and_short_circuits_right_operand() {
    // `false and 1 / 0` must return 0 without trapping: the right operand
    // is never evaluated.
    let code = r#"
fn short_circuit_and() -> Bool {
  let flag = false
  return flag and (1 // 0) == 0
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i32>(&mut store, "short_circuit_and")
        .expect("exported `short_circuit_and`");

    assert_eq!(f.call(&mut store, ()).unwrap(), 0);
}

#[test]
fn test_or_short_circuits_right_operand() {
    // `true or 1 / 0` must return true without trapping.
    let code = r#"
fn short_circuit_or() -> Bool {
  let flag = true
  return flag or (1 // 0) == 0
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i32>(&mut store, "short_circuit_or")
        .expect("exported `short_circuit_or`");

    assert_eq!(f.call(&mut store, ()).unwrap(), 1);
}

#[test]
fn test_and_evaluates_right_when_left_is_true() {
    let code = r#"
fn and_right_evaluated() -> Bool {
  let flag = true
  return flag and false
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i32>(&mut store, "and_right_evaluated")
        .expect("exported `and_right_evaluated`");

    assert_eq!(f.call(&mut store, ()).unwrap(), 0);
}

#[test]
fn test_or_evaluates_right_when_left_is_false() {
    let code = r#"
fn or_right_evaluated() -> Bool {
  let flag = false
  return flag or true
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i32>(&mut store, "or_right_evaluated")
        .expect("exported `or_right_evaluated`");

    assert_eq!(f.call(&mut store, ()).unwrap(), 1);
}

#[test]
fn test_logical_operators_chain_with_comparisons() {
    let code = r#"
fn in_bounds(v: Int) -> Bool {
  return v >= 0 and v <= 100
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<i64, i32>(&mut store, "in_bounds")
        .expect("exported `in_bounds`");

    assert_eq!(f.call(&mut store, 50).unwrap(), 1);
    assert_eq!(f.call(&mut store, 0).unwrap(), 1);
    assert_eq!(f.call(&mut store, 100).unwrap(), 1);
    assert_eq!(f.call(&mut store, -1).unwrap(), 0);
    assert_eq!(f.call(&mut store, 101).unwrap(), 0);
}

#[test]
fn test_range_len_runtime_helper() {
    let code = r#"
fn range_span() -> Int {
  let r = 2..7
  return __aipo_range_len(r)
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "range_span")
        .expect("exported `range_span`");

    assert_eq!(f.call(&mut store, ()).unwrap(), 5);
}

#[test]
fn test_empty_range_len_is_zero() {
    let code = r#"
fn empty_range() -> Int {
  let r = 5..5
  return __aipo_range_len(r)
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "empty_range")
        .expect("exported `empty_range`");

    assert_eq!(f.call(&mut store, ()).unwrap(), 0);
}

#[test]
fn test_descending_range_len_is_zero() {
    let code = r#"
fn descending() -> Int {
  let r = 9..3
  return __aipo_range_len(r)
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "descending")
        .expect("exported `descending`");

    assert_eq!(f.call(&mut store, ()).unwrap(), 0);
}

#[test]
fn test_range_bounds_are_preserved() {
    let code = r#"
fn make_range() {
  let r = 11..20
  return r
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i32>(&mut store, "make_range")
        .expect("exported `make_range`");

    let range_ptr = f.call(&mut store, ()).unwrap() as usize;
    let memory = instance
        .get_memory(&mut store, "memory")
        .expect("exported `memory`");

    let mut start_bytes = [0u8; 8];
    memory
        .read(&store, range_ptr, &mut start_bytes)
        .expect("read start");
    let start = i64::from_le_bytes(start_bytes);

    let mut end_bytes = [0u8; 8];
    memory
        .read(&store, range_ptr + 8, &mut end_bytes)
        .expect("read end");
    let end = i64::from_le_bytes(end_bytes);

    assert_eq!(start, 11);
    assert_eq!(end, 20);
}
