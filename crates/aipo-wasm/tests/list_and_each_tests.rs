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

#[test]
fn test_global_len_builtin_for_list_string_and_dict() {
    let code = r#"
fn sizes() -> Int {
  let xs = [1, 2, 3]
  let text = "hello"
  let d = {"a": 1, "b": 2}
  return len(xs) + len(text) + len(d)
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "sizes")
        .expect("exported `sizes`");

    assert_eq!(f.call(&mut store, ()).unwrap(), 10);
}

#[test]
fn test_list_first_last_contains_and_is_empty() {
    let code = r#"
fn first_value() -> Int {
  let xs = [10, 20, 30]
  return xs.first()
}

fn last_value() -> Int {
  let xs = [10, 20, 30]
  return xs.last()
}

fn has_20() -> Bool {
  let xs = [10, 20, 30]
  return xs.contains(20)
}

fn has_99() -> Bool {
  let xs = [10, 20, 30]
  return xs.contains(99)
}

fn empty() -> Bool {
  let xs = []
  return xs.is_empty()
}

fn non_empty() -> Bool {
  let xs = [1]
  return xs.is_empty()
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let first = instance
        .get_typed_func::<(), i64>(&mut store, "first_value")
        .expect("first");
    let last = instance
        .get_typed_func::<(), i64>(&mut store, "last_value")
        .expect("last");
    let has_20 = instance
        .get_typed_func::<(), i32>(&mut store, "has_20")
        .expect("has_20");
    let has_99 = instance
        .get_typed_func::<(), i32>(&mut store, "has_99")
        .expect("has_99");
    let empty = instance
        .get_typed_func::<(), i32>(&mut store, "empty")
        .expect("empty");
    let non_empty = instance
        .get_typed_func::<(), i32>(&mut store, "non_empty")
        .expect("non_empty");

    assert_eq!(first.call(&mut store, ()).unwrap(), 10);
    assert_eq!(last.call(&mut store, ()).unwrap(), 30);
    assert_eq!(has_20.call(&mut store, ()).unwrap(), 1);
    assert_eq!(has_99.call(&mut store, ()).unwrap(), 0);
    assert_eq!(empty.call(&mut store, ()).unwrap(), 1);
    assert_eq!(non_empty.call(&mut store, ()).unwrap(), 0);
}

#[test]
fn test_list_mutation_methods() {
    let code = r#"
fn insert_then_get() -> Int {
  let xs = [1, 2, 3]
  xs.insert(1, 9)
  return xs[1]
}

fn remove_last_len() -> Int {
  let xs = [1, 2, 3]
  xs.remove_last()
  return len(xs)
}

fn remove_at_get() -> Int {
  let xs = [1, 2, 3]
  xs.remove_at(1)
  return xs[1]
}

fn remove_value() -> Bool {
  let xs = [1, 2, 3]
  xs.remove(2)
  return xs.contains(2)
}

fn clear_len() -> Int {
  let xs = [1, 2, 3]
  xs.clear()
  return len(xs)
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let insert = instance
        .get_typed_func::<(), i64>(&mut store, "insert_then_get")
        .expect("insert");
    let remove_last = instance
        .get_typed_func::<(), i64>(&mut store, "remove_last_len")
        .expect("remove_last");
    let remove_at = instance
        .get_typed_func::<(), i64>(&mut store, "remove_at_get")
        .expect("remove_at");
    let remove_value = instance
        .get_typed_func::<(), i32>(&mut store, "remove_value")
        .expect("remove");
    let clear = instance
        .get_typed_func::<(), i64>(&mut store, "clear_len")
        .expect("clear");

    assert_eq!(insert.call(&mut store, ()).unwrap(), 9);
    assert_eq!(remove_last.call(&mut store, ()).unwrap(), 2);
    assert_eq!(remove_at.call(&mut store, ()).unwrap(), 3);
    assert_eq!(remove_value.call(&mut store, ()).unwrap(), 0);
    assert_eq!(clear.call(&mut store, ()).unwrap(), 0);
}

#[test]
fn test_list_count_find_and_reverse() {
    let code = r#"
fn count_three() -> Int {
  let xs = [3, 1, 3, 2]
  return xs.count(3)
}

fn find_two() -> Int {
  let xs = [3, 1, 2]
  return xs.find(2)
}

fn find_missing() -> Int {
  let xs = [3, 1, 2]
  return xs.find(9)
}

fn reverse_first() -> Int {
  let xs = [1, 2, 3]
  # `reverse` answers a new list; the receiver keeps its order.
  return xs.reverse().first()
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let count = instance
        .get_typed_func::<(), i64>(&mut store, "count_three")
        .expect("count");
    let find = instance
        .get_typed_func::<(), i64>(&mut store, "find_two")
        .expect("find");
    let missing = instance
        .get_typed_func::<(), i64>(&mut store, "find_missing")
        .expect("missing");
    let reverse = instance
        .get_typed_func::<(), i64>(&mut store, "reverse_first")
        .expect("reverse");

    assert_eq!(count.call(&mut store, ()).unwrap(), 2);
    assert_eq!(find.call(&mut store, ()).unwrap(), 2);
    assert_eq!(missing.call(&mut store, ()).unwrap(), -1);
    assert_eq!(reverse.call(&mut store, ()).unwrap(), 3);
}

#[test]
fn test_list_transform_and_filter_callbacks() {
    let code = r#"
fn transformed_len() -> Int {
  let ys = [1, 2, 3].transform(fn (x) { return x * 2 })
  return ys.len()
}

fn transformed_edges() -> Int {
  let ys = [1, 2, 3].transform(fn (x) { return x * 2 })
  return ys.first() + ys.last()
}

fn filtered_len() -> Int {
  let ys = [1, 2, 3].filter(fn (x) { return x > 1 })
  return ys.len()
}

fn filtered_first() -> Int {
  let ys = [1, 2, 3].filter(fn (x) { return x > 1 })
  return ys.first()
}

fn mapped_alias_len() -> Int {
  let ys = [1, 2, 3].map(fn (x) { return x + 10 })
  return ys.len()
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let transformed_len = instance
        .get_typed_func::<(), i64>(&mut store, "transformed_len")
        .expect("transformed_len");
    let transformed_edges = instance
        .get_typed_func::<(), i64>(&mut store, "transformed_edges")
        .expect("transformed_edges");
    let filtered_len = instance
        .get_typed_func::<(), i64>(&mut store, "filtered_len")
        .expect("filtered_len");
    let filtered_first = instance
        .get_typed_func::<(), i64>(&mut store, "filtered_first")
        .expect("filtered_first");
    let mapped_alias_len = instance
        .get_typed_func::<(), i64>(&mut store, "mapped_alias_len")
        .expect("mapped_alias_len");

    assert_eq!(transformed_len.call(&mut store, ()).unwrap(), 3);
    assert_eq!(transformed_edges.call(&mut store, ()).unwrap(), 8);
    assert_eq!(filtered_len.call(&mut store, ()).unwrap(), 2);
    assert_eq!(filtered_first.call(&mut store, ()).unwrap(), 2);
    assert_eq!(mapped_alias_len.call(&mut store, ()).unwrap(), 3);
}

#[test]
fn test_list_sort_and_sort_by() {
    let code = r#"
fn test_sort_first() -> Int {
  let xs = [5, 2, 8, 1, 9]
  let sorted = xs.sort()
  return sorted.first()
}

fn test_sort_last() -> Int {
  let xs = [5, 2, 8, 1, 9]
  let sorted = xs.sort()
  return sorted.last()
}

fn test_sort_by_descending() -> Int {
  let xs = [5, 2, 8, 1, 9]
  let sorted = xs.sort_by(fn (x) { return 0 - x })
  return sorted.first()
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let test_sort_first = instance
        .get_typed_func::<(), i64>(&mut store, "test_sort_first")
        .expect("test_sort_first");
    let test_sort_last = instance
        .get_typed_func::<(), i64>(&mut store, "test_sort_last")
        .expect("test_sort_last");
    let test_sort_by_descending = instance
        .get_typed_func::<(), i64>(&mut store, "test_sort_by_descending")
        .expect("test_sort_by_descending");

    assert_eq!(test_sort_first.call(&mut store, ()).unwrap(), 1);
    assert_eq!(test_sort_last.call(&mut store, ()).unwrap(), 9);
    assert_eq!(test_sort_by_descending.call(&mut store, ()).unwrap(), 9);
}

#[test]
fn test_each_over_string_binds_a_character_string() {
    // A `String` iterates code points, and each binding is a one-character `String`.
    // When the binding was classified as `Int`, `len(ch)` read the pointer and
    // `io.println(ch)` printed the address instead of the character.
    let code = r#"
fn mark_score() -> Int {
  let word = "aéb"
  var marks = 0
  var width = 0
  each ch in word {
    if ch == "é" { marks = marks + 1 }
    width = width + len(ch)
  }
  return marks * 10 + width
}
"#;
    let (mut store, instance) = instantiate_aipo(code);
    let f = instance
        .get_typed_func::<(), i64>(&mut store, "mark_score")
        .expect("exported `mark_score`");

    assert_eq!(f.call(&mut store, ()).unwrap(), 13);
}
