// The disabled suite is an empty crate; do not require docs for that configuration.
#![cfg_attr(not(feature = "wasm"), allow(missing_docs))]
#![cfg(feature = "wasm")]

//! Integration tests for the Aipo CLI WebAssembly execution, build, and disassembly surface (ADP-013 / Marco 6).

use std::fs;
use std::path::PathBuf;

const EXIT_SUCCESS: u8 = 0;
const EXIT_LANGUAGE_FAILURE: u8 = 1;
#[cfg(feature = "wasmtime-runner")]
const EXIT_USAGE: u8 = 2;

fn run_cli(args: &[&str]) -> (u8, String, String) {
    let args_owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let code = aipo_cli::run_with(&args_owned, &mut out, &mut err);
    (
        code,
        String::from_utf8_lossy(&out).into_owned(),
        String::from_utf8_lossy(&err).into_owned(),
    )
}

fn temp_file(name: &str, content: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("aipo-wasm-test-{}-{name}", std::process::id()));
    fs::write(&path, content).expect("write temp file");
    path
}

fn temp_dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("aipo-wasm-dir-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("create temp dir");
    path
}

#[cfg(feature = "wasmtime-runner")]
#[test]
fn test_run_aipo_with_wasm_flag_and_host_io() {
    let script = temp_file(
        "io_hello.aipo",
        r#"
fn main() {
    io.print("Hello from WebAssembly! ")
    io.print(42)
    io.println()
}
"#,
    );

    let (code, stdout, stderr) = run_cli(&["run", script.to_str().unwrap(), "--wasm"]);
    assert_eq!(code, EXIT_SUCCESS, "stderr: {stderr}");
    assert_eq!(stdout, "Hello from WebAssembly! 42\n");

    let _ = fs::remove_file(script);
}

#[cfg(feature = "wasmtime-runner")]
#[test]
fn test_run_aipo_with_target_wasm() {
    let script = temp_file(
        "target_wasm.aipo",
        r#"
fn add(a: Int, b: Int) -> Int {
    return a + b
}

fn main() {
    io.print(add(15, 27))
    io.println()
}
"#,
    );

    let (code, stdout, stderr) = run_cli(&["run", script.to_str().unwrap(), "-t", "wasm"]);
    assert_eq!(code, EXIT_SUCCESS, "stderr: {stderr}");
    assert_eq!(stdout, "42\n");

    let (code2, stdout2, stderr2) = run_cli(&["run", script.to_str().unwrap(), "--target=wasm"]);
    assert_eq!(code2, EXIT_SUCCESS, "stderr: {stderr2}");
    assert_eq!(stdout2, "42\n");

    let _ = fs::remove_file(script);
}

#[cfg(feature = "wasmtime-runner")]
#[test]
fn test_build_and_run_wasm_binary() {
    let script = temp_file(
        "build_test.aipo",
        r#"
fn factorial(n: Int) -> Int {
    if n <= 1 {
        return 1
    }
    return n * factorial(n - 1)
}

fn main() {
    io.print(factorial(5))
    io.println()
}
"#,
    );

    let out_dir = temp_dir("build_out");

    // Build with --target wasm
    let (code, stdout, stderr) = run_cli(&[
        "build",
        script.to_str().unwrap(),
        "--target",
        "wasm",
        "--out",
        out_dir.to_str().unwrap(),
    ]);
    assert_eq!(code, EXIT_SUCCESS, "stderr: {stderr}");
    assert!(
        stdout.contains("built 1 file to"),
        "unexpected stdout: {stdout}"
    );

    let wasm_file = out_dir.join("app.wasm");
    assert!(wasm_file.exists(), "app.wasm was not generated");

    // Check magic bytes \0asm
    let bytes = fs::read(&wasm_file).expect("read app.wasm");
    assert_eq!(&bytes[0..4], b"\0asm");

    // Now execute the .wasm binary directly with `aipo run`
    let (run_code, run_stdout, run_stderr) = run_cli(&["run", wasm_file.to_str().unwrap()]);
    assert_eq!(run_code, EXIT_SUCCESS, "stderr: {run_stderr}");
    assert_eq!(run_stdout, "120\n");

    // Check .wasm binary with `aipo check`
    let (check_code, _, check_stderr) = run_cli(&["check", wasm_file.to_str().unwrap()]);
    assert_eq!(check_code, EXIT_SUCCESS, "stderr: {check_stderr}");

    // Disassemble .wasm binary with `aipo disasm`
    let (disasm_code, disasm_stdout, disasm_stderr) =
        run_cli(&["disasm", wasm_file.to_str().unwrap()]);
    assert_eq!(disasm_code, EXIT_SUCCESS, "stderr: {disasm_stderr}");
    assert!(
        disasm_stdout.contains("(module"),
        "expected WAT module header"
    );
    assert!(
        disasm_stdout.contains("factorial"),
        "expected factorial export"
    );

    let _ = fs::remove_file(script);
    let _ = fs::remove_dir_all(out_dir);
}

#[test]
fn test_build_with_wasm_flag_shorthand() {
    let script = temp_file(
        "build_flag.aipo",
        r#"
fn main() {
    io.print("build shorthand works")
}
"#,
    );

    let out_dir = temp_dir("build_shorthand_out");

    let (code, stdout, stderr) = run_cli(&[
        "build",
        script.to_str().unwrap(),
        "--wasm",
        "--out",
        out_dir.to_str().unwrap(),
    ]);
    assert_eq!(code, EXIT_SUCCESS, "stderr: {stderr}");
    assert!(stdout.contains("built 1 file to"));
    assert!(out_dir.join("app.wasm").exists());

    let _ = fs::remove_file(script);
    let _ = fs::remove_dir_all(out_dir);
}

#[test]
fn test_disasm_aipo_with_wasm_flag() {
    let script = temp_file(
        "disasm_test.aipo",
        r#"
fn square(x: Int) -> Int {
    return x * x
}
"#,
    );

    let (code, stdout, stderr) = run_cli(&["disasm", script.to_str().unwrap(), "--wasm"]);
    assert_eq!(code, EXIT_SUCCESS, "stderr: {stderr}");
    assert!(stdout.contains("(module"));
    assert!(stdout.contains("square"));
    assert!(stdout.contains("i64.mul"));

    let _ = fs::remove_file(script);
}

#[test]
fn test_check_aipo_with_wasm_flag() {
    let script = temp_file(
        "check_wasm.aipo",
        r#"
fn multiply(a: Int, b: Int) -> Int {
    return a * b
}
"#,
    );

    let (code, _, stderr) = run_cli(&["check", script.to_str().unwrap(), "--wasm"]);
    assert_eq!(code, EXIT_SUCCESS, "stderr: {stderr}");

    let _ = fs::remove_file(script);
}

#[cfg(feature = "wasmtime-runner")]
#[test]
fn test_wasm_error_handling_and_flags() {
    // Missing .wasm file
    let (code, _, stderr) = run_cli(&["run", "nonexistent_file.wasm"]);
    assert_eq!(code, EXIT_USAGE);
    assert!(stderr.contains("nonexistent_file.wasm"));

    // .wasm file with --package-cache is rejected
    let dummy_wasm = temp_file("dummy.wasm", "\0asm\x01\0\0\0");
    let (code, _, stderr) = run_cli(&[
        "run",
        dummy_wasm.to_str().unwrap(),
        "--package-cache",
        "/tmp",
    ]);
    assert_eq!(code, EXIT_USAGE);
    assert!(stderr.contains("--package-cache is not supported for .wasm files"));

    // .aibc file with --wasm is rejected
    let dummy_aibc = temp_file("dummy.aibc", "AIBC\x01\0\0\0");
    let (code, _, stderr) = run_cli(&["run", dummy_aibc.to_str().unwrap(), "--wasm"]);
    assert_eq!(code, EXIT_USAGE);
    assert!(stderr.contains("--wasm is not supported for .aibc files"));

    let (code, _, stderr) = run_cli(&["disasm", dummy_aibc.to_str().unwrap(), "--wasm"]);
    assert_eq!(code, EXIT_USAGE);
    assert!(stderr.contains("--wasm is not supported for .aibc files"));

    // Bad target name
    let (code, _, stderr) = run_cli(&["run", dummy_wasm.to_str().unwrap(), "-t", "unknown"]);
    assert_eq!(code, EXIT_USAGE);
    assert!(stderr.contains("unrecognized target 'unknown'"));

    let (code, _, stderr) = run_cli(&["build", dummy_wasm.to_str().unwrap(), "-t", "unknown"]);
    assert_eq!(code, EXIT_USAGE);
    assert!(stderr.contains("unrecognized build target 'unknown'"));

    let _ = fs::remove_file(dummy_wasm);
    let _ = fs::remove_file(dummy_aibc);
}

#[test]
fn test_wasm_syntax_or_semantic_error_returns_language_failure() {
    let script = temp_file(
        "bad_script.aipo",
        r#"
fn main() {
    let x = unknown_identifier
}
"#,
    );

    let (code, _, _) = run_cli(&["run", script.to_str().unwrap(), "--wasm"]);
    assert_eq!(code, EXIT_LANGUAGE_FAILURE);

    let (code, _, _) = run_cli(&["build", script.to_str().unwrap(), "--wasm"]);
    assert_eq!(code, EXIT_LANGUAGE_FAILURE);

    let _ = fs::remove_file(script);
}

#[test]
fn test_cli_instruction_budget_halts_infinite_loops() {
    let script = temp_file(
        "runaway.aipo",
        r#"
var x = 0
while true {
    x += 1
}
"#,
    );

    // 1. WebAssembly JIT with --budget
    let (code, _, stderr) = run_cli(&["run", script.to_str().unwrap(), "--wasm", "--budget=500"]);
    assert_eq!(code, EXIT_LANGUAGE_FAILURE);
    assert!(stderr.contains("AIPO_RT_OVERFLOW"), "Wasm stderr: {stderr}");
    assert!(
        stderr.contains("execution budget exceeded"),
        "Wasm stderr: {stderr}"
    );

    // 2. WebAssembly JIT with --max-instructions
    let (code, _, stderr) = run_cli(&[
        "run",
        script.to_str().unwrap(),
        "--wasm",
        "--max-instructions=500",
    ]);
    assert_eq!(code, EXIT_LANGUAGE_FAILURE);
    assert!(stderr.contains("AIPO_RT_OVERFLOW"), "Wasm stderr: {stderr}");

    // 3. Stack VM with --max-instructions
    let (code, _, stderr) = run_cli(&["run", script.to_str().unwrap(), "--max-instructions=100"]);
    assert_eq!(code, EXIT_LANGUAGE_FAILURE);
    assert!(
        stderr.contains("AIPO_RT_OVERFLOW"),
        "Stack VM stderr: {stderr}"
    );

    // 4. Register VM with --budget
    let (code, _, stderr) = run_cli(&[
        "run",
        script.to_str().unwrap(),
        "--engine=reg",
        "--budget=100",
    ]);
    assert_eq!(code, EXIT_LANGUAGE_FAILURE);
    assert!(
        stderr.contains("AIPO_RT_OVERFLOW"),
        "Reg VM stderr: {stderr}"
    );

    let _ = fs::remove_file(script);
}

#[test]
fn test_cli_budget_happy_path_completes() {
    let script = temp_file(
        "finite_calc.aipo",
        r#"
var sum = 0
repeat 10 as i {
    sum += i
}
io.println(sum)
"#,
    );

    // Stack VM
    let (code, _, stderr) = run_cli(&["run", script.to_str().unwrap(), "--budget=10000"]);
    assert_eq!(code, EXIT_SUCCESS, "stderr: {stderr}");

    // Reg VM
    let (code, _, stderr) = run_cli(&[
        "run",
        script.to_str().unwrap(),
        "--engine=reg",
        "--budget=10000",
    ]);
    assert_eq!(code, EXIT_SUCCESS, "stderr: {stderr}");

    // Wasm (routes io directly to CLI output stream)
    let (code, stdout, stderr) =
        run_cli(&["run", script.to_str().unwrap(), "--wasm", "--budget=100000"]);
    assert_eq!(code, EXIT_SUCCESS, "stderr: {stderr}");
    assert_eq!(stdout, "45\n");

    let _ = fs::remove_file(script);
}

#[test]
fn test_cli_budget_flag_validation() {
    let script = temp_file("empty.aipo", "fn main() {}\n");

    // Rejected on check command
    let (code, _, stderr) = run_cli(&["check", script.to_str().unwrap(), "--budget=100"]);
    assert_eq!(code, EXIT_USAGE);
    assert!(stderr.contains("--budget is only supported by run"));

    // Duplicate flag rejected
    let (code, _, stderr) = run_cli(&[
        "run",
        script.to_str().unwrap(),
        "--budget=100",
        "--max-instructions=200",
    ]);
    assert_eq!(code, EXIT_USAGE);
    assert!(stderr.contains("was provided more than once"));

    // Non-integer budget rejected
    let (code, _, stderr) = run_cli(&["run", script.to_str().unwrap(), "--budget=invalid"]);
    assert_eq!(code, EXIT_USAGE);
    assert!(stderr.contains("invalid budget 'invalid'"));

    let _ = fs::remove_file(script);
}

#[test]
fn test_wasm_unsupported_reports_compile_diagnostic() {
    let script = temp_file(
        "unsupported.aipo",
        "let items = [1, 2]\nio.println(len(items))\n",
    );
    let (code, _, stderr) = run_cli(&["check", script.to_str().unwrap(), "--wasm"]);
    assert_eq!(code, EXIT_LANGUAGE_FAILURE, "stderr: {stderr}");
    assert!(
        stderr.contains("AIPO_COMPILE_WASM_UNSUPPORTED"),
        "expected compile diagnostic, got: {stderr}"
    );
    assert!(
        !stderr.contains("AIPO_RT_TYPE_MISMATCH"),
        "backend limitation must not be a runtime fault: {stderr}"
    );
    let _ = fs::remove_file(script);
}
