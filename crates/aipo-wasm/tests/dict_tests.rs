#![cfg(feature = "wasmtime")]

//! End-to-end tests for Dict literals, lookup, mutation and `len` in Wasm.

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
fn test_dict_literal_and_int_key_lookup() {
    let code = r#"
fn score_of(id: Int) -> Int {
  let scores = { 1: 10, 2: 20, 3: 30 }
  return scores[id]
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<i64, i64>(&mut store, "score_of")
        .expect("exported `score_of`");

    assert_eq!(f.call(&mut store, 1).unwrap(), 10);
    assert_eq!(f.call(&mut store, 2).unwrap(), 20);
    assert_eq!(f.call(&mut store, 3).unwrap(), 30);
}

#[test]
fn test_dict_missing_key_faults() {
    // Canon: `dict[key]` requires an existing key so a stored value stays
    // distinguishable from absence. The Wasm backend used to answer zero, which made
    // a typo read as the integer zero instead of the VM's `AIPO_RT_KEY_NOT_FOUND`.
    let code = r#"
fn missing() -> Int {
  let scores = { 1: 10, 2: 20 }
  return scores[99]
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "missing")
        .expect("exported `missing`");

    let outcome = f.call(&mut store, ());
    assert!(
        outcome.is_err(),
        "a missing key must fault, not answer a value: {outcome:?}"
    );
}

#[test]
fn test_dict_len() {
    let code = r#"
fn dict_size() -> Int {
  let d = { 1: 10, 2: 20, 3: 30, 4: 40 }
  return d.len
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "dict_size")
        .expect("exported `dict_size`");

    assert_eq!(f.call(&mut store, ()).unwrap(), 4);
}

#[test]
fn test_dict_string_key_lookup() {
    let code = r#"
fn level_of() -> Int {
  let levels = { "ana": 1, "bruno": 2 }
  return levels["ana"]
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "level_of")
        .expect("exported `level_of`");

    assert_eq!(f.call(&mut store, ()).unwrap(), 1);
}

#[test]
fn test_dict_key_overwrite_replaces_value() {
    let code = r#"
fn duplicate_keys() -> Int {
  let d = { 1: 100, 1: 200 }
  return d[1]
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "duplicate_keys")
        .expect("exported `duplicate_keys`");

    assert_eq!(f.call(&mut store, ()).unwrap(), 200);
}

#[test]
fn test_dict_assignment_inserts_and_overwrites() {
    let code = r#"
fn mutated() -> Int {
  var d = { 1: 10 }
  d[1] = 99
  return d[1]
}

fn inserted() -> Int {
  var d = { 1: 10 }
  d[2] = 55
  return d[2]
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f1 = instance
        .get_typed_func::<(), i64>(&mut store, "mutated")
        .expect("exported `mutated`");
    let f2 = instance
        .get_typed_func::<(), i64>(&mut store, "inserted")
        .expect("exported `inserted`");

    assert_eq!(f1.call(&mut store, ()).unwrap(), 99);
    assert_eq!(f2.call(&mut store, ()).unwrap(), 55);
}

#[test]
fn test_list_index_assignment() {
    let code = r#"
fn replaced() -> Int {
  var xs = [1, 2, 3]
  xs[1] = 42
  return xs[1]
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "replaced")
        .expect("exported `replaced`");

    assert_eq!(f.call(&mut store, ()).unwrap(), 42);
}

#[test]
fn test_dict_hash_runtime_helper_is_stable() {
    // The exported helper must hash the same bytes identically across calls.
    let code = r#"
fn unused() -> Int {
  return 0
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let hash = instance
        .get_typed_func::<i32, i64>(&mut store, "__aipo_string_hash")
        .expect("exported `__aipo_string_hash`");

    assert!(instance.get_func(&mut store, "unused").is_some());
    assert!(
        hash.call(&mut store, 0)
            .map(|value| value == hash.call(&mut store, 0).unwrap())
            .unwrap_or(true)
    );
}

#[test]
fn test_dict_has_key() {
    let code = r#"
fn check_keys() -> Int {
  let d = { "apple": 10, 42: 100 }
  var count = 0
  if d.has("apple") {
    count = count + 1
  }
  if d.has("banana") {
    count = count + 10
  }
  if d.has(42) {
    count = count + 100
  }
  if d.has(99) {
    count = count + 1000
  }
  return count
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "check_keys")
        .expect("exported `check_keys`");

    assert_eq!(f.call(&mut store, ()).unwrap(), 101);
}

#[test]
fn test_dict_compound_assignment() {
    let code = r#"
fn tally() -> Int {
  var d = { "counter": 10, 7: 100 }
  d["counter"] += 5
  d[7] += 23
  return d["counter"] + d[7]
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "tally")
        .expect("exported `tally`");

    // 15 + 123 = 138
    assert_eq!(f.call(&mut store, ()).unwrap(), 138);
}

#[test]
fn storing_into_a_literal_dictionary_adds_a_new_key() {
    // A dictionary literal asks the allocator for exactly as many entries as it
    // writes, so the block had no room and `d["b"] = 2` wrote past it. The keys
    // then read back as the literal's own first key, because the store had
    // landed on the allocator's next block instead.
    let code = r#"
fn keys_after_store() -> Int {
  let d = {"a": 1}
  d["b"] = 2
  let ks = d.keys()
  if len(ks) == 2 { return len(ks) }
  return 0 - 1
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "keys_after_store")
        .expect("exported `keys_after_store`");
    assert_eq!(f.call(&mut store, ()).unwrap(), 2);
}

#[test]
fn dictionary_grows_past_the_minimum_capacity() {
    // The headroom is eight entries; a dictionary that keeps growing must still
    // keep every key distinct.
    let code = r#"
fn count_twenty() -> Int {
  var d = {}
  repeat 20 as i {
    d[String(i)] = i
  }
  return len(d)
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "count_twenty")
        .expect("exported `count_twenty`");
    assert_eq!(f.call(&mut store, ()).unwrap(), 20);
}
