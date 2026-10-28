//! Package test-suite gate for the host-free Aipo packages.
//!
//! `aipo-zoe` and `aipo-game` suites need the injected `host_*` globals, so they
//! are wired into `crates/aipo-game-host/tests/bridge_tests.rs`. The packages
//! here need no host bridge and are therefore driven straight through
//! `aipo test`.
//!
//! Before this gate existed, nothing compiled or ran `packages/aipo-ui`, which
//! let its sources drift far outside the grammar for 148 parse errors across
//! every module. This test is the regression guard for that.

use std::fs;
use std::path::{Path, PathBuf};

const EXIT_SUCCESS: u8 = 0;

/// Host-free packages whose `tests/*.aipo` suites must stay green.
const SUITES: &[&str] = &["aipo-ui", "aipo-html", "aipo-http", "aipo-egui"];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("canonicalize workspace root")
}

fn run_test_cli(args: &[&str]) -> (u8, String, String) {
    let string_args: Vec<String> = args.iter().map(|s| (*s).to_string()).collect();
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let code = aipo_cli::run_with(&string_args, &mut out, &mut err);
    (
        code,
        String::from_utf8_lossy(&out).into_owned(),
        String::from_utf8_lossy(&err).into_owned(),
    )
}

/// Every `.aipo` file directly under `dir`, sorted for stable failure output.
fn collect_sources(dir: &Path) -> Vec<PathBuf> {
    let mut sources: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "aipo"))
        .collect();
    sources.sort();
    sources
}

#[test]
fn host_free_package_suites_pass() {
    let root = workspace_root();

    for package in SUITES {
        let dir = root.join("packages").join(package);
        assert!(
            dir.join("aipo.toml").exists(),
            "{package} must be a package at {}",
            dir.display()
        );
        assert!(
            dir.join("tests").is_dir(),
            "{package} must ship a tests/ directory so this gate has something to run"
        );

        let (code, out, err) = run_test_cli(&["test", dir.to_str().unwrap()]);

        assert_eq!(
            code, EXIT_SUCCESS,
            "{package} test suite failed\n--- stdout ---\n{out}\n--- stderr ---\n{err}"
        );
        assert!(
            out.contains("test result: ok"),
            "{package} did not report a passing result:\n{out}"
        );
    }
}

/// Formatter gate for `aipo.ui`.
///
/// All package sources are formatter-clean as of 2026-10-27. Keeping the
/// explicit `aipo.ui` assertion protects the package that originally drifted
/// by 148 parse errors, while the full-package check can be expanded later
/// without changing the regression guard.
#[test]
fn aipo_ui_sources_are_formatted() {
    let dir = workspace_root().join("packages/aipo-ui/src");
    let sources = collect_sources(&dir);
    assert!(
        !sources.is_empty(),
        "aipo.ui has no .aipo sources under {}",
        dir.display()
    );

    // `aipo fmt` takes files, not directories, so pass the list explicitly.
    let mut args = vec!["fmt", "--check"];
    args.extend(
        sources
            .iter()
            .map(|p| p.to_str().expect("utf-8 source path")),
    );

    let (code, out, err) = run_test_cli(&args);

    assert_eq!(
        code, EXIT_SUCCESS,
        "aipo.ui sources are not formatter-clean; run `aipo fmt packages/aipo-ui/src/*.aipo`\n\
         --- stdout ---\n{out}\n--- stderr ---\n{err}"
    );
}
