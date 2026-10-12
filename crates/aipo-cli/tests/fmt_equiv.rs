//! Formatter equivalence: `fmt` is idempotent and preserves behavior.
//!
//! For every corpus program and a sample of generated programs:
//! `fmt(fmt(x)) == fmt(x)`, and `run(parse(fmt(x))) == run(parse(x))` on the
//! VM. (JS agreement of formatted sources follows from the differential
//! suites, which run the same pipeline rainbow.)

#![forbid(unsafe_code)]
#![cfg(feature = "formatter")]

use aipo_testkit::{corpus, pipeline, smith};
use std::sync::{Mutex, MutexGuard, OnceLock};

fn vm_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
}

fn formatted(name: &str, source: &str) -> String {
    aipo_formatter::format_text(name, source).expect("corpus program formats")
}

fn assert_fmt_equivalence(label: &str, source: &str) {
    let once = formatted("fmt.aipo", source);
    let twice = formatted("fmt.aipo", &once);
    assert_eq!(once, twice, "{label}: fmt is idempotent");
    let _guard = vm_lock();
    let first = pipeline::run_text("fmt.aipo", source);
    let second = pipeline::run_text("fmt.aipo", &once);
    assert_eq!(
        first.is_ok(),
        second.is_ok(),
        "{label}: formatted program exits alike"
    );
    if let (Ok(a), Ok(b)) = (first, second) {
        assert_eq!(a, b, "{label}: formatted program prints alike");
    }
}

#[test]
fn test_fmt_idempotent_and_equivalent_over_corpus() {
    for (path, _) in corpus::runnable_programs() {
        let source = std::fs::read_to_string(&path).expect("fixture is readable");
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("program.aipo");
        assert_fmt_equivalence(name, &source);
    }
}

#[test]
fn test_fmt_idempotent_and_equivalent_generated() {
    let config = smith::Config::default();
    for seed in [5u64, 42, 314] {
        let generated = smith::generate(seed, &config);
        assert_fmt_equivalence(&format!("smith-{seed}"), &generated.source);
    }
}
