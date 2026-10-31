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
use serde_json::json;

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
    let mut writer = ModuleWriter {
        module,
        out: String::with_capacity(module.functions.len() * 512),
    };
    writer.write();
    let module_str = writer.out;
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
///
/// The wire form is the shim's contract, which is not the IR's own `Serialize`
/// shape: field names are abbreviated (`op`, `t`, `n`, `f`) and some values are
/// flattened. It is written directly into the output buffer instead of building a
/// `serde_json::Value` tree, which would allocate a map and a `String` key per
/// instruction — the dominant cost of emitting a large module.
struct ModuleWriter<'a> {
    module: &'a CoreModule,
    out: String,
}

impl<'a> ModuleWriter<'a> {
    fn write(&mut self) {
        self.out.push_str("{\"version\":");
        write_json_string(&mut self.out, RUNTIME_VERSION);
        self.out.push_str(",\"functions\":[");
        for (index, function) in self.module.functions.iter().enumerate() {
            if index > 0 {
                self.out.push(',');
            }
            self.write_function(function);
        }
        self.out.push_str("],\"top\":");
        self.write_function(&self.module.top_level);
        self.out.push_str(",\"structs\":[");
        for (index, s) in self.module.structs.iter().enumerate() {
            if index > 0 {
                self.out.push(',');
            }
            self.out.push_str("{\"name\":");
            write_json_string(&mut self.out, &s.name);
            self.out.push_str(",\"fields\":[");
            for (field_index, (name, field)) in s.fields.iter().enumerate() {
                if field_index > 0 {
                    self.out.push(',');
                }
                self.out.push('[');
                write_json_string(&mut self.out, name);
                self.out.push(',');
                self.out
                    .push_str(&serde_json::to_string(field).unwrap_or_else(|_| "null".into()));
                self.out.push(']');
            }
            self.out.push_str("]}");
        }
        self.out.push_str("]}");
    }

    fn write_function(&mut self, func: &CoreFunction) {
        self.out.push_str("{\"name\":");
        write_json_string(&mut self.out, &func.name);
        self.out.push_str(",\"async\":");
        self.out
            .push_str(if func.is_async { "true" } else { "false" });
        self.out.push_str(",\"params\":[");
        for (index, param) in func.params.iter().enumerate() {
            if index > 0 {
                self.out.push(',');
            }
            write_json_string(&mut self.out, param);
        }
        self.out.push_str("],\"locals\":[");
        for (index, local) in func.locals.iter().enumerate() {
            if index > 0 {
                self.out.push(',');
            }
            write_json_string(&mut self.out, local);
        }
        self.out.push_str("],\"upvalues\":[");
        for (index, upvalue) in func.upvalues.iter().enumerate() {
            if index > 0 {
                self.out.push(',');
            }
            write_json_string(&mut self.out, upvalue);
        }
        self.out.push_str("],\"code\":[");
        for (index, inst) in func.instructions.iter().enumerate() {
            if index > 0 {
                self.out.push(',');
            }
            self.write_inst(inst);
        }
        self.out.push_str("]}");
    }

    fn write_inst(&mut self, inst: &CoreInst) {
        let out = &mut self.out;
        out.push_str("{\"op\":");
        let op = match inst {
            CoreInst::Constant(..) => "Constant",
            CoreInst::Load(..) => "Load",
            CoreInst::Store(..) => "Store",
            CoreInst::GetUpvalue(..) => "GetUpvalue",
            CoreInst::SetUpvalue(..) => "SetUpvalue",
            CoreInst::BoxLocal(..) => "BoxLocal",
            CoreInst::Binary(..) => "Binary",
            CoreInst::Unary(..) => "Unary",
            CoreInst::Call { .. } => "Call",
            CoreInst::Await(_) => "Await",
            CoreInst::Return { .. } => "Return",
            CoreInst::Jump(..) => "Jump",
            CoreInst::JumpIfFalse(..) => "JumpIfFalse",
            CoreInst::Pop(_) => "Pop",
            CoreInst::Dup(_) => "Dup",
            CoreInst::CloneStruct(_) => "CloneStruct",
            CoreInst::GetField(..) => "GetField",
            CoreInst::SetField(..) => "SetField",
            CoreInst::GetIndex(_) => "GetIndex",
            CoreInst::SetIndex(_) => "SetIndex",
            CoreInst::IsVariant(..) => "IsVariant",
            CoreInst::BuildList(..) => "BuildList",
            CoreInst::BuildDict(..) => "BuildDict",
            CoreInst::BuildStruct { .. } => "BuildStruct",
            CoreInst::SealStruct(_) => "SealStruct",
            CoreInst::AssertInvariant { .. } => "AssertInvariant",
            CoreInst::AssertContract { .. } => "AssertContract",
            CoreInst::CheckMutations(_) => "CheckMutations",
            CoreInst::PropagateFailure(_) => "PropagateFailure",
            CoreInst::MakeFunction(..) => "MakeFunction",
            CoreInst::MakeClosure { .. } => "MakeClosure",
            CoreInst::FillSelfCapture { .. } => "FillSelfCapture",
            CoreInst::Range(_) => "Range",
            CoreInst::Len(_) => "Len",
            CoreInst::TypeIs(_) => "TypeIs",
            CoreInst::TypeIsNullable(_) => "TypeIsNullable",
            CoreInst::IterGuard(_) => "IterGuard",
            CoreInst::IterAt(..) => "IterAt",
            CoreInst::IterGuardEnd(_) => "IterGuardEnd",
            CoreInst::Fail(_) => "Fail",
            CoreInst::PushHandler(..) => "PushHandler",
            CoreInst::PopHandler(_) => "PopHandler",
            CoreInst::PushUnset(_) => "PushUnset",
            CoreInst::JumpIfSetLocal { .. } => "JumpIfSetLocal",
        };
        write_json_string(out, op);
        match inst {
            CoreInst::Constant(value, _) => {
                out.push_str(",\"value\":");
                let rendered = constant_to_wire(value);
                out.push_str(&rendered);
            }
            CoreInst::Load(name, _)
            | CoreInst::Store(name, _)
            | CoreInst::GetUpvalue(name, _)
            | CoreInst::SetUpvalue(name, _)
            | CoreInst::BoxLocal(name, _)
            | CoreInst::MakeFunction(name, _)
            | CoreInst::FillSelfCapture { name, .. } => {
                out.push_str(",\"name\":");
                write_json_string(out, name);
            }
            CoreInst::Binary(op_kind, _) => write_op_field(out, op_kind),
            CoreInst::Unary(op_kind, _) => write_op_field(out, op_kind),
            CoreInst::Call { arg_count, .. } => {
                out.push_str(",\"argc\":");
                push_usize(out, *arg_count);
            }
            CoreInst::Return { has_value, .. } => {
                out.push_str(",\"has\":");
                out.push_str(if *has_value { "true" } else { "false" });
            }
            CoreInst::Jump(target, _) | CoreInst::JumpIfFalse(target, _) => {
                out.push_str(",\"t\":");
                push_isize(out, *target);
            }
            CoreInst::GetField(field, _) | CoreInst::SetField(field, _) => {
                out.push_str(",\"f\":");
                write_json_string(out, field);
            }
            CoreInst::IsVariant(variant, _) => {
                out.push_str(",\"variant\":");
                write_json_string(out, variant);
            }
            CoreInst::BuildList(count, _) | CoreInst::BuildDict(count, _) => {
                out.push_str(",\"n\":");
                push_usize(out, *count);
            }
            CoreInst::BuildStruct {
                type_name,
                field_count,
                defer_fixed,
                ..
            } => {
                out.push_str(",\"type\":");
                write_json_string(out, type_name);
                out.push_str(",\"n\":");
                push_usize(out, *field_count);
                out.push_str(",\"defer\":");
                out.push_str(if *defer_fixed { "true" } else { "false" });
            }
            CoreInst::AssertInvariant { type_name, .. } => {
                out.push_str(",\"type\":");
                write_json_string(out, type_name);
            }
            CoreInst::AssertContract {
                type_name,
                nullable,
                position,
                operations,
                ..
            } => {
                out.push_str(",\"type\":");
                write_json_string(out, type_name);
                out.push_str(",\"null\":");
                out.push_str(if *nullable { "true" } else { "false" });
                out.push_str(",\"pos\":");
                write_json_string(out, position);
                out.push_str(",\"ops\":[");
                for (index, (name, arity)) in operations.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    out.push('[');
                    write_json_string(out, name);
                    out.push(',');
                    push_usize(out, *arity);
                    out.push(']');
                }
                out.push(']');
            }
            CoreInst::MakeClosure {
                name,
                upvalues,
                self_capture,
                ..
            } => {
                out.push_str(",\"name\":");
                write_json_string(out, name);
                out.push_str(",\"ups\":[");
                for (index, upvalue) in upvalues.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write_json_string(out, upvalue);
                }
                out.push_str("],\"self\":");
                match self_capture {
                    Some(capture) => write_json_string(out, capture),
                    None => out.push_str("null"),
                }
            }
            CoreInst::IterAt(mode, _) => {
                out.push_str(",\"mode\":");
                out.push_str(match mode {
                    aipo_ir::IterMode::Primary => "0",
                    aipo_ir::IterMode::Key => "1",
                    aipo_ir::IterMode::Value => "2",
                });
            }
            CoreInst::PushHandler(target, _) => {
                out.push_str(",\"t\":");
                push_isize(out, *target);
            }
            CoreInst::JumpIfSetLocal { slot, target, .. } => {
                out.push_str(",\"slot\":");
                push_usize(out, *slot);
                out.push_str(",\"t\":");
                push_isize(out, *target);
            }
            CoreInst::Await(_)
            | CoreInst::Pop(_)
            | CoreInst::Dup(_)
            | CoreInst::CloneStruct(_)
            | CoreInst::GetIndex(_)
            | CoreInst::SetIndex(_)
            | CoreInst::SealStruct(_)
            | CoreInst::CheckMutations(_)
            | CoreInst::PropagateFailure(_)
            | CoreInst::Range(_)
            | CoreInst::Len(_)
            | CoreInst::TypeIs(_)
            | CoreInst::TypeIsNullable(_)
            | CoreInst::IterGuard(_)
            | CoreInst::IterGuardEnd(_)
            | CoreInst::Fail(_)
            | CoreInst::PopHandler(_)
            | CoreInst::PushUnset(_) => {}
        }
        self.out.push('}');
    }
}

/// Writes a `serde`-derived operator as the wire `op2` field.
fn write_op_field<T: serde::Serialize>(out: &mut String, operator: &T) {
    out.push_str(",\"op2\":");
    out.push_str(&serde_json::to_string(operator).unwrap_or_else(|_| "null".into()));
}

fn push_usize(out: &mut String, value: usize) {
    use std::fmt::Write as _;
    let _ = write!(out, "{value}");
}

fn push_isize(out: &mut String, value: isize) {
    use std::fmt::Write as _;
    let _ = write!(out, "{}", target_usize(value));
}

/// Appends a JSON string literal, escaping only the characters JSON requires.
fn write_json_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if (ch as u32) < 0x20 => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", ch as u32);
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

/// Renders a constant for the wire, preserving the shim's `null` for a non-finite
/// float (see [`constant_to_json`]).
fn constant_to_wire(value: &CoreConstant) -> String {
    match value {
        CoreConstant::Float(f) if !f.is_finite() => "null".to_string(),
        other => serde_json::to_string(other).unwrap_or_else(|_| "null".into()),
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
    use serde_json::Value as Json;

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
