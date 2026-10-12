#![cfg(feature = "wasmtime")]

//! End-to-end tests for the hybrid `Failure` model in the Wasm backend.
//!
//! `fail`, `or_else`, `attempt`/`failed` use module-level status globals plus
//! statement-boundary propagation, mirroring the stack VM's `CheckFailure`.

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
fn test_fail_raises_and_propagates() {
    let code = r#"
fn boom() -> Int {
  fail ("kaboom")
  return 0
}

fn caller() -> Int {
  let v = boom()
  return v
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "caller")
        .expect("exported `caller`");

    // `caller` returns immediately with the dummy value because a failure is pending.
    assert_eq!(f.call(&mut store, ()).unwrap(), 0);

    let status = instance
        .get_global(&mut store, "__aipo_failure_status")
        .expect("exported `__aipo_failure_status`");
    assert_eq!(status.get(&mut store).i32(), Some(1));
}

#[test]
fn test_or_else_supplies_fallback_on_failure() {
    let code = r#"
fn boom() -> Int {
  fail ("kaboom")
  return 0
}

fn safe() -> Int {
  let v = boom() or_else 42
  return v
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "safe")
        .expect("exported `safe`");

    assert_eq!(f.call(&mut store, ()).unwrap(), 42);

    let status = instance
        .get_global(&mut store, "__aipo_failure_status")
        .expect("exported `__aipo_failure_status`");
    assert_eq!(status.get(&mut store).i32(), Some(0));
}

#[test]
fn test_or_else_passes_through_when_no_failure() {
    let code = r#"
fn fine() -> Int {
  return 7
}

fn safe() -> Int {
  let v = fine() or_else 42
  return v
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "safe")
        .expect("exported `safe`");

    assert_eq!(f.call(&mut store, ()).unwrap(), 7);
}

#[test]
fn test_attempt_catches_failure_and_runs_handler() {
    let code = r#"
fn boom() -> Int {
  fail ("kaboom")
  return 0
}

fn guarded() -> Int {
  var caught = 0
  attempt {
    let v = boom()
    caught = v
  } failed err {
    caught = 1
  }
  return caught
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "guarded")
        .expect("exported `guarded`");

    // The handler ran, setting `caught` to 1.
    assert_eq!(f.call(&mut store, ()).unwrap(), 1);

    let status = instance
        .get_global(&mut store, "__aipo_failure_status")
        .expect("exported `__aipo_failure_status`");
    assert_eq!(status.get(&mut store).i32(), Some(0));
}

#[test]
fn test_attempt_without_failure_skips_handler() {
    let code = r#"
fn fine() -> Int {
  return 7
}

fn guarded() -> Int {
  var caught = 0
  attempt {
    let v = fine()
    caught = v
  } failed err {
    caught = 99
  }
  return caught
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "guarded")
        .expect("exported `guarded`");

    assert_eq!(f.call(&mut store, ()).unwrap(), 7);
}

#[test]
fn test_failure_does_not_leak_into_later_statements() {
    let code = r#"
fn boom() -> Int {
  fail ("nope")
  return 0
}

fn recovered() -> Int {
  var total = 0
  let bad = boom() or_else 5
  total = total + bad
  return total
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "recovered")
        .expect("exported `recovered`");

    assert_eq!(f.call(&mut store, ()).unwrap(), 5);
}

#[test]
fn test_or_else_chains() {
    let code = r#"
fn boom() -> Int {
  fail ("nope")
  return 0
}

fn chained() -> Int {
  let v = boom() or_else (boom() or_else 3)
  return v
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "chained")
        .expect("exported `chained`");

    assert_eq!(f.call(&mut store, ()).unwrap(), 3);
}
