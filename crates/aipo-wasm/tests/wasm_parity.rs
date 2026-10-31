#![cfg(feature = "wasmtime")]

//! Differential regression suite for the Wasm backend against the canonical
//! conformance corpus.
//!
//! Every fixture listed in `PASSING` is compiled to Wasm, executed on Wasmtime,
//! and diffed against the committed `.stdout`. The list is the set of fixtures
//! the backend currently reproduces byte-for-byte; a fixture only enters it
//! together with the fix that makes it pass, so this file also records the
//! backend's claimed coverage.

use std::path::{Path, PathBuf};

use aipo_hir::lower;
use aipo_source::{Source, SourceId};
use aipo_syntax::parse;
use aipo_wasm::{WasmExecutionOptions, compile_hir, execute_wasm_with_options};
use wasmtime::{Engine, Instance, Module, Store};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn corpus_dir() -> PathBuf {
    workspace_root().join("docs/conformance/programs")
}

fn compile(source_code: &str) -> Vec<u8> {
    let source = Source::new(SourceId::next(), "fixture.aipo", source_code);
    let (ast, diagnostics) = parse(&source);
    assert!(
        diagnostics.is_empty(),
        "parse diagnostics: {diagnostics:?} for:\n{source_code}"
    );
    let hir = lower(ast);
    compile_hir(&hir).expect("compilation to Wasm succeeds")
}

fn run(source_code: &str) -> String {
    let bytes = compile(source_code);
    let mut out: Vec<u8> = Vec::new();
    execute_wasm_with_options(&bytes, &mut out, WasmExecutionOptions::default())
        .unwrap_or_else(|error| panic!("execution failed: {error}\nfor:\n{source_code}"));
    String::from_utf8(out).expect("stdout is UTF-8")
}

fn run_fixture(name: &str) -> Result<String, String> {
    let path = corpus_dir().join(format!("{name}.aipo"));
    let source_code = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
    let source = Source::new(
        SourceId::next(),
        path.to_string_lossy().to_string(),
        &source_code,
    );
    let (ast, diagnostics) = parse(&source);
    assert!(
        diagnostics.is_empty(),
        "parse diagnostics for {name}: {diagnostics:?}"
    );
    let hir = lower(ast);
    let bytes = compile_hir(&hir).map_err(|error| error.to_string())?;
    let mut out: Vec<u8> = Vec::new();
    execute_wasm_with_options(&bytes, &mut out, WasmExecutionOptions::default())
        .map_err(|error| error.to_string())?;
    Ok(String::from_utf8(out).expect("stdout is UTF-8"))
}

/// Fixtures the Wasm backend reproduces byte-for-byte.
const PASSING: &[&str] = &[
    "01_hello",
    "09_slicing",
    "02_recursion",
    "03_control_flow",
    "05_structs_and_impl",
    "12_bytes",
    "16_interface_contracts",
    "22_signed_zero",
    "23_init_in_parameterized_fn",
    "27_numeric_literal_bases",
    "30_struct_update_with",
    "31_match_destructure_and_guards",
    "32_enum_variants_and_matching",
    "34_conditional_values",
    "36_semantic_type_tests",
];

#[test]
fn corpus_fixtures_match_committed_stdout() {
    let mut failures = Vec::new();
    for name in PASSING {
        let expected_path = corpus_dir().join(format!("{name}.stdout"));
        let expected = std::fs::read_to_string(&expected_path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", expected_path.display()));
        match run_fixture(name) {
            Ok(actual) if actual == expected => {}
            Ok(actual) => failures.push(format!(
                "{name}: stdout differs\n--- expected ---\n{expected}--- actual ---\n{actual}"
            )),
            Err(error) => failures.push(format!("{name}: {error}")),
        }
    }
    assert!(
        failures.is_empty(),
        "Wasm conformance failures:\n\n{}",
        failures.join("\n\n")
    );
}

#[test]
fn emitted_modules_always_validate() {
    // A module that fails Wasm validation is a backend bug, not a program bug:
    // the emitter produced malformed code. These are the shapes that used to.
    let snippets = [
        "fn f() {\n  return fail(\"boom\")\n}\nio.println(1)",
        "fn parse(text) {\n  let v = Int(text) or_else -1\n  if v < 0 {\n    return fail(\"no\")\n  }\n  return v\n}\nio.println(parse(\"42\"))",
        "io.println(math.pow(2, 10))",
        "io.println(math.round(2.5))",
        "let s = \"ready\"\nmatch s {\nwhen \"ready\"\n  io.println(\"ready\")\nelse\n  io.println(\"unknown\")\n}",
        "struct P {\n  var x\n  var y\n}\nlet a = P{x = 1, y = 2}\nlet b = P{x = 1, y = 2}\nio.println(String(a == b))",
    ];
    for snippet in snippets {
        let bytes = compile(snippet);
        let engine = Engine::default();
        Module::new(&engine, &bytes)
            .unwrap_or_else(|error| panic!("invalid module for:\n{snippet}\n{error:?}"));
    }
}

#[test]
fn math_helpers_match_canonical_semantics() {
    assert_eq!(run("io.println(math.pow(2, 10))"), "1024\n");
    assert_eq!(run("io.println(math.pow(3, 4))"), "81\n");
    assert_eq!(run("io.println(math.pow(2, 0))"), "1\n");
    assert_eq!(run("io.println(math.pow(-2, 3))"), "-8\n");
    // `math.round` answers an Int in canon, matching the VM.
    assert_eq!(run("io.println(math.round(2.5))"), "3\n");
    assert_eq!(run("io.println(math.round(-2.5))"), "-3\n");
    assert_eq!(run("io.println(math.sqrt(9.0))"), "3.0\n");
    assert_eq!(run("io.println(math.clamp(11, 0, 10))"), "10\n");
}

#[test]
fn each_binding_order_follows_canon() {
    // One name binds the natural element.
    assert_eq!(
        run(
            "let xs = [3, 7, 11]\nvar acc = 0\neach x in xs {\n  acc = acc + x\n}\nio.println(acc)"
        ),
        "21\n"
    );
    // Two names bind the key/index first, then the value.
    assert_eq!(
        run("let items = [\"a\", \"b\"]\neach i, v in items {\n  io.println(f\"{i}:{v}\")\n}"),
        "0:a\n1:b\n"
    );
}

#[test]
fn string_arguments_cross_function_boundaries_intact() {
    assert_eq!(
        run("fn greet(name) {\n  return f\"Hello, {name}!\"\n}\nio.println(greet(\"world\"))"),
        "Hello, world!\n"
    );
}

#[test]
fn struct_string_fields_print_as_text() {
    assert_eq!(
        run(
            "struct User {\n  name\n  age\n}\nlet ana = User{name = \"ana\", age = 30}\nio.println(ana.name)\nio.println(ana.age)"
        ),
        "ana\n30\n"
    );
}

#[test]
fn untyped_functions_return_through_match() {
    assert_eq!(
        run(
            "fn describe(n) {\n  match n {\n  when 0\n    return \"zero\"\n  else\n    return \"other\"\n  }\n}\nio.println(describe(0))"
        ),
        "zero\n"
    );
}

#[test]
fn omitted_parameters_take_their_declared_default() {
    assert_eq!(
        run(
            "fn describe(prefix, count = 1) {\n  return prefix + \" x\" + String(count)\n}\nio.println(describe(\"item\"))\nio.println(describe(\"item\", 3))"
        ),
        "item x1\nitem x3\n"
    );
}

#[test]
fn none_renders_as_its_literal_word() {
    assert_eq!(run("io.println(none)"), "none\n");
}

#[test]
fn bytes_slice_yields_an_independent_block() {
    // `bytes[a..b]` produces a fresh managed block: the length is the window's
    // own, and it does not alias the source.
    assert_eq!(
        run(
            "let data = Bytes(4)\nlet window = data[1..3]\nio.println(len(window))\nio.println(len(data))"
        ),
        "2\n4\n"
    );
}

#[test]
fn list_slice_yields_a_new_list_with_clamped_bounds() {
    // A range index over a `List` copies the window; an out-of-range end clamps
    // to the source length instead of trapping.
    assert_eq!(
        run("let xs = [10, 20, 30, 40]\nlet w = xs[1..3]\nio.println(len(w))\nio.println(w[0])"),
        "2\n20\n"
    );
    assert_eq!(
        run("let xs = [1, 2, 3]\nlet w = xs[1..99]\nio.println(len(w))"),
        "2\n"
    );
}

#[test]
fn string_index_and_slice_are_code_point_based() {
    assert_eq!(
        run("let w = \"abcdef\"\nio.println(w[0])\nio.println(w[1..4])"),
        "a\nbcd\n"
    );
    // Multi-byte characters must not be cut in half by a byte-offset slice.
    // `len` counts code points, matching the canonical `string.len`.
    assert_eq!(
        run("let w = \"héllo\"\nio.println(w[1])\nio.println(len(w))"),
        "é\n5\n"
    );
}

#[test]
fn list_element_read_keeps_the_element_kind() {
    // `letters[0]` holds a `String`, so it prints as text rather than as the
    // pointer it occupies.
    assert_eq!(
        run("let letters = [\"a\", \"b\"]\nio.println(letters[0])"),
        "a\n"
    );
}

#[test]
fn collections_render_with_canonical_shape() {
    // Lists print as `[a, b, c]`, with elements rendered by their own kind.
    assert_eq!(run("io.println([1, 2, 3])"), "[1, 2, 3]\n");
    assert_eq!(run("io.println([])"), "[]\n");
    assert_eq!(
        run("let letters = [\"a\", \"b\"]\nio.println(letters)"),
        "[a, b]\n"
    );
}

#[test]
fn omitted_slice_bounds_reach_the_end() {
    // An omitted bound is spelled as the largest safe integer upstream, which does
    // not fit `i32`; the lowering saturates instead of wrapping negative.
    assert_eq!(
        run("let w = \"abcdef\"\nio.println(w[3..])\nio.println(w[..2])"),
        "def\nab\n"
    );
    assert_eq!(
        run("let xs = [1, 2, 3]\nio.println(xs[1..])\nio.println(xs[..2])"),
        "[2, 3]\n[1, 2]\n"
    );
}

#[test]
fn string_match_compares_values_not_addresses() {
    // Two equal strings built at runtime live at different offsets, so pattern
    // matching cannot compare pointers.
    assert_eq!(
        run(
            "fn classify(s) {\n  match s {\n  when \"ok\"\n    return 1\n  else\n    return 0\n  }\n}\nlet built = \"o\" + \"k\"\nio.println(classify(built))"
        ),
        "1\n"
    );
}

#[test]
fn module_validates_and_instantiates() {
    // A program with no host I/O imports nothing, so it instantiates against an
    // empty linker. Anything that links `aipo_host` is covered by the runner tests.
    let bytes = compile("fn add(a, b) {\n  return a + b\n}\nlet total = add(1, 2)");
    let engine = Engine::default();
    let module = Module::new(&engine, &bytes).expect("valid module");
    let mut store = Store::new(&engine, ());
    Instance::new(&mut store, &module, &[]).expect("instantiation without host imports");
}
