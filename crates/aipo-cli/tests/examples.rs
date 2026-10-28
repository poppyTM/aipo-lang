//! Examples harness: documentation that executes.
//!
//! Every runnable example (top-level `examples/*.aipo` with a committed
//! `.stdout`, plus `<name>/main.aipo` multi-file cases) runs on the VM and on
//! Node from `aipo build`, and all three outputs must agree. Examples are
//! user-facing artifacts: if one rots, this suite fails.

#![forbid(unsafe_code)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

fn lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

/// Program output flows through the process-global stdlib sink, not the CLI
/// streams, so it is captured separately from command output.
#[derive(Clone)]
struct CaptureSink(Arc<Mutex<Vec<u8>>>);

impl Write for CaptureSink {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn discover() -> Vec<(PathBuf, PathBuf)> {
    let dir = examples_dir();
    let mut cases = Vec::new();
    let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("examples dir is readable")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .collect();
    entries.sort();
    for entry in entries {
        if entry.is_dir() {
            let main = entry.join("main.aipo");
            let stdout = entry.join("main.stdout");
            if main.exists() && stdout.exists() {
                cases.push((main, stdout));
            }
            continue;
        }
        if entry.extension().and_then(|e| e.to_str()) == Some("aipo") {
            let stdout = entry.with_extension("stdout");
            if stdout.exists() {
                cases.push((entry, stdout));
            }
        }
    }
    cases
}

fn run_vm(entry: &Path) -> (u8, String, String) {
    let _guard = lock();
    let captured = Arc::new(Mutex::new(Vec::new()));
    aipo_cli::set_output_sink(Some(Box::new(CaptureSink(Arc::clone(&captured)))));
    let args = vec!["run".to_string(), entry.to_string_lossy().into_owned()];
    let mut out: Vec<u8> = Vec::new();
    let mut err: Vec<u8> = Vec::new();
    let code = aipo_cli::run_with(&args, &mut out, &mut err);
    aipo_cli::set_output_sink(None);
    let program_output = captured
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clone();
    (
        code,
        String::from_utf8_lossy(&program_output).into_owned(),
        String::from_utf8_lossy(&err).into_owned(),
    )
}

fn build_and_run_node(entry: &Path, tag: &str) -> (Option<i32>, String, String) {
    let pid = std::process::id();
    let out_dir = std::env::temp_dir().join(format!("aipo-example-{pid}-{tag}"));
    let _ = std::fs::remove_dir_all(&out_dir);
    let build = vec![
        "build".to_string(),
        entry.to_string_lossy().into_owned(),
        "--out".to_string(),
        out_dir.to_string_lossy().into_owned(),
    ];
    let (code, build_out, build_err) = {
        let _guard = lock();
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        let code = aipo_cli::run_with(&build, &mut out, &mut err);
        (
            code,
            String::from_utf8_lossy(&out).into_owned(),
            String::from_utf8_lossy(&err).into_owned(),
        )
    };
    assert_eq!(code, 0, "example builds: {}", entry.display());
    assert!(build_err.is_empty(), "build is quiet: {build_err}");
    assert!(
        build_out.contains("built 4 files"),
        "JS builds emit app.js, runtime, map and package.json: {build_out}"
    );
    let manifest = std::fs::read_to_string(out_dir.join("package.json"))
        .expect("JS builds write package.json");
    assert!(
        manifest.contains("\"type\": \"module\""),
        "package.json selects ESM: {manifest}"
    );
    let output = Command::new("node")
        .arg("app.js")
        .current_dir(&out_dir)
        .output()
        .expect("node runs (requires Node >= 20)");
    let result = (
        output.status.code(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    );
    let _ = std::fs::remove_dir_all(&out_dir);
    result
}

#[test]
fn test_examples_run_identically_on_both_backends() {
    let cases = discover();
    assert!(cases.len() >= 20, "examples library has substance");
    for (entry, snapshot) in cases {
        let name = entry
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("example.aipo");
        let parent = entry
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("examples");
        let tag = format!("{parent}-{name}");
        let expected = std::fs::read_to_string(&snapshot).expect("snapshot is readable");
        let (code, vm_stdout, vm_stderr) = run_vm(&entry);
        assert_eq!(code, 0, "{tag} runs on the VM:\n{vm_stderr}");
        assert_eq!(vm_stdout, expected, "{tag} matches its snapshot");
        let (node_code, js_stdout, js_stderr) = build_and_run_node(&entry, &tag);
        assert_eq!(node_code, Some(0), "{tag} runs on node:\n{js_stderr}");
        assert_eq!(js_stdout, expected, "{tag} matches on JS too");
    }
}
