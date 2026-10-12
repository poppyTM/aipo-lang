//! Native C host integration test verifying Aipo C ABI under gcc/clang and AddressSanitizer.
//!
//! This integration test compiles `tests/host/main.c` against `include/aipo.h` and links
//! it with the compiled `libaipo_c_abi.so` / `libaipo_c_abi.a`. It executes the resulting
//! binary to verify that real C host code interacts safely with the Aipo runtime.

use std::path::PathBuf;
use std::process::Command;

fn find_c_compiler() -> Option<&'static str> {
    for compiler in ["clang", "gcc", "cc"] {
        if let Ok(output) = Command::new(compiler).arg("--version").output()
            && output.status.success()
        {
            return Some(compiler);
        }
    }
    None
}

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("valid workspace root")
        .to_path_buf()
}

fn target_dir() -> PathBuf {
    let mut dir = project_root();
    dir.push("target");
    if let Ok(target) = std::env::var("CARGO_TARGET_DIR") {
        dir = PathBuf::from(target);
    }
    // Default to debug directory
    dir.push("debug");
    dir
}

#[test]
fn test_native_c_host_harness() {
    let compiler = match find_c_compiler() {
        Some(c) => c,
        None => {
            eprintln!("Skipping native C host test: no C compiler found in PATH");
            return;
        }
    };

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let include_dir = manifest_dir.join("include");
    let c_source = manifest_dir.join("tests").join("host").join("main.c");
    let target_debug = target_dir();

    assert!(include_dir.exists(), "include dir must exist");
    assert!(c_source.exists(), "main.c must exist");

    // Ensure libaipo_c_abi cdylib is up to date
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let build_status = Command::new(cargo)
        .args(["build", "-p", "aipo-c-abi"])
        .current_dir(project_root())
        .status()
        .expect("cargo build should execute");
    assert!(
        build_status.success(),
        "cargo build -p aipo-c-abi must succeed"
    );

    let out_bin = std::env::temp_dir().join("aipo_c_host_test_runner");

    // Compile native C test
    let compile_status = Command::new(compiler)
        .arg("-Wall")
        .arg("-Wextra")
        .arg("-Werror")
        .arg(format!("-I{}", include_dir.display()))
        .arg(&c_source)
        .arg(format!("-L{}", target_debug.display()))
        .arg(format!("-Wl,-rpath,{}", target_debug.display()))
        .arg("-laipo_c_abi")
        .arg("-lpthread")
        .arg("-ldl")
        .arg("-lm")
        .arg("-o")
        .arg(&out_bin)
        .status()
        .expect("failed to execute C compiler");

    assert!(compile_status.success(), "C host test compilation failed");

    // Execute native C test
    let run_status = Command::new(&out_bin)
        .status()
        .expect("failed to run C host test binary");

    assert!(run_status.success(), "C host test binary failed execution");

    // If clang is the compiler, also compile and run with AddressSanitizer and UBSan
    if compiler == "clang" {
        let asan_bin = std::env::temp_dir().join("aipo_c_host_asan_runner");
        let asan_compile = Command::new("clang")
            .arg("-Wall")
            .arg("-Wextra")
            .arg("-Werror")
            .arg("-fsanitize=address,undefined")
            .arg(format!("-I{}", include_dir.display()))
            .arg(&c_source)
            .arg(format!("-L{}", target_debug.display()))
            .arg(format!("-Wl,-rpath,{}", target_debug.display()))
            .arg("-laipo_c_abi")
            .arg("-lpthread")
            .arg("-ldl")
            .arg("-lm")
            .arg("-o")
            .arg(&asan_bin)
            .status();

        if let Ok(status) = asan_compile
            && status.success()
        {
            let asan_run = Command::new(&asan_bin).status();
            if let Ok(run_res) = asan_run {
                assert!(
                    run_res.success(),
                    "AddressSanitizer reported violations in native C harness"
                );
            }
            let _ = std::fs::remove_file(&asan_bin);
        }
    }

    let _ = std::fs::remove_file(&out_bin);
}
