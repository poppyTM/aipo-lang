//! Generates the Unicode tables the Wasm runtime embeds in every module.
//!
//! The runtime helpers (`__aipo_str_case`, `__aipo_nfc`, `__aipo_str_trim`) run
//! inside the emitted module and cannot call Rust, so the data they need is written
//! here as Rust arrays and copied into linear memory by `unicode.rs`.
//!
//! Every table is derived from the same sources the reference VM uses — `char`'s
//! own case mapping for `to_uppercase`/`to_lowercase`, and `unicode-normalization`
//! for canonical decomposition, combining classes and composition — so the Wasm
//! backend cannot drift from the VM's Unicode version.
//!
//! Output: `$OUT_DIR/unicode_tables.rs`.

use std::fmt::Write as _;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let mut out = String::with_capacity(1024 * 1024);

    emit_case_tables(&mut out);
    emit_whitespace(&mut out);
    emit_decomposition(&mut out);
    emit_combining_classes(&mut out);
    emit_composition(&mut out);

    let dest =
        std::path::Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR")).join("unicode_tables.rs");
    std::fs::write(&dest, out).expect("write unicode_tables.rs");
}

/// Writes a `&[(u32, u32)]` table sorted by its first element.
fn emit_pairs(out: &mut String, name: &str, rows: &[(u32, u32)]) {
    writeln!(out, "pub(crate) const {name}: &[(u32, u32)] = &[").unwrap();
    for (key, value) in rows {
        writeln!(out, "    ({key}, {value}),").unwrap();
    }
    writeln!(out, "];").unwrap();
}

/// Writes a `&[(u32, &[u32])]` table sorted by its first element.
fn emit_mappings(out: &mut String, name: &str, rows: &[(u32, Vec<u32>)]) {
    writeln!(out, "pub(crate) const {name}: &[(u32, &[u32])] = &[").unwrap();
    for (key, values) in rows {
        let values = values
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(out, "    ({key}, &[{values}]),").unwrap();
    }
    writeln!(out, "];").unwrap();
}

fn emit_case_tables(out: &mut String) {
    let mut upper_one = Vec::new();
    let mut upper_many = Vec::new();
    let mut lower_one = Vec::new();
    let mut lower_many = Vec::new();

    for code in 0..=0x10FFFFu32 {
        let Some(ch) = char::from_u32(code) else {
            continue;
        };

        let upper: Vec<u32> = ch.to_uppercase().map(u32::from).collect();
        if upper != [code] {
            if upper.len() == 1 {
                upper_one.push((code, upper[0]));
            } else {
                upper_many.push((code, upper));
            }
        }

        let lower: Vec<u32> = ch.to_lowercase().map(u32::from).collect();
        if lower != [code] {
            if lower.len() == 1 {
                lower_one.push((code, lower[0]));
            } else {
                lower_many.push((code, lower));
            }
        }
    }

    // A `&[u32]` expansion is copied into a fixed 3-slot record by the runtime; the
    // longest mapping in Unicode is three code points, and the generator asserts it
    // so a future Unicode version cannot silently truncate one.
    assert!(
        upper_many.iter().all(|(_, v)| v.len() <= 3),
        "uppercase expansion longer than 3"
    );
    assert!(
        lower_many.iter().all(|(_, v)| v.len() <= 3),
        "lowercase expansion longer than 3"
    );

    emit_pairs(out, "UPPER_ONE_TO_ONE", &upper_one);
    emit_mappings(out, "UPPER_EXPANSIONS", &upper_many);
    emit_pairs(out, "LOWER_ONE_TO_ONE", &lower_one);
    emit_mappings(out, "LOWER_EXPANSIONS", &lower_many);
}

fn emit_whitespace(out: &mut String) {
    let mut ranges: Vec<(u32, u32)> = Vec::new();
    for code in 0..=0x10FFFFu32 {
        let Some(ch) = char::from_u32(code) else {
            continue;
        };
        if !ch.is_whitespace() {
            continue;
        }
        match ranges.last_mut() {
            Some((_, end)) if *end + 1 == code => *end = code,
            _ => ranges.push((code, code)),
        }
    }
    emit_pairs(out, "WHITESPACE", &ranges);
}

fn emit_decomposition(out: &mut String) {
    let mut rows: Vec<(u32, Vec<u32>)> = Vec::new();
    for code in 0..=0x10FFFFu32 {
        let Some(ch) = char::from_u32(code) else {
            continue;
        };
        // Hangul syllables decompose algorithmically; listing 11 172 of them would
        // add ~134 KiB to every module for no reason.
        if (0xAC00..=0xD7A3).contains(&code) {
            continue;
        }
        let mut parts = Vec::new();
        unicode_normalization::char::decompose_canonical(ch, |part| {
            parts.push(u32::from(part));
        });
        if parts != [code] {
            rows.push((code, parts));
        }
    }
    assert!(
        rows.iter().all(|(_, parts)| parts.len() <= 4),
        "canonical decomposition longer than 4"
    );
    emit_mappings(out, "DECOMPOSITION", &rows);
}

fn emit_combining_classes(out: &mut String) {
    let mut rows: Vec<(u32, u32, u8)> = Vec::new();
    for code in 0..=0x10FFFFu32 {
        let Some(ch) = char::from_u32(code) else {
            continue;
        };
        let class = unicode_normalization::char::canonical_combining_class(ch);
        if class == 0 {
            continue;
        }
        match rows.last_mut() {
            Some((_, end, last)) if *last == class && *end + 1 == code => *end = code,
            _ => rows.push((code, code, class)),
        }
    }
    writeln!(
        out,
        "pub(crate) const COMBINING_CLASS: &[(u32, u32, u8)] = &["
    )
    .unwrap();
    for (start, end, class) in rows {
        writeln!(out, "    ({start}, {end}, {class}),").unwrap();
    }
    writeln!(out, "];").unwrap();
}

fn emit_composition(out: &mut String) {
    let mut rows: Vec<(u32, u32, u32)> = Vec::new();
    for code in 0..=0x10FFFFu32 {
        let Some(ch) = char::from_u32(code) else {
            continue;
        };
        let mut parts = Vec::new();
        unicode_normalization::char::decompose_canonical(ch, |part| parts.push(part));
        // A primary composite is exactly two code points that recompose to the same
        // character. Hangul is algorithmic in the runtime and is not listed here.
        if parts.len() != 2 {
            continue;
        }
        if (0xAC00..=0xD7A3).contains(&code) {
            continue;
        }
        if unicode_normalization::char::compose(parts[0], parts[1]) == Some(ch) {
            rows.push((u32::from(parts[0]), u32::from(parts[1]), code));
        }
    }
    rows.sort_unstable();
    writeln!(out, "pub(crate) const COMPOSITION: &[(u32, u32, u32)] = &[").unwrap();
    for (first, second, composed) in rows {
        writeln!(out, "    ({first}, {second}, {composed}),").unwrap();
    }
    writeln!(out, "];").unwrap();
}
