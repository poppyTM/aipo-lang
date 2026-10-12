//! Host surface described as data: the `--ahs` flag (Wave 4, `P03-G01`).
//!
//! Canon requires the compiler to reach a host surface without knowing which host it is. The
//! AHS is that surface described as data, so these tests assert the three behaviors that make
//! the claim real: a host module is unknown without a description, resolvable with one, and a
//! description that contradicts itself is refused instead of half-applied.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

const EXIT_SUCCESS: u8 = 0;
const EXIT_LANGUAGE_FAILURE: u8 = 1;
const EXIT_USAGE: u8 = 2;

/// Serializes standard-library setup, which still installs a process-global clock.
fn run_lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn conformance_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/conformance")
}

fn probe_program() -> PathBuf {
    conformance_dir().join("ahs/host_surface_probe.aipo")
}

fn host_surface() -> PathBuf {
    conformance_dir().join("ahs/headless_test_host.json")
}

fn run_cli(args: &[&str]) -> (u8, String, String) {
    let _serialized = run_lock();
    let args: Vec<String> = args.iter().map(|arg| (*arg).to_string()).collect();
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let code = aipo_cli::run_with(&args, &mut out, &mut err);
    (
        code,
        String::from_utf8_lossy(&out).into_owned(),
        String::from_utf8_lossy(&err).into_owned(),
    )
}

fn run_source(name: &str, text: &str, flags: &[&str]) -> (u8, String, String) {
    let path = std::env::temp_dir().join(format!("aipo-ahs-{}-{name}.aipo", std::process::id()));
    std::fs::write(&path, text).expect("write program");
    let flag = format!("--ahs={}", host_surface().display());
    let mut args = vec!["check", &flag, path.to_str().expect("UTF-8 temp path")];
    args.extend_from_slice(flags);
    let result = run_cli(&args);
    std::fs::remove_file(path).expect("remove program");
    result
}

#[test]
fn test_a_host_module_is_unknown_without_a_surface() {
    let (code, _stdout, stderr) = run_cli(&["check", &probe_program().to_string_lossy()]);
    assert_eq!(code, EXIT_LANGUAGE_FAILURE);
    assert!(
        stderr.contains("AIPO_SEM_UNKNOWN_NAME"),
        "expected an unknown name for the host module, got: {stderr}"
    );
    assert!(stderr.contains("demo"), "{stderr}");
}

#[test]
fn test_a_host_surface_makes_its_modules_resolvable() {
    let (code, stdout, stderr) = run_cli(&[
        "check",
        &format!("--ahs={}", host_surface().display()),
        &probe_program().to_string_lossy(),
    ]);
    assert_eq!(code, EXIT_SUCCESS, "stderr: {stderr}");
    assert!(stdout.is_empty() && stderr.is_empty());
}

#[test]
fn test_the_surface_is_not_carried_into_the_next_invocation() {
    // The flag describes *this* run. A sticky surface would make the second check pass and
    // hide a program that only works because a previous invocation happened to install a host.
    let (code, _stdout, _stderr) = run_cli(&[
        "check",
        &format!("--ahs={}", host_surface().display()),
        &probe_program().to_string_lossy(),
    ]);
    assert_eq!(code, EXIT_SUCCESS);

    let (code, _stdout, stderr) = run_cli(&["check", &probe_program().to_string_lossy()]);
    assert_eq!(code, EXIT_LANGUAGE_FAILURE, "stderr: {stderr}");
    assert!(stderr.contains("AIPO_SEM_UNKNOWN_NAME"), "{stderr}");
}

#[test]
fn test_an_inconsistent_surface_is_a_usage_error() {
    let dir = std::env::temp_dir().join(format!("aipo-ahs-inconsistent-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    // `subject` names the trailing-block binding. `nope` is not a parameter of `ping`, which is
    // exactly the kind of contradiction that must not be half-applied.
    let broken = dir.join("broken.json");
    std::fs::write(
        &broken,
        r#"{"host":"headless-test-host","version":"0.1.0","modules":[{"name":"demo","functions":[{"name":"ping","params":[{"name":"value","type":{"name":"Int"}}],"subject":"nope"}]}]}"#,
    )
    .expect("write surface");

    let (code, _stdout, stderr) = run_cli(&[
        "check",
        &format!("--ahs={}", broken.display()),
        &probe_program().to_string_lossy(),
    ]);
    assert_eq!(code, EXIT_USAGE, "stderr: {stderr}");
    assert!(
        stderr.contains("subject 'nope' is not a parameter"),
        "the host's own problem must be reported: {stderr}"
    );
    std::fs::remove_file(&broken).expect("remove surface");
    std::fs::remove_dir(&dir).expect("remove temp dir");
}

#[test]
fn test_malformed_surface_json_is_a_usage_error() {
    let path = std::env::temp_dir().join(format!("aipo-ahs-malformed-{}.json", std::process::id()));
    std::fs::write(&path, "{not JSON}").expect("write malformed surface");
    let (code, _, stderr) = run_cli(&[
        "check",
        &format!("--ahs={}", path.display()),
        &probe_program().to_string_lossy(),
    ]);
    assert_eq!(code, EXIT_USAGE, "{stderr}");
    assert!(stderr.contains("invalid host surface"), "{stderr}");
    std::fs::remove_file(path).expect("remove malformed surface");
}

#[test]
fn test_binary_cli_consumes_ahs_and_emits_jsonl_contract_errors() {
    let flag = format!("--ahs={}", host_surface().display());
    let accepted = std::process::Command::new(env!("CARGO_BIN_EXE_aipo"))
        .args(["check", &flag])
        .arg(probe_program())
        .output()
        .expect("run aipo check");
    assert!(
        accepted.status.success(),
        "{}",
        String::from_utf8_lossy(&accepted.stderr)
    );
    assert!(accepted.stdout.is_empty() && accepted.stderr.is_empty());

    let rejected = std::process::Command::new(env!("CARGO_BIN_EXE_aipo"))
        .args(["check", &flag, "--message-format=jsonl"])
        .arg(conformance_dir().join("ahs/host_contract_violation.aipo"))
        .output()
        .expect("run aipo check with invalid contract");
    assert_eq!(
        rejected.status.code(),
        Some(i32::from(EXIT_LANGUAGE_FAILURE))
    );
    let stderr = String::from_utf8(rejected.stderr).expect("UTF-8 diagnostics");
    assert!(
        stderr.contains("\"code\":\"AIPO_SEM_CONTRACT_VIOLATION_STATIC\""),
        "{stderr}"
    );

    let unknown = std::process::Command::new(env!("CARGO_BIN_EXE_aipo"))
        .arg("check")
        .arg(probe_program())
        .output()
        .expect("run aipo check without AHS");
    assert_eq!(
        unknown.status.code(),
        Some(i32::from(EXIT_LANGUAGE_FAILURE))
    );
    assert!(String::from_utf8_lossy(&unknown.stderr).contains("AIPO_SEM_UNKNOWN_NAME"));
}

/// The prelude modules are the standard library's own host surface. Without member
/// signatures, `check` accepted *any* `modulo.membro`, so a typo like `math.nope(1.0)`
/// or a documented-but-absent `task.now()` passed the frontend and only faulted at
/// runtime with `AIPO_RT_KEY_NOT_FOUND`.
#[test]
fn test_prelude_module_members_are_checked_against_the_runtime() {
    let (code, _, stderr) = run_source(
        "prelude-member-ok",
        "io.println(String(math.sqrt(4.0)))",
        &[],
    );
    assert_eq!(code, EXIT_SUCCESS, "{stderr}");

    // Open-ended natives (`Value::native(.., usize::MAX, ..)`) record a lower bound,
    // so the extra arguments they genuinely accept must stay accepted.
    for (name, program) in [
        (
            "prelude-variadic-join",
            "io.println(path.join(\"a\", \"b\", \"c\"))",
        ),
        (
            "prelude-variadic-log",
            "log.info(\"mensagem\", {\"campo\": \"valor\"})",
        ),
        ("prelude-variadic-approx", "expect.approx(0.1, 0.2, 0.5)"),
    ] {
        let (code, _, stderr) = run_source(name, program, &[]);
        assert_eq!(code, EXIT_SUCCESS, "{name}: {stderr}");
    }

    for (name, program, expected) in [
        (
            "prelude-member-unknown",
            "io.println(String(math.nope(1.0)))",
            "unknown host member 'math.nope'",
        ),
        (
            "prelude-member-absent",
            "io.println(String(task.now()))",
            "unknown host member 'task.now'",
        ),
        (
            "prelude-member-wrong-arity",
            "io.println(String(math.sqrt(1.0, 2.0)))",
            "AIPO_SEM_ARITY_MISMATCH",
        ),
        (
            "prelude-member-too-few",
            "io.println(path.join())",
            "AIPO_SEM_ARITY_MISMATCH",
        ),
    ] {
        let (code, _, stderr) = run_source(name, program, &[]);
        assert_eq!(code, EXIT_LANGUAGE_FAILURE, "{name}: {stderr}");
        assert!(stderr.contains(expected), "{name}: {stderr}");
    }
}

#[test]
fn test_a_missing_surface_file_is_a_usage_error() {
    let (code, _stdout, stderr) = run_cli(&[
        "check",
        "--ahs=/definitely/not/a/surface.json",
        &probe_program().to_string_lossy(),
    ]);
    assert_eq!(code, EXIT_USAGE);
    assert!(stderr.contains("cannot read host surface"), "{stderr}");
}

#[test]
fn test_an_unknown_host_member_is_reported() {
    let program = conformance_dir().join("ahs/host_unknown_member.aipo");
    let (code, _stdout, stderr) = run_cli(&[
        "check",
        &format!("--ahs={}", host_surface().display()),
        &program.to_string_lossy(),
    ]);
    assert_eq!(code, EXIT_LANGUAGE_FAILURE, "stderr: {stderr}");
    assert!(
        stderr.contains("AIPO_SEM_UNKNOWN_NAME"),
        "expected unknown member, got: {stderr}"
    );
    assert!(stderr.contains("demo.nonexistent"), "{stderr}");
}

#[test]
fn test_a_host_call_with_wrong_arity_is_reported() {
    let program = conformance_dir().join("ahs/host_arity_mismatch.aipo");
    let (code, _stdout, stderr) = run_cli(&[
        "check",
        &format!("--ahs={}", host_surface().display()),
        &program.to_string_lossy(),
    ]);
    assert_eq!(code, EXIT_LANGUAGE_FAILURE, "stderr: {stderr}");
    assert!(
        stderr.contains("AIPO_SEM_ARITY_MISMATCH"),
        "expected arity mismatch, got: {stderr}"
    );
    assert!(stderr.contains("demo.ping"), "{stderr}");
}

#[test]
fn test_an_optional_host_parameter_may_be_omitted_or_supplied() {
    let program = conformance_dir().join("ahs/host_optional_param.aipo");
    let (code, _stdout, stderr) = run_cli(&[
        "check",
        &format!("--ahs={}", host_surface().display()),
        &program.to_string_lossy(),
    ]);
    assert_eq!(code, EXIT_SUCCESS, "stderr: {stderr}");
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn test_named_host_arguments_are_matched_by_parameter_name() {
    let (code, _, stderr) = run_source(
        "named-valid",
        "demo.greet(loud = true, name = \"Aipo\")\ndemo.greet(\"Aipo\", loud = false)\ndemo.ping(value = 42)",
        &[],
    );
    assert_eq!(code, EXIT_SUCCESS, "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
}

#[test]
fn test_invalid_named_host_arguments_are_reported() {
    for (name, program, expected) in [
        (
            "named-unknown",
            "demo.ping(nope = 1)",
            "AIPO_SEM_NAMED_ARG_UNKNOWN",
        ),
        (
            "named-duplicate",
            "demo.ping(value = 1, value = 2)",
            "AIPO_SEM_DUPLICATE_NAMED_ARG",
        ),
        (
            "mixed-duplicate",
            "demo.ping(1, value = 2)",
            "AIPO_SEM_DUPLICATE_NAMED_ARG",
        ),
        (
            "required-missing",
            "demo.greet(loud = true)",
            "AIPO_SEM_ARITY_MISMATCH",
        ),
        (
            "optional-contract",
            "demo.greet(\"Aipo\", loud = 1)",
            "AIPO_SEM_CONTRACT_VIOLATION_STATIC",
        ),
    ] {
        let (code, _, stderr) = run_source(name, program, &[]);
        assert_eq!(code, EXIT_LANGUAGE_FAILURE, "{name}: {stderr}");
        assert!(stderr.contains(expected), "{name}: {stderr}");
    }
}

#[test]
fn test_host_schema_does_not_override_local_shadowing() {
    let (code, _, stderr) = run_source(
        "shadowed",
        "struct Local {}\nLocal:ping(self) {}\nfn use_local(demo) { demo.nonexistent() }\nfn main() { let demo = Local{}\ndemo.ping() }",
        &[],
    );
    assert_eq!(code, EXIT_SUCCESS, "{stderr}");
}

#[test]
fn test_struct_method_checks_remain_enabled() {
    for (name, program, expected) in [
        (
            "method-arity",
            "struct Local {}\nLocal:ping(self, n: Int) {}\nlet item = Local{}\nitem.ping()",
            "AIPO_SEM_ARITY_MISMATCH",
        ),
        (
            "method-contract",
            "struct Local {}\nLocal:ping(self, n: Int) {}\nlet item = Local{}\nitem.ping(\"wrong\")",
            "AIPO_SEM_CONTRACT_VIOLATION_STATIC",
        ),
    ] {
        let (code, _, stderr) = run_source(name, program, &[]);
        assert_eq!(code, EXIT_LANGUAGE_FAILURE, "{stderr}");
        assert!(stderr.contains(expected), "{stderr}");
    }
}

#[test]
fn test_wasm_frontend_also_consumes_host_signatures() {
    let (code, _, stderr) = run_source("wasm-contract", "demo.ping(\"wrong\")", &["--wasm"]);
    assert_eq!(code, EXIT_LANGUAGE_FAILURE, "{stderr}");
    assert!(
        stderr.contains("AIPO_SEM_CONTRACT_VIOLATION_STATIC"),
        "{stderr}"
    );
    assert!(!stderr.contains("AIPO_SEM_UNKNOWN_NAME"), "{stderr}");
}

#[test]
fn test_host_signatures_are_preserved_for_embedding_without_leaking() {
    let _serialized = run_lock();
    let surface = aipo_cli::load_host_surface(&host_surface()).expect("load surface");
    let program = conformance_dir().join("ahs/host_contract_violation.aipo");
    let (_, diagnostics) =
        aipo_cli::compile_file(&program, Some(&surface)).expect_err("contract error");
    assert!(diagnostics.iter().any(|d| d.code == aipo_diagnostics::DiagnosticCode::AIPO_SEM_CONTRACT_VIOLATION_STATIC), "{diagnostics:?}");
    assert!(aipo_cli::compile_file(&probe_program(), Some(&surface)).is_ok());
    let (_, diagnostics) =
        aipo_cli::compile_file(&probe_program(), None).expect_err("unknown module");
    assert!(
        diagnostics
            .iter()
            .any(|d| d.code == aipo_diagnostics::DiagnosticCode::AIPO_SEM_UNKNOWN_NAME),
        "{diagnostics:?}"
    );
}

#[test]
fn test_nullable_host_contracts_and_dynamic_arguments() {
    let _serialized = run_lock();
    let mut surface = aipo_cli::load_host_surface(&host_surface()).expect("load surface");
    let mut ping = surface.host_function("demo", "ping").expect("ping").clone();
    ping.params[0].nullable = true;
    surface.add_host_module(
        "demo",
        std::collections::HashMap::from([("ping".to_string(), ping)]),
    );
    for (text, valid) in [
        ("demo.ping(none)", true),
        ("demo.ping(1)", true),
        ("let value = \"dynamic\"\ndemo.ping(value)", true),
        ("demo.ping(\"literal\")", false),
    ] {
        let source = aipo_source::Source::new(aipo_source::SourceId::next(), "nullable.aipo", text);
        let compiled = aipo_cli::analyze_with_surface(
            &source,
            Path::new("nullable.aipo"),
            None,
            Some(&surface),
        );
        assert_eq!(
            compiled.diagnostics.is_empty(),
            valid,
            "{text}: {:?}",
            compiled.diagnostics
        );
    }
}

#[test]
fn test_a_surface_does_not_leak_into_disassembly_or_after_load_failure() {
    let flag = format!("--ahs={}", host_surface().display());
    let path = probe_program();
    let program = path.to_string_lossy();
    assert_eq!(run_cli(&["check", &flag, &program]).0, EXIT_SUCCESS);
    let (code, _, stderr) = run_cli(&["disasm", &program]);
    assert_eq!(code, EXIT_LANGUAGE_FAILURE, "{stderr}");
    assert!(stderr.contains("AIPO_SEM_UNKNOWN_NAME"), "{stderr}");
    assert_eq!(
        run_cli(&["check", "--ahs=/missing.json", &program]).0,
        EXIT_USAGE
    );
    let _serialized = run_lock();
    assert!(aipo_cli::compile_file(&path, None).is_err());
}

#[test]
fn test_ahs_flag_rejects_empty_duplicate_and_unsupported_inputs() {
    let flag = format!("--ahs={}", host_surface().display());
    let path = probe_program();
    let program = path.to_string_lossy();
    for args in [
        vec!["check", "--ahs=", &program],
        vec!["check", "--ahs", "--wasm", &program],
        vec!["check", "--ahs"],
        vec!["check", &flag, &flag, &program],
        vec!["disasm", &flag, &program],
        vec!["run", &flag, "precompiled.aibc"],
        vec!["check", &flag, "precompiled.wasm"],
    ] {
        let (code, _, stderr) = run_cli(&args);
        assert_eq!(code, EXIT_USAGE, "{args:?}: {stderr}");
        assert!(stderr.contains("--ahs"), "{stderr}");
    }
}

#[test]
fn test_ahs_separate_argument_and_jsonl_diagnostics() {
    let (code, _, stderr) = run_cli(&[
        "check",
        "--ahs",
        &host_surface().to_string_lossy(),
        &probe_program().to_string_lossy(),
    ]);
    assert_eq!(code, EXIT_SUCCESS, "{stderr}");
    let (code, _, stderr) =
        run_source("jsonl", "demo.ping(\"wrong\")", &["--message-format=jsonl"]);
    assert_eq!(code, EXIT_LANGUAGE_FAILURE, "{stderr}");
    assert!(
        stderr.contains("\"code\":\"AIPO_SEM_CONTRACT_VIOLATION_STATIC\""),
        "{stderr}"
    );
}

#[test]
fn test_run_checks_the_description_but_does_not_install_natives() {
    let program = conformance_dir().join("ahs/host_contract_violation.aipo");
    let flag = format!("--ahs={}", host_surface().display());
    let (code, _, stderr) = run_cli(&["run", &flag, &program.to_string_lossy()]);
    assert_eq!(code, EXIT_LANGUAGE_FAILURE, "{stderr}");
    assert!(
        stderr.contains("AIPO_SEM_CONTRACT_VIOLATION_STATIC"),
        "{stderr}"
    );
    let (code, _, stderr) = run_cli(&["run", &flag, &probe_program().to_string_lossy()]);
    assert_eq!(code, EXIT_LANGUAGE_FAILURE, "{stderr}");
    assert!(stderr.contains("AIPO_RT_"), "{stderr}");
    assert!(!stderr.contains("AIPO_SEM_UNKNOWN_NAME"), "{stderr}");
}

#[test]
fn test_a_host_call_violating_contract_is_reported() {
    let program = conformance_dir().join("ahs/host_contract_violation.aipo");
    let (code, _stdout, stderr) = run_cli(&[
        "check",
        &format!("--ahs={}", host_surface().display()),
        &program.to_string_lossy(),
    ]);
    assert_eq!(code, EXIT_LANGUAGE_FAILURE, "stderr: {stderr}");
    assert!(
        stderr.contains("AIPO_SEM_CONTRACT_VIOLATION_STATIC"),
        "expected contract violation, got: {stderr}"
    );
    assert!(stderr.contains("demo.ping"), "{stderr}");
}
