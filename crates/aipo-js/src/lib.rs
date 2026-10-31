//! `aipo-js` is the Wave 2 JavaScript backend for Aipo.
//!
//! The emitter consumes target-neutral Core IR (`aipo-ir`) and produces an ESM
//! bundle: `app.js` (the program as data + a call into the shim),
//! `aipo-runtime.js` (the versioned semantics shim) and `app.js.map`
//! (a valid ECMA-426 source map back to the entry `.aipo` file).
//!
//! Contract (see `docs/crates/crate-contracts.md` and
//! `docs/waves/wave-2-js-parity.md`):
//!
//! - depends on `aipo-ir` + `aipo-diagnostics` only — bytecode coupling is forbidden;
//! - output is ESM with mandatory source maps;
//! - the shim carries only unavoidable semantic deltas; divergence between the VM
//!   and JS backends is a bug.
//!
//! The host-only `fs` capability surface is not implemented or advertised by this target. In
//! particular, JavaScript/Web does not announce `filesystem.read` or `filesystem.roots` and has
//! no fallback that pretends to provide them.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use aipo_ir::{CoreConstant, CoreFunction, CoreInst, CoreModule};
use serde_json::{Value as Json, json};

/// Version of the emitted runtime shim.
///
/// Bumped whenever `runtime/aipo-runtime.js` semantics change; the emitted
/// `app.js` records it so a stale shim is detectable.
pub const RUNTIME_VERSION: &str = "1.2.1";

/// The versioned runtime shim source, embedded at compile time.
pub const RUNTIME_JS: &str = include_str!("../runtime/aipo-runtime.js");

/// An emitted JavaScript bundle.
pub struct JsBundle {
    /// `app.js` — ESM entry point importing `./aipo-runtime.js`.
    pub app_js: String,
    /// `aipo-runtime.js` — the versioned shim.
    pub runtime_js: String,
    /// `app.js.map` — ECMA-426 source map JSON.
    pub source_map: String,
}

/// Emits a JavaScript bundle for a lowered Core IR module.
///
/// `source_name` is the entry file name recorded in the source map (for example
/// `main.aipo`); `source_text` is its content, used to size line mappings.
#[must_use]
pub fn emit_js(source_name: &str, source_text: &str, module: &CoreModule) -> JsBundle {
    let module_json = module_to_json(module);
    let module_str = serde_json::to_string(&module_json).unwrap_or_else(|_| "{}".to_string());
    // The entry grants the `clock` capability by installing the system clock, mirroring the
    // CLI profile that grants it for `aipo run`. A host that needs determinism installs its own
    // source on `globalThis.__aipoClock` before this module evaluates, and
    // `installDefaultClock` then leaves it alone — that preload is the deterministic test
    // profile, and it is the only reason a replay reads the same clock on both backends.
    let app_js = format!(
        "import {{ runModule, installDefaultClock }} from './aipo-runtime.js';\n// aipo-js runtime v{RUNTIME_VERSION}\ninstallDefaultClock();\nconst MODULE = {module_str};\nconst __code = runModule(MODULE);\nif (typeof process !== 'undefined' && __code) process.exit(__code);\n//# sourceMappingURL=app.js.map\n"
    );
    let source_map = build_source_map(source_name, source_text, &app_js);
    JsBundle {
        app_js,
        runtime_js: RUNTIME_JS.to_string(),
        source_map,
    }
}

/// Serializes a Core IR module into the JSON the shim interprets.
fn module_to_json(module: &CoreModule) -> Json {
    let functions: Vec<Json> = module.functions.iter().map(function_to_json).collect();
    let structs: Vec<Json> = module
        .structs
        .iter()
        .map(|s| {
            json!({
                "name": s.name,
                "fields": s.fields.iter().map(|(n, f)| json!([n, f])).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({
        "version": RUNTIME_VERSION,
        "functions": functions,
        "top": function_to_json(&module.top_level),
        "structs": structs,
    })
}

fn function_to_json(func: &CoreFunction) -> Json {
    let code: Vec<Json> = func.instructions.iter().map(inst_to_json).collect();
    json!({
        "name": func.name,
        // `async` is call protocol, not syntax: the shim must know which callees
        // produce a `Task` instead of running, exactly like the VM's function table.
        "async": func.is_async,
        "params": func.params,
        "locals": func.locals,
        "upvalues": func.upvalues,
        "code": code,
    })
}

/// Maps one Core IR instruction to its compact JSON form.
fn inst_to_json(inst: &CoreInst) -> Json {
    match inst {
        CoreInst::Constant(c, _) => json!({"op":"Constant","value": constant_to_json(c)}),
        CoreInst::Load(n, _) => json!({"op":"Load","name": n}),
        CoreInst::Store(n, _) => json!({"op":"Store","name": n}),
        CoreInst::GetUpvalue(n, _) => json!({"op":"GetUpvalue","name": n}),
        CoreInst::SetUpvalue(n, _) => json!({"op":"SetUpvalue","name": n}),
        // A captured mutable local lives in one shared cell so the closure's writes and
        // the enclosing frame's reads reach the same storage.
        CoreInst::BoxLocal(n, _) => json!({"op":"BoxLocal","name": n}),
        CoreInst::Binary(op, _) => {
            json!({"op":"Binary","op2": serde_json::to_value(op).unwrap_or(Json::Null)})
        }
        CoreInst::Unary(op, _) => {
            json!({"op":"Unary","op2": serde_json::to_value(op).unwrap_or(Json::Null)})
        }
        CoreInst::Call { arg_count, .. } => json!({"op":"Call","argc": arg_count}),
        CoreInst::Await(_) => json!({"op":"Await"}),
        CoreInst::Return { has_value, .. } => json!({"op":"Return","has": has_value}),
        CoreInst::Jump(t, _) => json!({"op":"Jump","t": target_usize(*t)}),
        CoreInst::JumpIfFalse(t, _) => json!({"op":"JumpIfFalse","t": target_usize(*t)}),
        CoreInst::Pop(_) => json!({"op":"Pop"}),
        CoreInst::Dup(_) => json!({"op":"Dup"}),
        CoreInst::CloneStruct(_) => json!({"op":"CloneStruct"}),
        CoreInst::GetField(f, _) => json!({"op":"GetField","f": f}),
        CoreInst::SetField(f, _) => json!({"op":"SetField","f": f}),
        CoreInst::GetIndex(_) => json!({"op":"GetIndex"}),
        CoreInst::SetIndex(_) => json!({"op":"SetIndex"}),
        CoreInst::IsVariant(v, _) => json!({"op":"IsVariant","variant": v}),
        CoreInst::BuildList(n, _) => json!({"op":"BuildList","n": n}),
        CoreInst::BuildDict(n, _) => json!({"op":"BuildDict","n": n}),
        CoreInst::BuildStruct {
            type_name,
            field_count,
            defer_fixed,
            ..
        } => json!({"op":"BuildStruct","type": type_name, "n": field_count, "defer": defer_fixed}),
        CoreInst::SealStruct(_) => json!({"op":"SealStruct"}),
        CoreInst::AssertInvariant { type_name, .. } => {
            json!({"op":"AssertInvariant","type": type_name})
        }
        CoreInst::AssertContract {
            type_name,
            nullable,
            position,
            operations,
            ..
        } => {
            json!({"op":"AssertContract","type": type_name, "null": nullable, "pos": position, "ops": operations.iter().map(|(n, a)| json!([n, a])).collect::<Vec<_>>()})
        }
        CoreInst::CheckMutations(_) => json!({"op":"CheckMutations"}),
        CoreInst::PropagateFailure(_) => json!({"op":"PropagateFailure"}),
        CoreInst::MakeFunction(n, _) => json!({"op":"MakeFunction","name": n}),
        CoreInst::MakeClosure {
            name,
            upvalues,
            self_capture,
            ..
        } => json!({"op":"MakeClosure","name": name, "ups": upvalues, "self": self_capture}),
        CoreInst::FillSelfCapture { name, .. } => json!({"op":"FillSelfCapture","name": name}),
        CoreInst::Range(_) => json!({"op":"Range"}),
        CoreInst::Len(_) => json!({"op":"Len"}),
        CoreInst::TypeIs(_) => json!({"op":"TypeIs"}),
        CoreInst::TypeIsNullable(_) => json!({"op":"TypeIsNullable"}),
        CoreInst::IterGuard(_) => json!({"op":"IterGuard"}),
        CoreInst::IterAt(mode, _) => json!({
            "op":"IterAt",
            "mode": match mode {
                aipo_ir::IterMode::Primary => 0,
                aipo_ir::IterMode::Key => 1,
                aipo_ir::IterMode::Value => 2,
            }
        }),
        CoreInst::IterGuardEnd(_) => json!({"op":"IterGuardEnd"}),
        CoreInst::Fail(_) => json!({"op":"Fail"}),
        CoreInst::PushHandler(t, _) => json!({"op":"PushHandler","t": target_usize(*t)}),
        CoreInst::PopHandler(_) => json!({"op":"PopHandler"}),
        CoreInst::PushUnset(_) => json!({"op":"PushUnset"}),
        CoreInst::JumpIfSetLocal { slot, target, .. } => {
            json!({"op":"JumpIfSetLocal","slot": slot, "t": target_usize(*target)})
        }
    }
}

/// Serializes a constant for the shim.
///
/// `serde_json` writes a non-finite float as `null`, which would erase the reason the value
/// is unusable. The wire form stays `null` (the shim already reports
/// `AIPO_RT_NON_FINITE_FLOAT` for it, matching the VM's fault for the same constant), but the
/// intent is stated here instead of resting on the serializer's default.
fn constant_to_json(value: &CoreConstant) -> Json {
    match value {
        CoreConstant::Float(f) if !f.is_finite() => Json::Null,
        other => serde_json::to_value(other).unwrap_or(Json::Null),
    }
}

fn target_usize(raw: isize) -> usize {
    if raw < 0 { 0 } else { raw as usize }
}

/// Builds a minimal valid ECMA-426 source map mapping each emitted line to a source line.
fn build_source_map(source_name: &str, source_text: &str, app_js: &str) -> String {
    let src_lines = source_text.lines().count().max(1) as u32;
    let out_lines = app_js.lines().count().max(1);
    let mut mappings = String::new();
    for (i, _) in app_js.lines().enumerate() {
        if i > 0 {
            mappings.push(';');
        }
        let src_line = (i as u32).min(src_lines - 1);
        mappings.push_str(&encode_segment(src_line));
    }
    let _ = out_lines;
    serde_json::to_string(&json!({
        "version": 3,
        "file": "app.js",
        "sources": [source_name],
        "sourcesContent": [source_text],
        "names": [],
        "mappings": mappings,
    }))
    .unwrap_or_else(|_| "{\"version\":3}".to_string())
}

/// Encodes one mapping segment `genCol=0, src=0, srcLine, srcCol=0` as VLQ.
fn encode_segment(src_line: u32) -> String {
    let mut out = String::new();
    encode_vlq(&mut out, 0);
    encode_vlq(&mut out, 0);
    encode_vlq(&mut out, src_line as i64);
    encode_vlq(&mut out, 0);
    out
}

fn encode_vlq(out: &mut String, mut value: i64) {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut v = if value < 0 {
        ((-value) << 1) | 1
    } else {
        value << 1
    };
    value = v;
    loop {
        v = value >> 5;
        let mut digit = (value & 31) as usize;
        value = v;
        if value > 0 {
            digit |= 32;
        }
        out.push(CHARS[digit] as char);
        if value == 0 {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emit_produces_esm_wrapper_and_valid_map() {
        let module = CoreModule {
            functions: vec![],
            top_level: CoreFunction {
                name: "__top_level__".to_string(),
                is_async: false,
                params: vec![],
                locals: vec![],
                upvalues: vec![],
                instructions: vec![],
                span: aipo_source::SourceSpan::empty(0),
            },
            structs: vec![],
            span: aipo_source::SourceSpan::empty(0),
        };
        let bundle = emit_js("main.aipo", "io.println(1)\n", &module);
        assert!(bundle.app_js.contains("runModule"));
        assert!(bundle.app_js.contains("sourceMappingURL"));
        let map: Json = serde_json::from_str(&bundle.source_map).expect("map is JSON");
        assert_eq!(map["version"], 3);
        assert_eq!(map["sources"][0], "main.aipo");
    }
}
