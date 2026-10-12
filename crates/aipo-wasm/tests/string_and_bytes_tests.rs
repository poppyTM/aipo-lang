//! End-to-end integration tests for dynamic Strings and Bytes substrate in Wasm.

use aipo_hir::lower;
use aipo_source::{Source, SourceId};
use aipo_syntax::parse;
use aipo_wasm::{compile_hir, execute_wasm};
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
fn test_string_concatenation_and_equality() {
    let code = r#"
fn check_concat() -> Int {
  let a = "hello"
  let b = " world"
  let c = a + b
  if c == "hello world" {
    return 1
  }
  return 0
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "check_concat")
        .expect("exported `check_concat`");

    assert_eq!(f.call(&mut store, ()).unwrap(), 1);
}

#[test]
fn test_string_dynamic_conversions_and_compound_assign() {
    let code = r#"
fn build_message() -> Int {
  var s = "count: "
  s += 42
  s += ", ok: "
  s += true
  if s == "count: 42, ok: true" {
    return 100
  }
  return 0
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "build_message")
        .expect("exported `build_message`");

    assert_eq!(f.call(&mut store, ()).unwrap(), 100);
}

#[test]
fn test_string_constructor_call() {
    let code = r#"
fn str_conv() -> Int {
  let s1 = String(123)
  let s2 = String(false)
  if s1 == "123" and s2 == "false" {
    return 1
  }
  return 0
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "str_conv")
        .expect("exported `str_conv`");

    assert_eq!(f.call(&mut store, ()).unwrap(), 1);
}

#[test]
fn test_bytes_substrate_indexing_and_len() {
    let code = r#"
fn test_bytes() -> Int {
  var b = Bytes(16)
  if b.len != 16 {
    return -1
  }
  b[0] = 10
  b[1] = 20
  b[0] += 5
  return b[0] + b[1]
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "test_bytes")
        .expect("exported `test_bytes`");

    // 15 + 20 = 35
    assert_eq!(f.call(&mut store, ()).unwrap(), 35);
}

#[test]
fn test_bytes_packing_and_unpacking() {
    let code = r#"
fn test_pack() -> Int {
  var b = Bytes(32)
  b.write_i32_le(0, 100000)
  b.write_i16_le(4, 1234)
  b.write_u8(6, 250)

  let v32 = b.read_i32_le(0)
  let v16 = b.read_i16_le(4)
  let v8 = b.read_u8(6)

  if v32 == 100000 and v16 == 1234 and v8 == 250 {
    return 1
  }
  return 0
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "test_pack")
        .expect("exported `test_pack`");

    assert_eq!(f.call(&mut store, ()).unwrap(), 1);
}

#[test]
fn test_bytes_decode_to_string() {
    let code = r#"
fn test_decode() -> Int {
  var b = Bytes(5)
  b.write_u8(0, 72)
  b.write_u8(1, 101)
  b.write_u8(2, 108)
  b.write_u8(3, 108)
  b.write_u8(4, 111)

  let s = b.decode()
  if s == "Hello" {
    return 1
  }
  return 0
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "test_decode")
        .expect("exported `test_decode`");

    assert_eq!(f.call(&mut store, ()).unwrap(), 1);
}

#[test]
fn test_bytes_and_string_io_execution() {
    let code = r#"
var b = Bytes(4)
b.write_u8(0, 65)
b.write_u8(1, 105)
b.write_u8(2, 112)
b.write_u8(3, 111)
let s = b.decode()
io.println(s + " Wasm " + 1)
"#;
    let source = Source::new(SourceId::next(), "io_test.aipo", code);
    let (ast, diags) = parse(&source);
    assert!(diags.is_empty(), "parse diagnostics: {diags:?}");
    let hir = lower(ast);
    let wasm_bytes = compile_hir(&hir).expect("compilation succeeds");

    let mut stdout = Vec::new();
    let res = execute_wasm(&wasm_bytes, &mut stdout).expect("execution succeeds");
    assert_eq!(res, 0);
    assert_eq!(String::from_utf8(stdout).unwrap(), "Aipo Wasm 1\n");
}
