//! Member existence is a frontend contract for every built-in receiver type.
//!
//! The member sets come from the same registration the VM dispatches with, so a name
//! the analyzer accepts is one the runtime can resolve. Before that derivation, only
//! `List` and `String` were checked: a typo on a `Set`, `Bytes` or `Dict` receiver
//! passed `check` and only surfaced at runtime, and on `Dict` it surfaced as
//! `AIPO_RT_KEY_NOT_FOUND`, which points at a missing key instead of a missing method.

use std::path::Path;
use std::process::Command;

const EXIT_LANGUAGE_FAILURE: i32 = 1;

fn run_check(source: &str) -> (i32, String) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-artifacts");
    std::fs::create_dir_all(&dir).expect("artifact dir");
    let path = dir.join(format!(
        "member_check_{}_{}.aipo",
        std::process::id(),
        next_id()
    ));
    std::fs::write(&path, source).expect("write fixture");

    let output = Command::new(env!("CARGO_BIN_EXE_aipo"))
        .arg("check")
        .arg(&path)
        .output()
        .expect("run aipo check");
    let _ = std::fs::remove_file(&path);
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn next_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    COUNTER.fetch_add(1, Ordering::Relaxed)
}

#[test]
fn unknown_member_is_rejected_for_every_cataloged_type() {
    // One receiver per built-in type that registers methods. Each case must be a
    // frontend error, so the message is what a typo produces instead of a runtime fault.
    let cases = [
        ("List", "io.println([1].nope())"),
        ("String", "io.println(\"a\".nope())"),
        ("Set", "io.println(Set([1]).nope())"),
        ("Dict", "let d = {\"a\": 1}\nio.println(d.nope())"),
        ("Bytes", "io.println(Bytes(2).nope())"),
    ];
    for (type_name, source) in cases {
        let (code, stderr) = run_check(source);
        assert_eq!(
            code, EXIT_LANGUAGE_FAILURE,
            "`{type_name}` must reject an unknown member at check time; stderr: {stderr}"
        );
        assert!(
            stderr.contains("has no member 'nope'"),
            "`{type_name}` must name the member it rejects; stderr: {stderr}"
        );
    }
}

#[test]
fn every_real_member_still_checks() {
    // The catalog must not reject a method the runtime actually dispatches, including
    // the higher-order ones the VM drives rather than registers as natives.
    let source = "\
let xs = [1, 2, 3]
xs.first()
xs.transform(fn(x) { return x })
xs.filter(fn(x) { return x > 1 })
xs.any(fn(x) { return true })
xs.reduce(0, fn(a, b) { return a + b })
let s = Set([1])
s.add(2)
s.has(1)
let d = {\"a\": 1}
d.keys()
let b = Bytes(4)
b.read_u32(0)
io.println(\"ok\")
";
    let (code, stderr) = run_check(source);
    assert_eq!(
        code, 0,
        "every registered member must pass the check; stderr: {stderr}"
    );
}

#[test]
fn fallback_member_lists_do_not_lag_the_registration() {
    // `aipo_sema::check` without a standard library falls back to its own `List`
    // and `String` member lists. They exist so a bare analyzer still rejects a
    // typo, and nothing stops them drifting from the runtime registration the CLI
    // actually checks against — a member that exists at runtime but is missing
    // from the fallback would reject valid code, and the reverse would accept a
    // name that only faults later. This pins the two together.
    let surface = aipo_cli::prelude_surface();
    for (type_name, fallback) in [
        ("List", aipo_sema::LIST_MEMBERS),
        ("String", aipo_sema::STRING_MEMBERS),
    ] {
        // Compare against the catalog the analyzer actually consults, not the raw
        // native table: higher-order `List` methods are VM-driven and reach the
        // catalog through a separate list.
        let registered: std::collections::HashSet<String> = surface
            .type_members(type_name)
            .map(|members| members.iter().cloned().collect())
            .unwrap_or_default();
        let fallback: std::collections::HashSet<&str> = fallback.iter().copied().collect();

        let missing: Vec<&str> = fallback
            .iter()
            .copied()
            .filter(|name| !registered.contains(*name))
            .collect();
        assert!(
            missing.is_empty(),
            "`{type_name}` fallback lists members the runtime does not register: {missing:?}"
        );

        // The fallback is allowed to be a subset: it is a safety net for the bare
        // analyzer, not the authoritative catalog.
        let only_runtime: Vec<&String> = registered
            .iter()
            .filter(|name| !fallback.contains(name.as_str()))
            .collect();
        assert!(
            only_runtime.is_empty(),
            "`{type_name}` registers members its fallback rejects: {only_runtime:?}"
        );
    }
}
