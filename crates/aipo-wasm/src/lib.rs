//! WebAssembly (Wasm 2.0 / WASI) binary module emitter and execution foundation for Aipo (ADP-013).

pub mod compiler;
pub mod emitter;
pub mod error;
pub mod runner;
pub mod types;

pub use compiler::compile_hir;
pub use emitter::WasmEmitter;
pub use error::WasmCompileError;
pub use runner::{WasmRuntimeError, disassemble_wasm, execute_wasm, execute_wasm_with_fuel};
pub use types::{WasmFnType, WasmType};
pub use wasm_encoder::Instruction;
