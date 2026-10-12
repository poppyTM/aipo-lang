//! Unicode properties for the stdlib string layer.
//!
//! The contract under test (see `docs/stdlib/mvp-subset.md`): NFC is an
//! invariant established at construction boundaries, all operations count code
//! points (except `byte_len`), and `reverse` is grapheme-aware. Characterization
//! cases (extreme combining runs, emoji, mixed scripts) assert termination and
//! documented behavior without inventing new restrictions.

#![forbid(unsafe_code)]

use aipo_stdlib::string;
use aipo_vm::Value;
use std::rc::Rc;

fn text(value: &str) -> Value {
    Value::String(Rc::new(value.to_string()))
}

fn rendered(result: Value) -> String {
    match result {
        Value::String(s) => s.to_string(),
        other => panic!("expected String, got {other:?}"),
    }
}

fn int(result: Value) -> i64 {
    match result {
        Value::Int(n) => n,
        other => panic!("expected Int, got {other:?}"),
    }
}

#[test]
fn test_nfc_is_idempotent_and_composed() {
    // e + combining acute must equal precomposed \u{e9} after every normalizing op.
    let decomposed = text("e\u{301}");
    assert_eq!(
        rendered(string::string_lower(std::slice::from_ref(&decomposed)).unwrap()),
        "\u{e9}"
    );
    assert_eq!(
        rendered(string::string_upper(std::slice::from_ref(&decomposed)).unwrap()),
        "\u{c9}"
    );
    assert_eq!(
        rendered(string::string_reverse(std::slice::from_ref(&decomposed)).unwrap()),
        "\u{e9}"
    );
    let joined = string::string_join(&[
        text("-"),
        Value::List(Rc::new(std::cell::RefCell::new(
            vec![decomposed.clone(), text("x")].into(),
        ))),
    ]);
    assert_eq!(rendered(joined.unwrap()), "\u{e9}-x");
}

#[test]
fn test_composed_and_decomposed_agree() {
    let pairs = [
        ("\u{e9}", "e\u{301}"),
        ("\u{fc}", "u\u{308}"),
        ("\u{f1}", "n\u{303}"),
    ];
    for (composed, decomposed) in pairs {
        let left = text(composed);
        let right = text(decomposed);
        // Raw decomposed input counts its marks: NFC is established by the
        // Aipo construction boundaries (lexer, conversion, concat), not by
        // re-scanning every argument of every op. Normalizing ops agree.
        assert_eq!(
            int(string::string_len(std::slice::from_ref(&right)).unwrap()),
            2,
            "decomposed counts marks for {composed:?}"
        );
        assert_eq!(
            rendered(string::string_reverse(std::slice::from_ref(&left)).unwrap()),
            rendered(string::string_reverse(std::slice::from_ref(&right)).unwrap()),
            "reverse agrees for {composed:?}"
        );
        assert_eq!(
            rendered(string::string_upper(std::slice::from_ref(&left)).unwrap()),
            rendered(string::string_upper(std::slice::from_ref(&right)).unwrap()),
            "upper agrees for {composed:?}"
        );
    }
}

#[test]
fn test_code_points_not_bytes_or_graphemes_except_reverse() {
    // \u{e9} is 1 code point, 2 bytes; flag emoji is 2 code points, 1 grapheme.
    assert_eq!(int(string::string_len(&[text("\u{e9}")]).unwrap()), 1);
    assert_eq!(int(string::string_byte_len(&[text("\u{e9}")]).unwrap()), 2);
    assert_eq!(
        int(string::string_len(&[text("\u{1f1e7}\u{1f1f7}")]).unwrap()),
        2
    );
    // Slicing is code-point based and tolerant.
    assert_eq!(
        rendered(
            string::string_slice(&[text("h\u{e9}llo"), Value::Int(1), Value::Int(3)]).unwrap()
        ),
        "\u{e9}l"
    );
}

#[test]
fn test_grapheme_reverse_keeps_clusters() {
    // Woman technologist is ZWJ-joined: reversing must not split the cluster.
    let family = "\u{1f469}\u{200d}\u{1f4bb}";
    let reversed = rendered(string::string_reverse(&[text(family)]).unwrap());
    assert_eq!(reversed, family);
    // Combining mark stays with its base.
    assert_eq!(
        rendered(string::string_reverse(&[text("ab\u{301}")]).unwrap()),
        "b\u{301}a"
    );
}

#[test]
fn test_case_mapping_is_deterministic() {
    assert_eq!(
        rendered(string::string_upper(&[text("\u{df}")]).unwrap()),
        "SS"
    );
    assert_eq!(
        rendered(string::string_lower(&[text("\u{130}")]).unwrap()),
        "i\u{307}"
    );
    assert_eq!(
        rendered(string::string_capitalize(&[text("hello WORLD")]).unwrap()),
        "Hello world"
    );
}

#[test]
fn test_extreme_combining_runs_terminate() {
    let heavy = format!("e{}", "\u{301}".repeat(500));
    let value = text(&heavy);
    let _ = string::string_len(std::slice::from_ref(&value)).unwrap();
    let _ = string::string_reverse(std::slice::from_ref(&value)).unwrap();
    let _ = string::string_upper(std::slice::from_ref(&value)).unwrap();
    let _ = string::string_lower(std::slice::from_ref(&value)).unwrap();
    let _ = string::string_slice(&[value.clone(), Value::Int(0), Value::Int(3)]).unwrap();
}

#[test]
fn test_mixed_scripts_preserved() {
    let mixed = "hi-\u{4e2d}-\u{3b1}-\u{1f389}";
    assert_eq!(int(string::string_len(&[text(mixed)]).unwrap()), 8);
    assert_eq!(
        rendered(string::string_trim(&[text("  x  ")]).unwrap()),
        "x"
    );
    assert_eq!(
        rendered(string::string_replace(&[text("aaa"), text("a"), text("b")]).unwrap()),
        "bbb"
    );
}

#[test]
fn test_empty_needle_rules() {
    assert_eq!(
        string::string_contains(&[text("abc"), text("")]).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        int(string::string_find(&[text("abc"), text("")]).unwrap()),
        0
    );
    assert!(string::string_split(&[text("abc"), text("")]).is_err());
    assert!(string::string_replace(&[text("abc"), text(""), text("x")]).is_err());
}

#[test]
fn test_join_rejects_coercion() {
    let list = Value::List(Rc::new(std::cell::RefCell::new(vec![Value::Int(1)].into())));
    assert!(string::string_join(&[text(","), list]).is_err());
}

#[test]
fn test_format_template_errors_are_recoverable() {
    let dict = |pairs: Vec<(&str, Value)>| {
        Value::Dict(Rc::new(std::cell::RefCell::new(
            pairs
                .into_iter()
                .map(|(k, v)| (text(k), v))
                .collect::<aipo_vm::DictMap>(),
        )))
    };
    let missing = string::string_format(&[text("hi {name}"), dict(vec![])]).unwrap();
    assert!(matches!(missing, Value::Failure(_)));
    let malformed = string::string_format(&[text("hi {name"), dict(vec![])]).unwrap();
    assert!(matches!(malformed, Value::Failure(_)));
    let ok =
        string::string_format(&[text("hi {name}!"), dict(vec![("name", text("ana"))])]).unwrap();
    assert_eq!(rendered(ok), "hi ana!");
}
