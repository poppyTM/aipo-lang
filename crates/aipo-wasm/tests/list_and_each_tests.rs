#![cfg(feature = "wasmtime")]

//! End-to-end tests for List literals, Indexing, and Each iteration in Wasm.

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
fn test_list_literal_construction_and_indexing() {
    let code = r#"
fn get_first() -> Int {
  let xs = [10, 20, 30]
  return xs[0]
}

fn get_last() -> Int {
  let xs = [10, 20, 30]
  return xs[2]
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f1 = instance
        .get_typed_func::<(), i64>(&mut store, "get_first")
        .expect("exported `get_first`");
    let f2 = instance
        .get_typed_func::<(), i64>(&mut store, "get_last")
        .expect("exported `get_last`");

    assert_eq!(f1.call(&mut store, ()).unwrap(), 10);
    assert_eq!(f2.call(&mut store, ()).unwrap(), 30);
}

#[test]
fn test_list_len_property() {
    let code = r#"
fn list_size() -> Int {
  let xs = [1, 2, 3, 4, 5]
  return xs.len
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "list_size")
        .expect("exported `list_size`");

    assert_eq!(f.call(&mut store, ()).unwrap(), 5);
}

#[test]
fn test_each_loop_over_range() {
    let code = r#"
fn sum_range() -> Int {
  var acc = 0
  each x in 1..6 {
    acc = acc + x
  }
  return acc
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "sum_range")
        .expect("exported `sum_range`");

    // 1 + 2 + 3 + 4 + 5 = 15
    assert_eq!(f.call(&mut store, ()).unwrap(), 15);
}

#[test]
fn test_each_loop_over_list() {
    let code = r#"
fn sum_list() -> Int {
  let xs = [3, 7, 11]
  var acc = 0
  each x in xs {
    acc = acc + x
  }
  return acc
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "sum_list")
        .expect("exported `sum_list`");

    // 3 + 7 + 11 = 21
    assert_eq!(f.call(&mut store, ()).unwrap(), 21);
}

#[test]
fn test_each_loop_with_index() {
    let code = r#"
fn sum_with_indices() -> Int {
  let xs = [100, 200, 300]
  var acc = 0
  each x, i in xs {
    acc = acc + x * i
  }
  return acc
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "sum_with_indices")
        .expect("exported `sum_with_indices`");

    // 100*0 + 200*1 + 300*2 = 800
    assert_eq!(f.call(&mut store, ()).unwrap(), 800);
}

#[test]
fn test_each_loop_break() {
    let code = r#"
fn sum_until_limit() -> Int {
  var acc = 0
  each x in 1..100 {
    if x > 5 {
      break
    }
    acc = acc + x
  }
  return acc
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "sum_until_limit")
        .expect("exported `sum_until_limit`");

    // 1 + 2 + 3 + 4 + 5 = 15
    assert_eq!(f.call(&mut store, ()).unwrap(), 15);
}

#[test]
fn test_each_loop_continue() {
    let code = r#"
fn sum_odds() -> Int {
  var acc = 0
  each x in 1..7 {
    if x % 2 == 0 {
      continue
    }
    acc = acc + x
  }
  return acc
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "sum_odds")
        .expect("exported `sum_odds`");

    // 1 + 3 + 5 = 9
    assert_eq!(f.call(&mut store, ()).unwrap(), 9);
}
