//! Differential register/stack fixtures.
use std::path::{Path, PathBuf};
use std::process::Command;

fn conformance_programs() -> Vec<(PathBuf, PathBuf)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/conformance/programs");
    let mut cases = Vec::new();
    for entry in std::fs::read_dir(&root).expect("conformance programs directory") {
        let path = entry.expect("directory entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("aipo") {
            continue;
        }
        let stdout = path.with_extension("stdout");
        if stdout.is_file() {
            cases.push((path, stdout));
        }
    }
    cases.sort();
    cases
}

#[test]
fn register_and_stack_agree_on_every_conformance_program() {
    let cases = conformance_programs();
    assert!(
        cases.len() >= 30,
        "expected the full conformance corpus, found {}",
        cases.len()
    );
    for (source, expected_path) in cases {
        let name = source
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("program.aipo");
        let expected = std::fs::read(&expected_path).expect("fixture output");
        for engine in ["vm", "reg"] {
            let output = Command::new(env!("CARGO_BIN_EXE_aipo"))
                .arg("run")
                .arg(&source)
                .arg(format!("--engine={engine}"))
                .env("AIPOLANG", "en")
                .output()
                .expect("run Aipo");
            assert!(
                output.status.success(),
                "{name} ({engine}): {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(output.stdout, expected, "{name} ({engine})");
        }
    }
}

#[test]
fn plan_selects_the_register_backend_for_a_supported_program() {
    let source =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/conformance/programs/01_hello.aipo");
    let output = Command::new(env!("CARGO_BIN_EXE_aipo"))
        .arg("plan")
        .arg(&source)
        .env("AIPOLANG", "en")
        .output()
        .expect("run aipo plan");
    assert!(
        output.status.success(),
        "plan failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let plan = String::from_utf8_lossy(&output.stdout);
    assert!(
        plan.contains("\"engine\":\"reg\""),
        "expected a native register plan, got: {plan}"
    );
}
