//! Explicit selection between native register execution and canonical semantics.
use crate::{BytecodeModule, RegCompiledModule, RegEmitter};
use aipo_ir::{CoreInst, CoreModule};
use serde::{Deserialize, Serialize};

/// A compilation plan. Canonical execution retains full language semantics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ExecutionPlan {
    /// Verified native register bytecode.
    Register(Box<RegCompiledModule>),
    /// Verified canonical bytecode, with the reason for selecting it.
    Canonical {
        /// Stack bytecode with methods, captures, scheduler and contract hooks.
        module: Box<BytecodeModule>,
        /// The capability absent from the native register engine.
        reason: String,
    },
}
impl ExecutionPlan {
    /// Returns the actual execution engine, suitable for diagnostics and tooling.
    pub fn engine(&self) -> &'static str {
        match self {
            Self::Register(_) => "reg",
            Self::Canonical { .. } => "vm",
        }
    }
}
/// Compiles for the register profile, selecting the canonical engine when necessary.
/// This never drops IR or disguises a native-emitter error as successful Reg bytecode.
///
/// # Errors
/// Returns canonical bytecode verification errors if neither plan can be generated.
pub fn compile_execution_plan(core: &CoreModule) -> Result<ExecutionPlan, Vec<String>> {
    let shared_services = !core.structs.is_empty()
        || core
            .functions
            .iter()
            .chain(std::iter::once(&core.top_level))
            .any(|function| {
                function
                    .instructions
                    .iter()
                    .any(|inst| matches!(inst, CoreInst::GetField(..) | CoreInst::SetField(..)))
            });
    let reason = if shared_services {
        "member access and construction use the canonical method and contract services".into()
    } else {
        match RegEmitter::new().compile_module(core) {
            Ok(module) => return Ok(ExecutionPlan::Register(Box::new(module))),
            Err(reason) => reason,
        }
    };
    crate::compile(core).map(|module| ExecutionPlan::Canonical {
        module: Box::new(module),
        reason,
    })
}
