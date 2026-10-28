//! Keeps the public C header and the Rust `extern "C"` exports in sync.
use std::fs;
use std::path::Path;
use std::sync::OnceLock;

fn workspace_root() -> &'static Path {
    static ROOT: OnceLock<std::path::PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .canonicalize()
            .expect("workspace root")
    })
}

fn exported_functions() -> Vec<String> {
    let src = workspace_root().join("crates/aipo-c-abi/src");
    let mut names = Vec::new();
    for entry in fs::read_dir(src).expect("c-abi src") {
        let path = entry.expect("entry").path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let text = fs::read_to_string(&path).expect("read source");
        for line in text.lines() {
            let line = line.trim();
            let Some(rest) = line.strip_prefix("pub ") else {
                continue;
            };
            let rest = rest.strip_prefix("unsafe ").unwrap_or(rest);
            let Some(rest) = rest.strip_prefix("extern \"C\" fn ") else {
                continue;
            };
            if let Some(name) = rest.split('(').next() {
                names.push(name.trim().to_string());
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

fn declared_functions() -> Vec<String> {
    let header = fs::read_to_string(workspace_root().join("crates/aipo-c-abi/include/aipo.h"))
        .expect("read aipo.h");
    let mut names = Vec::new();
    for (index, _) in header.match_indices("aipo_") {
        let rest = &header[index..];
        let Some(end) = rest.find('(') else {
            continue;
        };
        let candidate = rest[..end].trim();
        if !candidate
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_')
        {
            continue;
        }
        if candidate.ends_with("_t") {
            continue;
        }
        names.push(candidate.to_string());
    }
    names.sort();
    names.dedup();
    names
}

#[test]
fn header_declares_every_export_and_no_extra_function() {
    let exported = exported_functions();
    let declared = declared_functions();
    assert!(!exported.is_empty(), "no exports found");
    assert_eq!(
        exported, declared,
        "aipo.h and extern \"C\" exports diverged\nexports: {exported:?}\ndeclared: {declared:?}"
    );
}
