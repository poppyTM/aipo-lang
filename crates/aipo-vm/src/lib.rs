//! Aipo Virtual Machine: stack-based bytecode interpreter, call frames, value model, and runtime fault handling.

pub mod arena;
pub mod convert;
pub mod engine;
pub mod fault;
pub mod frame;
pub mod host;
pub mod reg_task;
pub mod reg_vm;
pub mod value;
pub mod vm;

pub use arena::{ArenaAllocator, ArenaError};
pub use convert::{
    BYTE_MAX, TypeTag, convert_byte, convert_bytes, convert_duration, convert_float, convert_int,
    convert_set, convert_string, convert_via_type,
};
pub use engine::{AipoEngine, AipoEngineBuilder, AipoScope, ScopedHandle};
pub use fault::{VmError, VmFault};
pub use frame::{CallFrame, HandlerFrame};
pub use host::{
    EnvironmentSource, FilesystemError, FilesystemSource, FilesystemSourceError, HostContext,
    host_fault_to_vm_fault, host_value_to_value, value_to_host_value,
};
pub use reg_task::{RegCall, RegScheduler, RegTaskOutcome, RegTaskStatus, RegWaitTarget};
pub use reg_vm::RegVm;
pub use value::{
    DictMap, FailureValue, GroupId, MAX_SAFE_INT, MIN_SAFE_INT, MethodKind, RangeData, SeqOp,
    SequencePipeline, SequenceSource, StructInstance, StructMethodData, TaskId, Value,
    check_finite_float, check_safe_int,
};
pub use vm::{HostNative, HostNativeCallback, HostNativeEntry, TestMode, Vm, VmMetrics};

use aipo_bytecode::BytecodeModule;

/// Convenience function to instantiate a fresh VM and execute a compiled module.
///
/// # Errors
/// Returns `VmError` if a runtime fault or uncaught failure occurs.
pub fn execute(module: &BytecodeModule) -> Result<Value, VmError> {
    let mut vm = Vm::new();
    vm.run(module)
}
