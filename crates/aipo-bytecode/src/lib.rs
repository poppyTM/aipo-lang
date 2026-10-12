//! `aipo-bytecode` defines the binary module format, instruction set,
//! bytecode emitter, structural verifier, and disassembler for Aipo.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod disasm;
pub mod emitter;
pub mod execution_plan;
pub use execution_plan::{ExecutionPlan, compile_execution_plan};
pub mod instruction;
pub mod linker;
pub mod module;
pub use linker::append_unit;
pub mod opcode;
pub mod reg_emitter;
pub mod reg_verifier;
pub mod verifier;

pub use disasm::{disassemble, disassemble_with_source};
pub use emitter::BytecodeEmitter;
pub use instruction::{RegInstruction, RegOpCode};
pub use module::{AIBC_MAGIC, AIBC_VERSION, BytecodeModule, FunctionInfo, StructInfo};
pub use opcode::{Constant, OpCode};
pub use reg_emitter::{RegCompiledFunction, RegCompiledModule, RegContract, RegEmitter};
pub use reg_verifier::RegVerifier;
pub use verifier::BytecodeVerifier;

use aipo_ir::CoreModule;

/// Compiles a `CoreModule` into a verified `BytecodeModule`.
///
/// # Errors
/// Returns verification error strings if the emitted bytecode violates structural invariants.
pub fn compile(module: &CoreModule) -> Result<BytecodeModule, Vec<String>> {
    let emitter = BytecodeEmitter::new();
    let bc_module = emitter.emit(module)?;
    BytecodeVerifier::verify(&bc_module)?;
    Ok(bc_module)
}

#[cfg(test)]
mod tests {
    use super::*;
    use aipo_hir::lower;
    use aipo_ir::{CoreFunction, CoreInst, lower_to_ir};
    use aipo_source::{Source, SourceId};
    use aipo_syntax::parse;

    #[test]
    fn test_compile_verify_and_disassemble() {
        let src = Source::new(
            SourceId::next(),
            "test.aipo",
            "let a = 10\nlet b = 20\nlet sum = a + b",
        );
        let (ast, diags) = parse(&src);
        assert!(diags.is_empty());
        let hir = lower(ast);
        let ir = lower_to_ir(&hir);

        let bc = compile(&ir).expect("compilation and verification must succeed");
        assert_eq!(bc.magic, AIBC_MAGIC);
        assert_eq!(bc.version, AIBC_VERSION);
        assert_eq!(bc.constants.len(), 2);
        assert_eq!(bc.names.len(), 3);

        let disasm = disassemble(&bc);
        assert!(disasm.contains("== Disassembly =="));
        assert!(disasm.contains("OpConstant"));
        assert!(disasm.contains("OpSetGlobal"));
        assert!(disasm.contains("Add"));
    }

    #[test]
    fn test_compile_verify_and_disassemble_contracts_and_mutation_boundaries() {
        // The contract and mutation-boundary opcodes must survive the whole emitter -> verifier
        // -> disassembler round trip, because a wrong operand width is only visible there.
        let src = Source::new(
            SourceId::next(),
            "contracts.aipo",
            "struct Bounds {\n    lo\n    hi\n}\n\nBounds:init(lo, hi) {\n        self.lo = lo\n        self.hi = hi\n    }\n\nBounds:invariant {\n        self.lo < self.hi\n    }\n\nfn widen(var b: Bounds, by: Int) -> Int {\n    b.hi = b.hi + by\n    return b.hi\n}\n\nlet r = Bounds{lo = 1, hi = 5}\nwiden(r, 2)\n",
        );
        let (ast, diags) = parse(&src);
        assert!(diags.is_empty(), "fixture must parse cleanly: {diags:?}");
        let hir = lower(ast);
        let ir = lower_to_ir(&hir);

        let bc = compile(&ir).expect("compilation and verification must succeed");
        let disasm = disassemble(&bc);
        assert!(disasm.contains("OpAssertContract parameter `b`: Bounds"));
        assert!(disasm.contains("OpAssertContract parameter `by`: Int"));
        assert!(disasm.contains("OpAssertContract return: Int"));
        assert!(disasm.contains("OpCheckMutations"));
        assert!(disasm.contains("OpAssertInvariant Bounds"));
    }

    #[test]
    fn test_verifier_detects_bad_magic() {
        let mut bc = BytecodeModule::new();
        bc.magic = *b"NOPE";
        let res = BytecodeVerifier::verify(&bc);
        assert!(res.is_err());
        assert!(res.unwrap_err()[0].contains("invalid magic header"));
    }

    #[test]
    fn test_verifier_detects_out_of_bounds_jump() {
        let mut bc = BytecodeModule::new();
        // OpJump with large positive offset past end of code
        bc.code = vec![OpCode::Jump as u8, 0x01, 0x00];
        let res = BytecodeVerifier::verify(&bc);
        assert!(res.is_err());
        assert!(res.unwrap_err()[0].contains("targets out-of-bounds address"));
    }

    #[test]
    fn test_verifier_detects_invalid_opcode() {
        let mut bc = BytecodeModule::new();
        bc.code = vec![0xFF]; // Non-existent opcode
        let res = BytecodeVerifier::verify(&bc);
        assert!(res.is_err());
        assert!(res.unwrap_err()[0].contains("invalid opcode byte"));
    }

    fn empty_module() -> CoreModule {
        CoreModule {
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
        }
    }

    #[test]
    fn test_emitter_rejects_oversized_list_literal() {
        let mut module = empty_module();
        module.top_level.instructions.push(CoreInst::BuildList(
            u16::MAX as usize + 1,
            aipo_source::SourceSpan::empty(0),
        ));
        let res = compile(&module);
        assert!(res.is_err());
        assert!(
            res.unwrap_err()[0].contains("exceeds 16-bit bytecode limit"),
            "oversized literal is a compile-time error, not silent truncation"
        );
    }

    #[test]
    fn test_emitter_rejects_unknown_function() {
        let mut module = empty_module();
        module.top_level.instructions.push(CoreInst::MakeFunction(
            "missing".to_string(),
            aipo_source::SourceSpan::empty(0),
        ));
        let res = compile(&module);
        assert!(res.is_err());
        assert!(res.unwrap_err()[0].contains("unknown function 'missing'"));
    }

    #[test]
    fn test_emitter_rejects_negative_jump_target() {
        let mut module = empty_module();
        module
            .top_level
            .instructions
            .push(CoreInst::Jump(-1, aipo_source::SourceSpan::empty(0)));
        let res = compile(&module);
        assert!(res.is_err());
        assert!(res.unwrap_err()[0].contains("is negative"));
    }

    #[test]
    fn test_binary_serialization_roundtrip() {
        let src = Source::new(
            SourceId::next(),
            "test.aipo",
            "struct Point {\n    x\n    y\n}\n\nfn add(a: Int, b: Int) -> Int {\n    return a + b\n}\n\nlet p = Point{x = 1, y = 2}\nadd(p.x, p.y)\n",
        );
        let (ast, diags) = parse(&src);
        assert!(diags.is_empty(), "fixture must parse cleanly: {diags:?}");
        let hir = lower(ast);
        let ir = lower_to_ir(&hir);
        let bc = compile(&ir).expect("compilation must succeed");

        // Serialize to bytes
        let bytes = bc.to_bytes();
        assert!(bytes.starts_with(b"AIBC"));

        // Deserialize from bytes
        let roundtrip = BytecodeModule::from_bytes(&bytes).expect("deserialization must succeed");
        assert_eq!(bc, roundtrip);

        // Verification of deserialized module succeeds
        BytecodeVerifier::verify(&roundtrip).expect("roundtrip module must be valid bytecode");
    }

    #[test]
    fn test_binary_deserialization_rejects_bad_magic_and_version() {
        let mut bytes = BytecodeModule::new().to_bytes();
        bytes[0] = b'X';
        assert!(BytecodeModule::from_bytes(&bytes).is_err());

        let mut bytes = BytecodeModule::new().to_bytes();
        bytes[4] = 0xFF; // bad version
        assert!(BytecodeModule::from_bytes(&bytes).is_err());
    }

    #[test]
    fn test_disassemble_with_source_annotates_lines() {
        let src = Source::new(
            SourceId::next(),
            "sample.aipo",
            "let a = 1\nlet b = 2\nlet c = a + b\n",
        );
        let (ast, diags) = parse(&src);
        assert!(diags.is_empty());
        let hir = lower(ast);
        let ir = lower_to_ir(&hir);
        let bc = compile(&ir).expect("compilation must succeed");

        let text = disassemble_with_source(&bc, &src);
        assert!(text.contains("== Disassembly =="));
        // Verify line annotation [line:col] is present for instructions
        assert!(text.contains("[1:"));
        assert!(text.contains("OpConstant"));
    }
}
