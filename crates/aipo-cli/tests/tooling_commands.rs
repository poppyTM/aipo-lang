//! Integration coverage for the developer tooling surface.
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("aipo-tooling-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("scratch directory");
    dir
}

fn run(args: &[String]) -> (u8, String, String) {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let code = aipo_cli::run_with(args, &mut out, &mut err);
    (
        code,
        String::from_utf8_lossy(&out).into_owned(),
        String::from_utf8_lossy(&err).into_owned(),
    )
}

fn string_args(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_string()).collect()
}

#[test]
fn new_plan_profile_build_and_disasm_work() {
    let dir = scratch("commands");
    let project = dir.join("demo");
    let (code, _, err) = run(&string_args(&["new", project.to_str().unwrap()]));
    assert_eq!(code, 0, "new failed: {err}");
    assert!(project.join("aipo.toml").is_file());
    assert!(project.join("src/main.aipo").is_file());

    let (code, _, err) = run(&string_args(&["new", project.to_str().unwrap()]));
    assert_eq!(code, 2, "new must refuse an existing directory: {err}");

    let source = project.join("src/main.aipo");
    let (code, out, err) = run(&string_args(&["plan", source.to_str().unwrap()]));
    assert_eq!(code, 0, "plan failed: {err}");
    assert!(out.contains("\"engine\""), "plan output: {out}");

    let (code, out, err) = run(&string_args(&[
        "profile",
        source.to_str().unwrap(),
        "--budget",
        "100000",
        "--json",
    ]));
    assert_eq!(code, 0, "profile failed: {err}");
    assert!(out.contains("\"ok\":true"), "profile output: {out}");

    let (code, out, err) = run(&string_args(&["disasm", source.to_str().unwrap()]));
    assert_eq!(code, 0, "disasm failed: {err}");
    assert!(
        out.contains("Disassembly") || out.contains("Op"),
        "disasm output: {out}"
    );

    let js_dir = dir.join("js");
    let (code, out, err) = run(&string_args(&[
        "build",
        source.to_str().unwrap(),
        "--target",
        "js",
        "--out",
        js_dir.to_str().unwrap(),
    ]));
    assert_eq!(code, 0, "build js failed: {err}");
    assert!(out.contains("built 4 files"), "build output: {out}");
    assert_eq!(
        fs::read_to_string(js_dir.join("package.json")).expect("package.json"),
        "{\n  \"type\": \"module\"\n}\n"
    );

    let wasm_dir = dir.join("wasm");
    let (code, _, err) = run(&string_args(&[
        "build",
        source.to_str().unwrap(),
        "--target",
        "wasm",
        "--out",
        wasm_dir.to_str().unwrap(),
    ]));
    assert_eq!(code, 0, "build wasm failed: {err}");
    let magic = fs::read(wasm_dir.join("app.wasm")).expect("wasm binary");
    assert_eq!(&magic[..4], b"\0asm");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn package_lock_and_audit_work() {
    let dir = scratch("package");
    let project = dir.join("demo");
    let (code, _, err) = run(&string_args(&["new", project.to_str().unwrap()]));
    assert_eq!(code, 0, "new failed: {err}");
    let (code, _, err) = run(&string_args(&[
        "package",
        "lock",
        project.to_str().unwrap(),
    ]));
    assert_eq!(code, 0, "package lock failed: {err}");
    assert!(project.join("aipo.lock").is_file());
    let (code, _, err) = run(&string_args(&[
        "package",
        "audit",
        project.to_str().unwrap(),
    ]));
    assert_eq!(code, 0, "package audit failed: {err}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn test_runner_discovers_directive_and_name_conventions() {
    let dir = scratch("test-runner");
    fs::write(
        dir.join("sample_test.aipo"),
        "#!test\nfn sample() {\n    io.println(\"ok\")\n}\n",
    )
    .expect("test fixture");
    let (code, out, err) = run(&string_args(&["test", dir.to_str().unwrap()]));
    assert_eq!(code, 0, "test runner failed: {err}");
    assert!(out.contains("test result: ok"), "test output: {out}");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn lsp_initializes_and_shuts_down() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_aipo"))
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn aipo lsp");
    let initialize = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {"capabilities": {}}
    });
    let shutdown = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "shutdown",
        "params": null
    });
    let exit = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "exit",
        "params": null
    });
    let mut input = Vec::new();
    for message in [initialize, shutdown, exit] {
        let body = serde_json::to_vec(&message).expect("json");
        input.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
        input.extend_from_slice(&body);
    }
    {
        let stdin = child.stdin.as_mut().expect("lsp stdin");
        stdin.write_all(&input).expect("write lsp input");
    }
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("lsp exits");
    assert!(
        output.status.success(),
        "lsp failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"serverInfo\""), "lsp output: {stdout}");
}

#[test]
fn debug_accepts_quit_and_watch_reports_usage() {
    let dir = scratch("interactive");
    let source = dir.join("main.aipo");
    fs::write(&source, "io.println(\"debug\")\n").expect("debug fixture");
    let mut child = Command::new(env!("CARGO_BIN_EXE_aipo"))
        .arg("debug")
        .arg(&source)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn aipo debug");
    {
        let stdin = child.stdin.as_mut().expect("debug stdin");
        stdin.write_all(b"quit\n").expect("write debug input");
    }
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("debug exits");
    assert!(
        output.status.success(),
        "debug failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("debug>"), "debug output: {stdout}");

    let (code, _, err) = run(&string_args(&[
        "watch",
        dir.join("missing.aipo").to_str().unwrap(),
    ]));
    assert_eq!(code, 2, "watch must reject a missing path: {err}");
    let _ = fs::remove_dir_all(&dir);
}
