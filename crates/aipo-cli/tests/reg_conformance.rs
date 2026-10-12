//! Differential register/stack fixtures. Added during reconstruction; not run in that delivery.
use std::path::Path;
use std::process::Command;

#[test]
fn register_and_stack_agree_on_dictionary_and_conditional_fixtures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/conformance/programs");
    for name in [
        "33_dict_primary_iteration",
        "34_conditional_values",
        "35_register_profile_shared_services",
        "36_semantic_type_tests",
    ] {
        let source = root.join(format!("{name}.aipo"));
        let expected = std::fs::read(root.join(format!("{name}.stdout"))).expect("fixture output");
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
