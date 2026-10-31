//! Direct tests for the emitted Unicode helpers.
//!
//! Each helper is a Wasm function that reads a `String` out of linear memory, so
//! these tests build a real module, place a string with `__aipo_alloc`, and call
//! the helper through its export. That is the only way to see the code points the
//! helpers actually produce, independent of the surface method that calls them.

use aipo_hir::lower;
use aipo_source::{Source, SourceId};
use aipo_syntax::parse;
use aipo_wasm::compile_hir;
use wasmtime::{Engine, Instance, Memory, Module, Store};

/// Builds a module with no program in it, so the helpers are the only exports
/// under test.
fn instantiate() -> (Store<()>, Instance) {
    let source = Source::new(
        SourceId::next(),
        "test.aipo",
        "fn noop() -> Int { return 0 }",
    );
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

fn memory(store: &mut Store<()>, instance: &Instance) -> Memory {
    instance
        .get_memory(&mut *store, "memory")
        .expect("exported memory")
}

/// Places `text` in linear memory as an Aipo `String` and returns its pointer.
fn put_string(store: &mut Store<()>, instance: &Instance, text: &str) -> i32 {
    let alloc = instance
        .get_typed_func::<i32, i32>(&mut *store, "__aipo_alloc")
        .expect("exported allocator");
    let bytes = text.as_bytes();
    let ptr = alloc
        .call(&mut *store, bytes.len() as i32 + 4)
        .expect("allocation succeeds");
    let mem = memory(store, instance);
    mem.write(
        &mut *store,
        ptr as usize,
        &(bytes.len() as u32).to_le_bytes(),
    )
    .expect("write length");
    mem.write(&mut *store, ptr as usize + 4, bytes)
        .expect("write bytes");
    ptr
}

/// Reads an Aipo `String` back out of linear memory.
fn get_string(store: &mut Store<()>, instance: &Instance, ptr: i32) -> String {
    let mem = memory(store, instance);
    let mut len = [0u8; 4];
    mem.read(&*store, ptr as usize, &mut len)
        .expect("read length");
    let len = u32::from_le_bytes(len) as usize;
    let mut bytes = vec![0u8; len];
    mem.read(&*store, ptr as usize + 4, &mut bytes)
        .expect("read bytes");
    String::from_utf8(bytes).expect("helper output is valid UTF-8")
}

fn call1(store: &mut Store<()>, instance: &Instance, name: &str, arg: i32) -> i32 {
    instance
        .get_typed_func::<i32, i32>(&mut *store, name)
        .unwrap_or_else(|error| panic!("exported `{name}`: {error}"))
        .call(&mut *store, arg)
        .unwrap_or_else(|error| panic!("`{name}` traps: {error}"))
}

fn call2(store: &mut Store<()>, instance: &Instance, name: &str, a: i32, b: i32) -> i32 {
    instance
        .get_typed_func::<(i32, i32), i32>(&mut *store, name)
        .unwrap_or_else(|error| panic!("exported `{name}`: {error}"))
        .call(&mut *store, (a, b))
        .unwrap_or_else(|error| panic!("`{name}` traps: {error}"))
}

fn nfc(store: &mut Store<()>, instance: &Instance, text: &str) -> String {
    let ptr = put_string(store, instance, text);
    let out = call1(store, instance, "__aipo_nfc", ptr);
    get_string(store, instance, out)
}

fn case(store: &mut Store<()>, instance: &Instance, text: &str, upper: bool) -> String {
    let ptr = put_string(store, instance, text);
    let out = call2(store, instance, "__aipo_str_case", ptr, i32::from(upper));
    get_string(store, instance, out)
}

fn trim(store: &mut Store<()>, instance: &Instance, text: &str) -> String {
    let ptr = put_string(store, instance, text);
    let out = call1(store, instance, "__aipo_str_trim", ptr);
    get_string(store, instance, out)
}

#[test]
fn nfc_composes_base_and_mark() {
    let (mut store, instance) = instantiate();
    assert_eq!(nfc(&mut store, &instance, "e\u{0301}"), "\u{00e9}");
}

#[test]
fn nfc_leaves_nfc_text_alone() {
    let (mut store, instance) = instantiate();
    assert_eq!(nfc(&mut store, &instance, "\u{00e9}"), "\u{00e9}");
    assert_eq!(nfc(&mut store, &instance, "ascii"), "ascii");
    assert_eq!(
        nfc(&mut store, &instance, "\u{65e5}\u{672c}\u{8a9e}"),
        "\u{65e5}\u{672c}\u{8a9e}"
    );
}

#[test]
fn nfc_handles_singleton_decomposition() {
    let (mut store, instance) = instantiate();
    // ANGSTROM SIGN canonical-decomposes to A with a ring.
    assert_eq!(nfc(&mut store, &instance, "\u{212b}"), "\u{00c5}");
}

#[test]
fn nfc_reorders_marks_by_combining_class() {
    let (mut store, instance) = instantiate();
    // `a` + acute (230) + dot below (220) orders dot below first, then composes.
    // `a` + acute (230) + dot below (220) reorders the marks, then composes
    // `a` with the dot below into U+1EA1.
    assert_eq!(
        nfc(&mut store, &instance, "a\u{0301}\u{0323}"),
        "\u{1ea1}\u{0301}"
    );
}

#[test]
fn nfc_composes_hangul() {
    let (mut store, instance) = instantiate();
    assert_eq!(
        nfc(&mut store, &instance, "\u{1100}\u{1161}\u{11a8}"),
        "\u{ac01}"
    );
    assert_eq!(nfc(&mut store, &instance, "\u{ac00}"), "\u{ac00}");
}

#[test]
fn upper_and_lower_cover_non_latin() {
    let (mut store, instance) = instantiate();
    assert_eq!(
        case(&mut store, &instance, "stra\u{00df}e", true),
        "STRASSE"
    );
    assert_eq!(case(&mut store, &instance, "\u{0456}", true), "\u{0406}");
    assert_eq!(case(&mut store, &instance, "\u{03c3}", true), "\u{03a3}");
    assert_eq!(
        case(&mut store, &instance, "\u{65e5}\u{672c}\u{8a9e}", true),
        "\u{65e5}\u{672c}\u{8a9e}"
    );
}

#[test]
fn lower_of_dotted_capital_i_keeps_the_mark() {
    let (mut store, instance) = instantiate();
    assert_eq!(case(&mut store, &instance, "\u{0130}", false), "i\u{0307}");
}

#[test]
fn trim_uses_unicode_whitespace() {
    let (mut store, instance) = instantiate();
    assert_eq!(trim(&mut store, &instance, "\u{00a0} x \u{3000}"), "x");
    assert_eq!(trim(&mut store, &instance, "   "), "");
    assert_eq!(trim(&mut store, &instance, "plain"), "plain");
}
