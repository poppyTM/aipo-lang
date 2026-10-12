//! Compile-time type-proof regressions; not executed in P07-G02.
use aipo_source::{Source, SourceId};

fn compile(text: &str) -> Result<Vec<u8>, aipo_wasm::WasmCompileError> {
    let source = Source::new(SourceId::next(), "types.aipo", text);
    let (program, diagnostics) = aipo_syntax::parse(&source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");
    aipo_wasm::compile_hir(&aipo_hir::lower(program))
}

#[test]
fn untyped_parameters_and_mixed_joins_do_not_get_a_fabricated_int_tag() {
    assert!(compile("fn uncertain(value) { return value is Int }\n").is_err());
    assert!(
        compile(
            "fn mixed(flag: Bool) {\nlet value = if flag then 1 else true\nreturn value is Int\n}\n"
        )
        .is_err()
    );
}

#[test]
fn semantic_bool_and_nullable_none_can_be_proven() {
    assert!(compile("fn yes() { return true is Bool }\nfn no() { return true is Int }\nfn nullable() { return none is Int? }\n").is_ok());
}

#[test]
fn aliased_shadowed_and_unknown_type_targets_are_rejected() {
    for source in [
        "fn alias() { let T = Int\nreturn 1 is T }\n",
        "fn shadow(Int) { return 1 is Int }\n",
        "var Int = 1\nfn shadow() { return 1 is Int }\n",
        "fn Int() { return 1 }\nfn shadow() { return 1 is Int }\n",
        "fn unknown() { return 1 is Missing }\n",
        "fn String(value) { return 1 }\nfn shadow() { return String(1) is Int }\n",
    ] {
        assert!(
            compile(source).is_err(),
            "accepted unproven type target: {source}"
        );
    }
}
