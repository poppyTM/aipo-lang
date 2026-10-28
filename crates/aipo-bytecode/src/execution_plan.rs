//! Explicit selection between native register execution and canonical semantics.
use crate::{BytecodeModule, RegCompiledModule, RegEmitter};
use aipo_ir::CoreModule;
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
    const HIGHER_ORDER_METHODS: &[&str] = &[
        "filter",
        "transform",
        "map",
        "sort_by",
        "any",
        "all",
        "flat_map",
        "reduce",
    ];
    let uses_higher_order = core
        .functions
        .iter()
        .chain(std::iter::once(&core.top_level))
        .any(|function| {
            function.instructions.iter().any(|inst| {
                matches!(
                    inst,
                    aipo_ir::CoreInst::GetField(name, _)
                        if HIGHER_ORDER_METHODS.contains(&name.as_str())
                )
            })
        });
    let uses_failure_control = core
        .functions
        .iter()
        .chain(std::iter::once(&core.top_level))
        .any(|function| {
            function
                .instructions
                .iter()
                .any(|inst| matches!(inst, aipo_ir::CoreInst::PushHandler(..)))
        });
    let reason = if uses_higher_order {
        "higher-order collection methods require --engine=vm".into()
    } else if uses_failure_control {
        "failure handlers require --engine=vm".into()
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
