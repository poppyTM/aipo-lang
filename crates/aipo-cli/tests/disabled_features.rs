//! Disabled execution features return an explicit language failure.
#[cfg(not(feature = "wasm"))]
#[test]
fn wasm_flag_reports_disabled_backend() {
    let path = std::env::temp_dir().join(format!("aipo-disabled-wasm-{}.aipo", std::process::id()));
    std::fs::write(&path, "io.println(1)\n").expect("write source");
    let mut out = Vec::new();
    let mut err = Vec::new();
    let args = vec![
        "run".to_string(),
        path.to_string_lossy().into_owned(),
        "--wasm".to_string(),
    ];
    let code = aipo_cli::run_with(&args, &mut out, &mut err);
    let _ = std::fs::remove_file(path);
    assert_eq!(code, 1);
    assert!(String::from_utf8_lossy(&err).contains("disabled"));
}

#[cfg(all(feature = "wasm", not(feature = "wasmtime-runner")))]
#[test]
fn binary_execution_reports_disabled_runner() {
    let path =
        std::env::temp_dir().join(format!("aipo-disabled-runner-{}.wasm", std::process::id()));
    std::fs::write(&path, b"\0asm\x01\0\0\0").expect("write wasm header");
    let mut out = Vec::new();
    let mut err = Vec::new();
    let args = vec!["run".to_string(), path.to_string_lossy().into_owned()];
    let code = aipo_cli::run_with(&args, &mut out, &mut err);
    let _ = std::fs::remove_file(path);
    assert_eq!(code, 1);
    assert!(String::from_utf8_lossy(&err).contains("disabled"));
}
