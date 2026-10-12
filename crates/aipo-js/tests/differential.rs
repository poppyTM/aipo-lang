//! VM↔JS differential suite for the Wave 2 MVP parity slice.
//!
//! For every `docs/conformance/programs/NN_*.aipo` fixture the harness runs the
//! program twice — once on the Rust VM, once on `node` over the emitted JS
//! bundle — and requires VM stdout == JS stdout == committed `.stdout`.
//!
//! Run with:
//!
//! ```bash
//! cargo test -p aipo-js --test differential
//! ```

#![forbid(unsafe_code)]

use aipo_source::{Source, SourceId};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};

static IO_LOCK: Mutex<()> = Mutex::new(());

fn lock_io() -> MutexGuard<'static, ()> {
    IO_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// The deterministic clock profile this suite runs both backends under.
///
/// `time.now`/`time.monotonic` are host capabilities, so a parity comparison is only
/// meaningful when the two backends read the *same* source: the VM installs
/// [`DeterministicClock`] and the JavaScript side gets its twin as a preload module (see
/// [`write_clock_profile`]). Wall time is a fixed instant and the monotonic origin is zero, so
/// a fixture can assert the clock contracts and still produce a stable, comparable result.
const PROFILE_WALL_SECONDS: f64 = 1_000_000.0;
const PROFILE_MONOTONIC_SECONDS: f64 = 0.0;

/// The VM-side half of the deterministic clock profile.
struct DeterministicClock;

impl aipo_stdlib::time::ClockSource for DeterministicClock {
    fn wall_seconds(&self) -> f64 {
        PROFILE_WALL_SECONDS
    }

    fn monotonic_seconds(&self) -> f64 {
        PROFILE_MONOTONIC_SECONDS
    }
}

/// Writes the JavaScript half of the deterministic clock profile, returning the preload file
/// name to hand to `node --import`.
///
/// The emitted entry installs the system clock only when no source is present, so loading this
/// module first is what makes a replay replay. That is the same mechanism a host uses, not a
/// hook the runtime reserves for tests.
fn write_clock_profile(dir: &Path) -> &'static str {
    let source = format!(
        "// Deterministic clock profile for the VM/JS differential suite.\n\
         globalThis.__aipoClock = {{\n\
         \x20 wallSeconds: () => {PROFILE_WALL_SECONDS},\n\
         \x20 monotonicSeconds: () => {PROFILE_MONOTONIC_SECONDS},\n\
         }};\n"
    );
    std::fs::write(dir.join("clock-profile.mjs"), source).expect("write clock profile");
    "clock-profile.mjs"
}

fn conformance_dir() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.pop();
    dir.pop();
    dir.push("docs");
    dir.push("conformance");
    dir
}

fn program_cases() -> Vec<(PathBuf, PathBuf)> {
    let dir = conformance_dir().join("programs");
    let mut cases = Vec::new();
    let entries = std::fs::read_dir(&dir).expect("conformance programs dir");
    for entry in entries {
        let path = entry.expect("dir entry").path();
        if path.extension().and_then(|e| e.to_str()) == Some("aipo") {
            let stdout = path.with_extension("stdout");
            if stdout.exists() {
                cases.push((path, stdout));
            }
        }
    }
    cases.sort();
    cases
}

fn prelude_surface() -> aipo_sema::PreludeSurface {
    let mut vm = aipo_vm::Vm::new();
    let mut registry = aipo_runtime::NativeRegistry::new();
    aipo_stdlib::register_stdlib(&mut vm, &mut registry);
    let mut surface = aipo_sema::PreludeSurface::fundamental();
    for name in vm.globals.keys() {
        match registry.get(None, name) {
            Some(meta) => surface.add_function(name, meta.arity, meta.arity),
            None => surface.add_variable(name),
        }
    }
    surface
}

struct Compiled {
    ir: aipo_ir::CoreModule,
    bytecode: aipo_bytecode::BytecodeModule,
}

fn compile(path: &Path) -> Compiled {
    let text = std::fs::read_to_string(path).expect("read fixture");
    let source = Source::new(SourceId::next(), path.display().to_string(), &text);
    let (program, mut diagnostics) = aipo_syntax::parse(&source);
    assert!(
        diagnostics
            .iter()
            .all(|d| d.severity != aipo_diagnostics::Severity::Error),
        "parse errors in {}: {diagnostics:?}",
        path.display()
    );
    let hir = aipo_hir::lower(program);
    let surface = prelude_surface();
    let (_, sema_diagnostics) = aipo_sema::check_with_prelude(&source, &hir, &surface);
    diagnostics.extend(sema_diagnostics);
    assert!(
        diagnostics
            .iter()
            .all(|d| d.severity != aipo_diagnostics::Severity::Error),
        "sema errors in {}: {diagnostics:?}",
        path.display()
    );
    let ir = aipo_ir::lower_to_ir(&hir);
    let bytecode = aipo_bytecode::compile(&ir).expect("bytecode verifies");
    Compiled { ir, bytecode }
}

fn run_vm(compiled: &Compiled) -> String {
    let _guard = lock_io();
    struct BufWriter(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
    impl Write for BufWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let buffer = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    aipo_stdlib::io::set_output_sink(Some(Box::new(BufWriter(buffer.clone()))));
    let (mut vm, _) = {
        let mut vm = aipo_vm::Vm::new();
        let mut registry = aipo_runtime::NativeRegistry::new();
        aipo_stdlib::register_stdlib(&mut vm, &mut registry);
        (vm, registry)
    };
    // The deterministic profile: the same clock the preload installs on the JavaScript side.
    aipo_stdlib::time::install_clock(Box::new(DeterministicClock));
    for decl in &compiled.bytecode.structs {
        let fields: Vec<(&str, bool)> = decl
            .fields
            .iter()
            .map(|(name, fixed)| (name.as_str(), *fixed))
            .collect();
        vm.register_struct(decl.name.clone(), fields);
    }
    for function in &compiled.bytecode.functions {
        if let Some((type_name, method)) = function.name.split_once('.') {
            vm.register_struct_method(
                type_name,
                method,
                function.entry_ip,
                function.params,
                function.is_async,
            );
        }
    }
    vm.run(&compiled.bytecode).expect("vm runs fixture");
    aipo_stdlib::io::set_output_sink(None);
    aipo_stdlib::time::revoke_clock();
    drop(_guard);
    let bytes = buffer.lock().unwrap_or_else(|e| e.into_inner()).clone();
    String::from_utf8(bytes).expect("vm output is utf-8")
}

fn run_js(path: &Path, compiled: &Compiled) -> String {
    let bundle = aipo_js::emit_js(
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("main.aipo"),
        &std::fs::read_to_string(path).expect("read fixture"),
        &compiled.ir,
    );
    let dir = std::env::temp_dir().join(format!(
        "aipo-js-diff-{}",
        path.file_stem().and_then(|n| n.to_str()).unwrap_or("prog")
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(dir.join("app.js"), &bundle.app_js).expect("write app.js");
    std::fs::write(dir.join("aipo-runtime.js"), &bundle.runtime_js).expect("write shim");
    std::fs::write(dir.join("app.js.map"), &bundle.source_map).expect("write map");
    let profile = write_clock_profile(&dir);
    let output = Command::new("node")
        .arg("--import")
        .arg(format!("./{profile}"))
        .arg("app.js")
        .current_dir(&dir)
        .output()
        .expect("node runs (requires Node >= 20)");
    assert!(
        output.status.success(),
        "node failed for {}: status {:?} stderr: {}",
        path.display(),
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("js output is utf-8")
}

#[test]
fn test_vm_and_js_agree_with_committed_stdout() {
    let cases = program_cases();
    assert!(
        cases.len() >= 20,
        "expected 20 program fixtures, found {}",
        cases.len()
    );
    for (path, stdout_path) in cases {
        let compiled = compile(&path);
        let expected = std::fs::read_to_string(&stdout_path).expect("read snapshot");
        let vm_out = run_vm(&compiled);
        assert_eq!(
            vm_out,
            expected,
            "VM output differs from committed snapshot for {}",
            path.display()
        );
        let js_out = run_js(&path, &compiled);
        assert_eq!(
            js_out,
            expected,
            "JS output differs from committed snapshot for {}",
            path.display()
        );
    }
}

#[test]
fn test_emitted_bundle_is_esm_with_source_map() {
    let dir = conformance_dir().join("programs");
    let path = dir.join("01_hello.aipo");
    let compiled = compile(&path);
    let bundle = aipo_js::emit_js("01_hello.aipo", "io.println(1)\n", &compiled.ir);
    assert!(bundle.app_js.contains("runModule"));
    assert!(bundle.app_js.contains("sourceMappingURL=app.js.map"));
    let map: serde_json::Value =
        serde_json::from_str(&bundle.source_map).expect("source map is JSON");
    assert_eq!(map["version"], 3);
    assert!(bundle.runtime_js.contains("RUNTIME_VERSION"));
    let _ = Write::write_all(&mut std::io::sink(), b"");
}

fn compile_code(name: &str, text: &str) -> Compiled {
    let source = Source::new(SourceId::next(), name.to_string(), text);
    let (program, mut diagnostics) = aipo_syntax::parse(&source);
    assert!(
        diagnostics
            .iter()
            .all(|d| d.severity != aipo_diagnostics::Severity::Error),
        "parse errors in {name}: {diagnostics:?}"
    );
    let hir = aipo_hir::lower(program);
    let surface = prelude_surface();
    let (_, sema_diagnostics) = aipo_sema::check_with_prelude(&source, &hir, &surface);
    diagnostics.extend(sema_diagnostics);
    assert!(
        diagnostics
            .iter()
            .all(|d| d.severity != aipo_diagnostics::Severity::Error),
        "sema errors in {name}: {diagnostics:?}"
    );
    let ir = aipo_ir::lower_to_ir(&hir);
    let bytecode = aipo_bytecode::compile(&ir).expect("bytecode verifies");
    Compiled { ir, bytecode }
}

fn run_js_code(name: &str, text: &str, compiled: &Compiled) -> String {
    let bundle = aipo_js::emit_js(name, text, &compiled.ir);
    let dir = std::env::temp_dir().join(format!("aipo-js-diff-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(dir.join("app.js"), &bundle.app_js).expect("write app.js");
    std::fs::write(dir.join("aipo-runtime.js"), &bundle.runtime_js).expect("write shim");
    std::fs::write(dir.join("app.js.map"), &bundle.source_map).expect("write map");
    let profile = write_clock_profile(&dir);
    let output = Command::new("node")
        .arg("--import")
        .arg(format!("./{profile}"))
        .arg("app.js")
        .current_dir(&dir)
        .output()
        .expect("node runs (requires Node >= 20)");
    assert!(
        output.status.success(),
        "node failed for {name}: status {:?} stderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("node stdout is utf-8")
}

#[test]
fn test_wave3_differential_parity() {
    let code = r#"
var s = Set([1, 2, 2, 3])
s.add(4)
io.println(s.has(2))
s.remove(2)
io.println(s.has(2))
io.println(s.len())
io.println(s)

var b = Bytes(4)
b.write_i16(0, -42)
b.write_u16(2, 60000)
io.println(b.read_i16(0))
io.println(b.read_u16(2))
var text = "hello"
var dec = text.encode().decode()
io.println(dec)

var d1 = Duration(1.5)
var d2 = Duration(2.5)
io.println(d1 + d2)
io.println(d2 - d1)
io.println(d1 < d2)
io.println(d1.total_seconds())

var l = [1, 2, 3]
io.println(l.lazy())
"#;

    let compiled = compile_code("wave3_diff.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("wave3_diff.aipo", code, &compiled);
    assert_eq!(
        vm_out, js_out,
        "VM and JS outputs must be identical for Wave 3 features"
    );
}

#[test]
fn test_wave3_async_spawn_await_differential() {
    let code = r#"
fn worker(x) {
    return x * 2
}

var t = task.spawn(worker, [21])
var res = await t
io.println(res)
"#;
    let compiled = compile_code("wave3_async_spawn.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("wave3_async_spawn.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(vm_out, "42\n");
}

#[test]
fn test_wave3_async_all_and_race_differential() {
    let code = r#"
fn f1() {
    return 10
}
fn f2() {
    return 20
}

var t1 = task.spawn(f1, [])
var t2 = task.spawn(f2, [])
var all_res = task.all([t1, t2])
io.println(all_res)

var race_res = task.race([task.spawn(f1, []), task.spawn(f2, [])])
io.println(race_res)
"#;
    let compiled = compile_code("wave3_async_all_race.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("wave3_async_all_race.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
}

#[test]
fn test_wave3_async_group_differential() {
    let code = r#"
fn mult(val) {
    return val * 3
}

var g = task.group()
var t1 = g.spawn(mult, [4])
var t2 = g.spawn(mult, [5])
var res = g.wait()
io.println(res)
"#;
    let compiled = compile_code("wave3_async_group.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("wave3_async_group.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(vm_out, "[12, 15]\n");
}

#[test]
fn test_wave3_async_sleep_and_timeout_differential() {
    let code = r#"
fn worker() {
    task.sleep(1)
    return 99
}

var t = task.spawn(worker, [])
var res = task.timeout(t, 2)
io.println(res)

fn slow_worker() {
    task.sleep(5)
    return 100
}

var t2 = task.spawn(slow_worker, [])
var timeout_res = task.timeout(t2, 1) or_else "timed_out"
io.println(timeout_res)
"#;
    let compiled = compile_code("wave3_async_sleep_timeout.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("wave3_async_sleep_timeout.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(vm_out, "99\ntimed_out\n");
}

#[test]
fn test_wave3_async_cancel_differential() {
    let code = r#"
fn worker() {
    task.sleep(10)
    return 1
}

var t = task.spawn(worker, [])
task.cancel(t)
io.println("cancelled ok")
"#;
    let compiled = compile_code("wave3_async_cancel.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("wave3_async_cancel.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(vm_out, "cancelled ok\n");
}

#[test]
fn test_time_clock_differential_under_the_deterministic_profile() {
    // The clock is a host capability, so parity means the two backends read the same source:
    // the readings themselves must match, not merely the booleans derived from them.
    let code = r#"
io.println(time.now().total_seconds())
io.println(time.monotonic().total_seconds())
io.println(time.monotonic().total_seconds() >= 0.0)
"#;
    let compiled = compile_code("wave4_time_diff.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("wave4_time_diff.aipo", code, &compiled);
    assert_eq!(vm_out, js_out, "VM and JS must read the same clock profile");
    // Aipo prints a whole Float with its fraction (`1000000.0`), so the profile above is what
    // makes both readings exact and comparable on both backends.
    assert_eq!(
        vm_out, "1000000.0\n0.0\ntrue\n",
        "the profile decides both readings"
    );
}

#[test]
fn test_denied_clock_faults_identically_on_both_backends() {
    // A host that grants nothing must not get a faked clock on either backend, and the fault
    // must carry the canon code rather than a silent zero.
    let code = "io.println(time.monotonic().total_seconds())\n";
    let compiled = compile_code("wave4_time_denied.aipo", code);

    // The clock is process-global, so this test must exclude the ones that install a source.
    let _guard = lock_io();
    let mut vm = aipo_vm::Vm::new();
    let mut registry = aipo_runtime::NativeRegistry::new();
    aipo_stdlib::register_stdlib(&mut vm, &mut registry);
    aipo_stdlib::time::revoke_clock();
    let vm_error = vm.run(&compiled.bytecode).expect_err("denied on the VM");
    assert_eq!(
        vm_error.diagnostic_code(),
        aipo_diagnostics::DiagnosticCode::AIPO_RT_CAPABILITY_DENIED
    );

    let bundle = aipo_js::emit_js("wave4_time_denied.aipo", code, &compiled.ir);
    let dir = std::env::temp_dir().join("aipo-js-diff-wave4_time_denied");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    std::fs::write(dir.join("app.js"), &bundle.app_js).expect("write app.js");
    std::fs::write(dir.join("aipo-runtime.js"), &bundle.runtime_js).expect("write shim");
    std::fs::write(dir.join("app.js.map"), &bundle.source_map).expect("write map");
    // Deny the capability: the preload removes whatever the entry installed.
    std::fs::write(
        dir.join("deny-clock.mjs"),
        "globalThis.__aipoClock = null;\n",
    )
    .expect("write deny profile");
    let output = Command::new("node")
        .arg("--import")
        .arg("./deny-clock.mjs")
        .arg("app.js")
        .current_dir(&dir)
        .output()
        .expect("node runs (requires Node >= 20)");
    assert!(
        !output.status.success(),
        "a denied clock must fail the JS run too"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("AIPO_RT_CAPABILITY_DENIED"),
        "JS denial must report the canon code, got: {stderr}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn test_short_lambdas_differential() {
    let code = r#"
let double = x => x * 2
let add = (a, b) => a + b
let get_42 = () => 42
let sink = _ => 99

io.println(double(21))
io.println(add(10, 32))
io.println(get_42())
io.println(sink(123))
"#;
    let compiled = compile_code("short_lambdas.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("short_lambdas.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(vm_out, "42\n42\n42\n99\n");
}

#[test]
fn test_elided_comparisons_differential() {
    let code = r#"
let x = 50
let in_range = x >= 0 and <= 100
let out_of_range = x < 0 or x > 100
let typed_in_range = x is Int and >= 0 and <= 100

io.println(in_range)
io.println(out_of_range)
io.println(typed_in_range)
"#;
    let compiled = compile_code("elided_comparisons.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("elided_comparisons.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(vm_out, "true\nfalse\ntrue\n");
}

#[test]
fn test_collection_methods_differential() {
    let code = r#"
let nums = [1, 2, 3, 4, 5]
let doubled = nums.map(x => x * 2)
io.println(doubled)

let has_even = nums.any(x => x % 2 == 0)
let all_pos = nums.all(x => x > 0)
let all_even = nums.all(x => x % 2 == 0)
io.println(has_even)
io.println(all_pos)
io.println(all_even)

let nested = [[1, 2], [3], [4, 5]]
let flattened = nested.flat_map(x => x)
io.println(flattened)

let sum = nums.reduce(0, (acc, x) => acc + x)
io.println(sum)

let product = nums.reduce(1, (acc, x) => acc * x)
io.println(product)

let d = {"a": 1, "b": 2}
let entries = d.entries()
io.println(entries)

let empty = []
io.println(empty.first_or(999))
io.println(empty.last_or(999))
io.println(nums.first_or(999))
io.println(nums.last_or(999))
io.println(nums.find_index(3))
io.println(nums.find_index(99))
"#;
    let compiled = compile_code("collection_methods.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("collection_methods.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(
        vm_out,
        "[2, 4, 6, 8, 10]\ntrue\ntrue\nfalse\n[1, 2, 3, 4, 5]\n15\n120\n[[a, 1], [b, 2]]\n999\n999\n1\n5\n2\nnone\n"
    );
}

#[test]
fn test_math_extensions_differential() {
    let code = r#"
io.println(math.sin(0))
io.println(math.cos(0))
io.println(math.tan(0))
io.println(math.hypot(3, 4))
io.println(math.log2(8))
io.println(math.log10(100))
io.println(math.exp(0))

io.println(math.sign(42))
io.println(math.sign(-42))
io.println(math.sign(0))
io.println(math.sign(3.14))
io.println(math.sign(-3.14))

attempt {
    let asin_err = math.asin(2.0)
    io.println(asin_err)
} failed err {
    io.println(err.message)
}

let log_fallback = math.log(-1.0) or_else -999.0
io.println(log_fallback)
"#;
    let compiled = compile_code("math_extensions.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("math_extensions.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(
        vm_out,
        "0.0\n1.0\n0.0\n5.0\n3.0\n2.0\n1.0\n1\n-1\n0\n1.0\n-1.0\nmath.asin domain error: argument must be between -1.0 and 1.0\n-999.0\n"
    );
}

#[test]
fn test_random_deterministic_differential() {
    let code = r#"
let rng = random.create(42)

let n1 = rng.int(1, 100)
let n2 = rng.int(1, 100)
let n3 = rng.int(1, 100)
io.println(n1)
io.println(n2)
io.println(n3)

let b = rng.bool()
io.println(b)

let items = ["apple", "banana", "cherry", "date"]
let chosen = rng.choice(items)
io.println(chosen)

let nums = [1, 2, 3, 4, 5]
let shuffled = rng.shuffle(nums)
io.println(shuffled)
"#;
    let compiled = compile_code("random_deterministic.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("random_deterministic.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
}

#[test]
fn test_json_module_differential() {
    let code = r#"
let text = "{\"name\": \"Aipo\", \"ver\": 1, \"active\": true}"
let parsed = json.parse(text)
io.println(parsed["name"])
io.println(parsed["ver"])
io.println(parsed["active"])

let encoded = json.stringify(parsed)
io.println(encoded)

attempt {
    let bad = json.parse("{\"dup\": 1, \"dup\": 2}")
    io.println(bad)
} failed err {
    io.println("duplicate key caught")
}
"#;
    let compiled = compile_code("json_differential.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("json_differential.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(
        vm_out,
        "Aipo\n1\ntrue\n{\"name\": \"Aipo\", \"ver\": 1, \"active\": true}\nduplicate key caught\n"
    );
}

#[test]
fn test_encoding_module_differential() {
    let code = r#"
let orig = "Hello, Aipo 2026!"
let b64 = encoding.base64_encode(orig)
io.println(b64)

let dec_b64 = encoding.base64_decode(b64)
io.println(encoding.utf8_decode(dec_b64))

let b64url = encoding.base64url_encode(orig)
io.println(b64url)

let dec_b64url = encoding.base64url_decode(b64url)
io.println(encoding.utf8_decode(dec_b64url))

let hex = encoding.hex_encode(orig)
io.println(hex)

let dec_hex = encoding.hex_decode(hex)
io.println(encoding.utf8_decode(dec_hex))

attempt {
    let bad = encoding.hex_decode("123")
    io.println(bad)
} failed err {
    io.println("invalid hex caught")
}
"#;
    let compiled = compile_code("encoding_differential.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("encoding_differential.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(
        vm_out,
        "SGVsbG8sIEFpcG8gMjAyNiE=\nHello, Aipo 2026!\nSGVsbG8sIEFpcG8gMjAyNiE\nHello, Aipo 2026!\n48656c6c6f2c204169706f203230323621\nHello, Aipo 2026!\ninvalid hex caught\n"
    );
}

#[test]
fn test_path_module_differential() {
    let code = r#"
let p = path.join("config", "app.json")
io.println(p)

let norm = path.normalize("src/utils/../main.aipo")
io.println(norm)

let is_abs1 = path.is_absolute("/home/user")
let is_abs2 = path.is_absolute("relative/path")
io.println(is_abs1)
io.println(is_abs2)

let full = "/var/log/system.log"
io.println(path.basename(full))
io.println(path.basename(full, ".log"))
io.println(path.dirname(full))
io.println(path.ext(full))
"#;
    let compiled = compile_code("path_differential.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("path_differential.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(
        vm_out,
        "config/app.json\nsrc/main.aipo\ntrue\nfalse\nsystem.log\nsystem\n/var/log\n.log\n"
    );
}

#[test]
fn test_string_human_facing_differential() {
    let code = r#"
let s = "café"
io.println(s.graphemes())

let sentence = "Hello, world! 2026"
io.println(sentence.words())

let multiline = "line1\nline2\nline3"
io.println(multiline.lines())

let upper = "AIPO CAFÉ"
io.println(upper.casefold())

let encoded = "UTF-8 text".encode_utf8()
let decoded = encoded.decode_utf8()
io.println(decoded)
"#;
    let compiled = compile_code("string_human_facing.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("string_human_facing.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(
        vm_out,
        "[c, a, f, é]\n[Hello, world, 2026]\n[line1, line2, line3]\naipo café\nUTF-8 text\n"
    );
}

#[test]
fn test_url_module_differential() {
    let code = r#"
let endpoint = url.parse("https://api.example.com/v1/users?page=1#details")
io.println(endpoint.protocol)
io.println(endpoint.hostname)
io.println(endpoint.pathname)
io.println(endpoint.search)
io.println(endpoint.hash)
io.println(endpoint.href)

let bad = url.parse("not a url") or_else "fallback"
io.println(bad)
"#;
    let compiled = compile_code("url_differential.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("url_differential.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(
        vm_out,
        "https:\napi.example.com\n/v1/users\n?page=1\n#details\nhttps://api.example.com/v1/users?page=1#details\nfallback\n"
    );
}

#[test]
fn test_eager_list_methods_differential() {
    let code = r#"
let nums = [1, 2, 2, 3, 4, 5]
io.println(nums.take(3))
io.println(nums.skip(3))
io.println(nums.distinct())
io.println([1, 2].zip(["a", "b"]))
io.println([1, 2].chain([3, 4]))
io.println([1, 2, 3, 4, 5].chunk(2))
io.println([1, 2, 3, 4].window(2))
io.println(["x", "y"].enumerate())
"#;
    let compiled = compile_code("eager_list_methods.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("eager_list_methods.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(
        vm_out,
        "[1, 2, 2]\n[3, 4, 5]\n[1, 2, 3, 4, 5]\n[[1, a], [2, b]]\n[1, 2, 3, 4]\n[[1, 2], [3, 4], [5]]\n[[1, 2], [2, 3], [3, 4]]\n[[0, x], [1, y]]\n"
    );
}

#[test]
fn test_regex_module_differential() {
    let code = r##"
let p = regex.compile("^[a-z0-9_]+$") or_else "invalid"
io.println(p.is_match("user_123"))
io.println(p.is_match("user 123!"))

let digits = regex.compile("\\d+") or_else "invalid"
let s = "items: 12, 34, 56"
io.println(digits.find(s))
io.println(digits.find_all(s))
io.println(digits.replace(s, "#"))
io.println(digits.split(s))

let bad = regex.compile("[invalid") or_else "fallback_error"
io.println(bad)
"##;
    let compiled = compile_code("regex_differential.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("regex_differential.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(
        vm_out,
        "true\nfalse\n12\n[12, 34, 56]\nitems: #, #, #\n[items: , , , , , ]\nfallback_error\n"
    );
}

#[test]
fn test_binary_module_differential() {
    let code = r#"
let buf = Bytes(16)
binary.write_i16_be(buf, 0, 4660)
binary.write_u32_be(buf, 2, 305419896)
binary.write_f64_be(buf, 6, 3.141592653589793)

io.println(binary.read_i16_be(buf, 0))
io.println(binary.read_u32_be(buf, 2))
io.println(binary.read_f64_be(buf, 6))

let varbuf = Bytes(10)
let written = binary.write_varint(varbuf, 0, 624485)
io.println(written)
let read = binary.read_varint(varbuf, 0)
io.println(read)

let sub = binary.slice(buf, 0, 6)
io.println(sub.len())
"#;
    let compiled = compile_code("binary_differential.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("binary_differential.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(
        vm_out,
        "4660\n305419896\n3.141592653589793\n3\n[624485, 3]\n6\n"
    );
}

#[test]
fn test_time_pure_types_differential() {
    let code = r#"
let d = time.date(2026, 9, 22)
io.println(d.to_iso())
io.println(d.year)
io.println(d.month)
io.println(d.day)

let t = time.time_of_day(14, 30, 45, 500)
io.println(t.to_iso())

let dt = time.date_time(d, t, 0)
io.println(dt.to_iso())
io.println(dt.epoch_seconds())

let parsed = time.parse_iso("2026-09-22T14:30:45.500Z") or_else "invalid"
io.println(parsed.to_iso())
io.println(parsed.date().to_iso())
io.println(parsed.time().to_iso())
"#;
    let compiled = compile_code("time_pure_types_differential.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("time_pure_types_differential.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(
        vm_out,
        "2026-09-22\n2026\n9\n22\n14:30:45.500\n2026-09-22T14:30:45.500Z\n1790087445.5\n2026-09-22T14:30:45.500Z\n2026-09-22\n14:30:45.500\n"
    );
}

#[test]
fn test_expect_and_testing_differential() {
    let code = r#"
io.println(expect.equal(10, 10))
io.println(expect.not_equal(10, 20))
io.println(expect.true(true))
io.println(expect.false(false))
io.println(expect.none(none))
io.println(expect.some(42))
io.println(expect.failure(fail("err")))
io.println(expect.contains([1, 2, 3], 2))
io.println(expect.approx(3.14159, 3.1415, 0.001))

let mismatch = expect.equal(1, 2) or_else "recovered"
io.println(mismatch)
"#;
    let compiled = compile_code("expect_differential.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("expect_differential.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(
        vm_out,
        "none\nnone\nnone\nnone\nnone\nnone\nnone\nnone\nnone\nrecovered\n"
    );
}

#[test]
fn test_log_module_differential() {
    let code = r#"
log.info("system started")
log.error("failure detected", {"code": 500})
"#;
    let compiled = compile_code("log_differential.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("log_differential.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(
        vm_out,
        "[INFO] system started\n[ERROR] failure detected #{code: 500}\n"
    );
}

#[test]
fn test_is_nullable_and_multiple_is_differential() {
    let code = r#"
let a = none
let b = 100
let c = "hello"

io.println(a is Int?)
io.println(b is Int?)
io.println(c is Int?)

let x = 10
let y = 20
let z = 30
let ok1 = x, y, z is Int
let ok2 = x, "str", z is Int
let ok3 = (x, none is Int?)
io.println(ok1)
io.println(ok2)
io.println(ok3)
"#;
    let compiled = compile_code("is_differential.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("is_differential.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(vm_out, "true\ntrue\nfalse\ntrue\nfalse\ntrue\n");
}

#[test]
fn test_safe_navigation_differential() {
    // Canon: `a?.b` is `none` when the receiver is `none`, and the call form skips the
    // access and the arguments. Both backends must agree byte for byte.
    let code = r#"
struct Player {
    name
}
fn label(p) {
    return p?.name
}
io.println(String(label(none)))
io.println(String(label(Player{name = "ana"})))

struct Doubler {
    n
}
Doubler:twice(self) {
    return self.n * 2
}
fn run(d) {
    return d?.twice()
}
io.println(String(run(none)))
io.println(String(run(Doubler{n = 21})))
"#;
    let compiled = compile_code("safe_navigation.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("safe_navigation.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(vm_out, "none\nana\nnone\n42\n");
}

#[test]
fn test_or_else_is_lazy_differential() {
    // Canon: the fallback runs only when the left side ends in a `Failure`, and a failed
    // fallback propagates instead of being swallowed.
    let code = r#"
var calls = 0
fn fallback() {
    calls = calls + 1
    return 7
}
fn ok() {
    return 1
}
fn bad() {
    return Int("nope")
}
io.println(String(ok() or_else fallback()))
io.println(String(calls))
io.println(String(bad() or_else fallback()))
io.println(String(calls))
attempt {
    let x = bad() or_else bad()
    io.println(String(x))
} failed error {
    io.println("double failure caught")
}
"#;
    let compiled = compile_code("or_else_lazy.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("or_else_lazy.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(vm_out, "1\n0\n7\n1\ndouble failure caught\n");
}

#[test]
fn test_failure_in_conditions_propagates_differential() {
    // A recoverable `Failure` in a condition must propagate to `attempt`, not turn into a
    // type fault at the branch.
    let code = r#"
fn value(flag) {
    if flag {
        return Int("12")
    }
    return Int("bad")
}
attempt {
    var v = value(false)
    if v > 0 {
        io.println("positive")
    }
} failed error {
    io.println("condition failure caught")
}
var count = 0
while count < 3 {
    attempt {
        if count == 1 {
            count = count + 1
            continue
        }
    } failed error {
        io.println("nope")
    }
    count = count + 1
}
io.println(String(count))
"#;
    let compiled = compile_code("condition_failure.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("condition_failure.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(vm_out, "condition failure caught\n3\n");
}

#[test]
fn test_each_over_dict_and_unwind_differential() {
    // `each key[, value] in dict` follows insertion order; leaving a loop body with
    // `break`/`continue` must not leak handlers or iteration guards.
    let code = r#"
var d = {"a": 1, "b": 2}
var out = ""
each k, v in d {
    out = out + k + String(v)
}
io.println(out)
each k in d {
    io.println(k)
}
each i in 0..3 {
    if i == 1 {
        break
    }
}
each i in 0..3 {
    io.println(String(i))
}
"#;
    let compiled = compile_code("each_dict.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("each_dict.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(vm_out, "a1b2\na\nb\n0\n1\n2\n");
}

#[test]
fn test_assignment_failure_and_pipeline_order_differential() {
    // A `Failure` stored or read through an assignment propagates; `a |> f` evaluates `a`
    // before `f`, which source order makes observable.
    let code = r#"
struct P {
    x
}
fn set_bad(p) {
    p.x = Int("bad")
}
attempt {
    set_bad(P{x = 1})
} failed error {
    io.println("field failure caught")
}
var log = ""
fn left() {
    log = log + "L"
    return 1
}
fn right(x) {
    log = log + "R"
    return x + 1
}
io.println(String(left() |> right()))
io.println(log)
"#;
    let compiled = compile_code("assign_pipeline.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("assign_pipeline.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(vm_out, "field failure caught\n2\nLR\n");
}

#[test]
fn test_await_do_covers_block_statements_differential() {
    // `await do` awaits tasks produced by assignments inside `while` and by `return`.
    let code = r#"
async fn pick(n) {
    return n + 1
}
async fn looped() {
    var total = 0
    await do {
        while total < 3 {
            total = pick(total)
        }
        return pick(total)
    }
}
let result = await looped()
io.println(String(result))
"#;
    let compiled = compile_code("await_do_blocks.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("await_do_blocks.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(vm_out, "4\n");
}

#[test]
fn test_loop_unwind_does_not_leak_handlers_or_guards_differential() {
    // `break`/`continue` leaving an `attempt` must pop its handler, and `return` leaving an
    // `each` must release the iteration guard: a later failure is caught by the *outer*
    // handler, and the collection is mutable again.
    let code = r#"
attempt {
    each i in 0..3 {
        attempt {
            break
        } failed error {
            io.println("inner handler must not run")
        }
    }
    let bad = Int("boom")
    io.println("not reached")
} failed error {
    io.println("outer: " + error.message)
}

var items = [1, 2, 3]
fn first_two() {
    each v in items {
        if v == 2 {
            return v
        }
    }
    return -1
}
io.println(String(first_two()))
items.add(4)
io.println(String(items.len()))
"#;
    let compiled = compile_code("loop_unwind.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("loop_unwind.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(vm_out, "outer: invalid integer text: \"boom\"\n2\n4\n");
}

#[test]
fn test_differential_structured_failures() {
    let code = r#"
attempt {
    fail { "code": 404, "reason": "resource missing" }
} failed err {
    io.println(err.message)
    let p = err.payload
    io.println(String(p["code"]))
    io.println(String(p["reason"]))
}
"#;
    let compiled = compile_code("structured_fail.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("structured_fail.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert!(vm_out.contains("404"));
    assert!(vm_out.contains("resource missing"));
}

#[test]
fn test_sequence_pipeline_and_struct_display_differential() {
    let code = r#"
struct Point {
    x
    y
}
let p = Point{x: 1, y: 2}
io.println(p)

let xs = [1, 2, 3, 4, 5, 6]
let res = xs.lazy().filter(fn(x) { return x % 2 == 0 }).map(fn(x) { return x * 10 }).collect()
io.println(res)
"#;
    let compiled = compile_code("seq_diff.aipo", code);
    let vm_out = run_vm(&compiled);
    let js_out = run_js_code("seq_diff.aipo", code, &compiled);
    assert_eq!(vm_out, js_out);
    assert_eq!(vm_out, "Point{x: 1, y: 2}\n[20, 40, 60]\n");
}
