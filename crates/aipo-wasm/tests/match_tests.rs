#![cfg(feature = "wasmtime")]

//! End-to-end tests for `match` statements in the Wasm backend.

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
fn test_match_on_int_arms() {
    let code = r#"
fn classify(v: Int) -> Int {
  match v {
    when 1
      return 100
    when 2
      return 200
    when 3
      return 300
    else
      return 0
  }
}
"#;
    let source = Source::new(SourceId::next(), "test.aipo", code);
    let (ast, diags) = parse(&source);
    assert!(diags.is_empty(), "parse diagnostics: {diags:?}");
    let hir = lower(ast);
    let wasm_bytes = compile_hir(&hir).expect("compilation to Wasm succeeds");
    let engine = Engine::default();
    let module = Module::new(&engine, &wasm_bytes).expect("valid module");
    let mut store = Store::new(&engine, ());
    let instance = Instance::new(&mut store, &module, &[]).expect("instantiation");
    let f = instance
        .get_typed_func::<i64, i64>(&mut store, "classify")
        .expect("exported `classify`");

    assert_eq!(f.call(&mut store, 1).unwrap(), 100);
    assert_eq!(f.call(&mut store, 2).unwrap(), 200);
    assert_eq!(f.call(&mut store, 3).unwrap(), 300);
    assert_eq!(f.call(&mut store, 9).unwrap(), 0);
}

#[test]
fn test_match_with_multi_pattern_arm() {
    let code = r#"
fn bucket(v: Int) -> Int {
  match v {
    when 0, 1, 2
      return 10
    when 3, 4
      return 20
    else
      return 99
  }
}
"#;
    let source = Source::new(SourceId::next(), "test.aipo", code);
    let (ast, diags) = parse(&source);
    assert!(diags.is_empty(), "parse diagnostics: {diags:?}");
    let hir = lower(ast);
    let wasm_bytes = compile_hir(&hir).expect("compilation to Wasm succeeds");
    let engine = Engine::default();
    let module = Module::new(&engine, &wasm_bytes).expect("valid module");
    let mut store = Store::new(&engine, ());
    let instance = Instance::new(&mut store, &module, &[]).expect("instantiation");
    let f = instance
        .get_typed_func::<i64, i64>(&mut store, "bucket")
        .expect("exported `bucket`");

    assert_eq!(f.call(&mut store, 0).unwrap(), 10);
    assert_eq!(f.call(&mut store, 2).unwrap(), 10);
    assert_eq!(f.call(&mut store, 3).unwrap(), 20);
    assert_eq!(f.call(&mut store, 4).unwrap(), 20);
    assert_eq!(f.call(&mut store, 7).unwrap(), 99);
}

#[test]
fn test_match_on_bool() {
    let code = r#"
fn describe(v: Bool) -> Int {
  match v {
    when true
      return 1
    else
      return 0
  }
}
"#;
    let source = Source::new(SourceId::next(), "test.aipo", code);
    let (ast, diags) = parse(&source);
    assert!(diags.is_empty(), "parse diagnostics: {diags:?}");
    let hir = lower(ast);
    let wasm_bytes = compile_hir(&hir).expect("compilation to Wasm succeeds");
    let engine = Engine::default();
    let module = Module::new(&engine, &wasm_bytes).expect("valid module");
    let mut store = Store::new(&engine, ());
    let instance = Instance::new(&mut store, &module, &[]).expect("instantiation");
    let f = instance
        .get_typed_func::<i32, i64>(&mut store, "describe")
        .expect("exported `describe`");

    assert_eq!(f.call(&mut store, 1).unwrap(), 1);
    assert_eq!(f.call(&mut store, 0).unwrap(), 0);
}

#[test]
fn test_match_selects_first_matching_arm() {
    let code = r#"
fn first_wins(v: Int) -> Int {
  match v {
    when 5
      return 1
    when 5
      return 2
    else
      return 3
  }
}
"#;
    let source = Source::new(SourceId::next(), "test.aipo", code);
    let (ast, diags) = parse(&source);
    assert!(diags.is_empty(), "parse diagnostics: {diags:?}");
    let hir = lower(ast);
    let wasm_bytes = compile_hir(&hir).expect("compilation to Wasm succeeds");
    let engine = Engine::default();
    let module = Module::new(&engine, &wasm_bytes).expect("valid module");
    let mut store = Store::new(&engine, ());
    let instance = Instance::new(&mut store, &module, &[]).expect("instantiation");
    let f = instance
        .get_typed_func::<i64, i64>(&mut store, "first_wins")
        .expect("exported `first_wins`");

    assert_eq!(f.call(&mut store, 5).unwrap(), 1);
}

#[test]
fn test_match_variant_execution() {
    let code = r#"
enum Status {
  On,
  Off,
}
fn test_on() -> Int {
  let s = Status.On
  match s {
    when Status.On {
      return 1
    }
    when Status.Off {
      return 0
    }
    else {
      return -1
    }
  }
}
fn test_off() -> Int {
  let s = Status.Off
  match s {
    when Status.On {
      return 1
    }
    when Status.Off {
      return 0
    }
    else {
      return -1
    }
  }
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f_on = instance
        .get_typed_func::<(), i64>(&mut store, "test_on")
        .expect("exported `test_on`");
    assert_eq!(f_on.call(&mut store, ()).unwrap(), 1);
    let f_off = instance
        .get_typed_func::<(), i64>(&mut store, "test_off")
        .expect("exported `test_off`");
    assert_eq!(f_off.call(&mut store, ()).unwrap(), 0);
}

#[test]
fn test_match_variant_payload_execution() {
    let code = r#"
enum Message {
  Quit,
  Move(x: Int, y: Int),
}
fn get_coords() -> Int {
  let m = Message.Move(15, 27)
  match m {
    when Message.Move(x, y) {
      return x + y
    }
    else {
      return 0
    }
  }
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "get_coords")
        .expect("exported `get_coords`");
    assert_eq!(f.call(&mut store, ()).unwrap(), 42);
}

#[test]
fn test_empty_collections_execution() {
    let code = r#"
fn check_empty_list() -> Int {
  let l = []
  return l.len()
}
fn check_empty_dict() -> Int {
  let d = {}
  return d.len()
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f_list = instance
        .get_typed_func::<(), i64>(&mut store, "check_empty_list")
        .expect("exported `check_empty_list`");
    assert_eq!(f_list.call(&mut store, ()).unwrap(), 0);
    let f_dict = instance
        .get_typed_func::<(), i64>(&mut store, "check_empty_dict")
        .expect("exported `check_empty_dict`");
    assert_eq!(f_dict.call(&mut store, ()).unwrap(), 0);
}

#[test]
fn test_match_with_guard() {
    let code = r#"
fn check_num(x: Int) -> Int {
  match x {
    when 10 if true {
      return 100
    }
    when 10 if false {
      return 200
    }
    else {
      return 0
    }
  }
}
"#;
    let source = Source::new(SourceId::next(), "test.aipo", code);
    let (ast, diags) = parse(&source);
    assert!(diags.is_empty(), "parse diagnostics: {diags:?}");
    let hir = lower(ast);
    let wasm_bytes = compile_hir(&hir).expect("compilation to Wasm succeeds");
    let engine = Engine::default();
    let module = Module::new(&engine, &wasm_bytes).expect("valid module");
    let mut store = Store::new(&engine, ());
    let instance = Instance::new(&mut store, &module, &[]).expect("instantiation");
    let f = instance
        .get_typed_func::<i64, i64>(&mut store, "check_num")
        .expect("exported function");

    assert_eq!(f.call(&mut store, 10).unwrap(), 100);
    assert_eq!(f.call(&mut store, 5).unwrap(), 0);
}

#[test]
fn test_match_destructure_struct() {
    let code = r#"
struct Point {
  x: Int,
  y: Int,
}
fn sum_coords(p: Point) -> Int {
  match p {
    when { x, y } {
      return x + y
    }
    else {
      return 0
    }
  }
}
"#;
    let source = Source::new(SourceId::next(), "test.aipo", code);
    let (ast, diags) = parse(&source);
    assert!(diags.is_empty(), "parse diagnostics: {diags:?}");
    let hir = lower(ast);
    let wasm_bytes = compile_hir(&hir).expect("compilation to Wasm succeeds");
    let engine = Engine::default();
    let module = Module::new(&engine, &wasm_bytes).expect("valid module");
    let mut store = Store::new(&engine, ());
    let instance = Instance::new(&mut store, &module, &[]).expect("instantiation");
    let f = instance
        .get_typed_func::<i64, i64>(&mut store, "sum_coords")
        .expect("exported function");

    // Allocate Point in linear memory
    let alloc = instance
        .get_typed_func::<i32, i32>(&mut store, "__aipo_alloc")
        .expect("allocator");
    let ptr = alloc.call(&mut store, 16).unwrap();

    let memory = instance.get_memory(&mut store, "memory").unwrap();
    memory
        .write(&mut store, ptr as usize, &15i64.to_le_bytes())
        .unwrap();
    memory
        .write(&mut store, (ptr + 8) as usize, &27i64.to_le_bytes())
        .unwrap();

    assert_eq!(f.call(&mut store, ptr as i64).unwrap(), 42);
}
