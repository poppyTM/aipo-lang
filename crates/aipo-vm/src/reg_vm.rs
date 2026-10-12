//! Virtual register execution engine for 32-bit instructions (Marco 3 / ADP-014).

use crate::arena::ArenaAllocator;
use crate::convert::{TypeTag, convert_via_type};
use crate::fault::VmFault;
use crate::reg_task::{RegCall, RegFrameSlot, RegScheduler, RegSuspendCapture, is_task_native};
use crate::value::{BoundMethodData, DictMap, MethodKind, StructInstance, Value};
use crate::vm::helpers::normalize_range;
use crate::vm::{MUTATING_METHODS, MethodNative};
use aipo_bytecode::{
    Constant, RegCompiledFunction, RegCompiledModule, RegContract, RegInstruction, RegOpCode,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// Maximum nested user-function call depth (bounds Rust stack + RAM).
pub const REG_MAX_CALL_DEPTH: usize = 64;

/// A saved caller activation for nested `RegVm` calls.
#[derive(Debug, Clone)]
pub struct RegCallFrame {
    /// Instruction offset to resume in the caller.
    pub return_pc: usize,
    /// Register receiving the callee result.
    pub return_reg: u8,
}

/// Compact virtual register machine for embedded execution and fast scripts.
#[derive(Debug)]
pub struct RegVm {
    /// 256 virtual registers (fixed array of 16-byte Values = 4 KiB total).
    pub registers: [Value; 256],
    /// Constant pool.
    pub constants: Vec<Value>,
    /// Global variables environment.
    pub globals: HashMap<String, Value>,
    /// Compiled user-function table for `MakeFunction`/`Call`.
    pub functions: Vec<RegCompiledFunction>,
    /// Declared struct definitions: name -> fields.
    pub struct_defs: HashMap<String, Vec<(String, bool)>>,
    /// Active nested-call frames (depth guard).
    pub frames: Vec<RegCallFrame>,
    /// Captured upvalue cells of each active call, parallel to `frames`.
    ///
    /// A closure invocation pushes its own cells; a plain function call pushes an empty
    /// frame so `GetUpvalue`/`SetUpvalue` resolve against the innermost frame.
    pub upvalue_frames: Vec<Vec<Rc<RefCell<Value>>>>,
    /// Signature-contract descriptors of the running function, indexed by `AssertContract`.
    pub contracts: Vec<RegContract>,
    /// Collections currently being iterated, guarded against structural mutation.
    pub active_iterations: Vec<usize>,
    /// Active exception/failure handler stack: `(handler_pc, err_reg)`.
    pub handler_stack: Vec<(usize, u8)>,
    /// Stack of preserved caller registers for windowed frame activations.
    /// Eliminates the blind 4 KiB (256 Value) clone on nested calls by saving
    /// only the register window needed by the callee.
    pub register_stack: Vec<Value>,
    /// Maximum nested-call depth.
    pub max_call_depth: usize,
    /// Linear bump arena for string/buffer allocations.
    pub arena: ArenaAllocator,
    /// Program counter.
    pub pc: usize,
    /// Maximum instructions the register machine may execute before it faults
    /// (`None` = unlimited).
    ///
    /// Mirrors `Vm`'s host-side instruction budget: it is set only through the embedding API,
    /// never by language syntax or a CLI flag, so a program's observable behaviour is unchanged
    /// unless the embedder opts in. See `docs/adp/ADP-003-execution-budgets.md`.
    pub max_instructions: Option<u64>,
    /// Interval in instructions between fuel budget checks (default: 64).
    pub meter_interval: u64,
    /// Instructions executed since the last reset, tracked only while a budget is set.
    pub instructions_executed: u64,
    /// Receiver-first native methods registered by the standard library,
    /// mirroring `Vm::method_natives` so both engines bind `x.len()` alike.
    pub method_natives: HashMap<(String, String), (usize, MethodNative)>,
    /// Cooperative task scheduler state (see [`crate::reg_task`]).
    pub sched: RegScheduler,
    /// Cooperative-suspend capture in flight while a suspension unwinds the
    /// Rust activation chain; consumed when the unwind reaches the task entry.
    pub(crate) pending_capture: Option<Box<RegSuspendCapture>>,
}

/// Default periodic instruction metering interval (64 instructions).
pub const DEFAULT_REG_METER_INTERVAL: u64 = 64;

impl Default for RegVm {
    fn default() -> Self {
        Self::new()
    }
}

impl RegVm {
    /// Constructs a new register machine with all registers initialized to `none`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            registers: std::array::from_fn(|_| Value::None),
            constants: Vec::new(),
            globals: HashMap::new(),
            functions: Vec::new(),
            struct_defs: HashMap::new(),
            frames: Vec::new(),
            upvalue_frames: Vec::new(),
            contracts: Vec::new(),
            active_iterations: Vec::new(),
            handler_stack: Vec::new(),
            register_stack: Vec::new(),
            max_call_depth: REG_MAX_CALL_DEPTH,
            arena: ArenaAllocator::with_default_capacity(),
            pc: 0,
            max_instructions: None,
            meter_interval: DEFAULT_REG_METER_INTERVAL,
            instructions_executed: 0,
            method_natives: HashMap::new(),
            sched: RegScheduler::default(),
            pending_capture: None,
        }
    }

    /// Registers a receiver-first native method (mirrors `Vm::register_method_native`).
    pub fn register_method_native(
        &mut self,
        type_name: &str,
        method: &str,
        arity: usize,
        func: MethodNative,
    ) {
        self.method_natives
            .insert((type_name.to_string(), method.to_string()), (arity, func));
    }

    /// Binds a builtin method of `receiver` by name, if one exists: the register
    /// engine's counterpart of the stack VM's `bind_method`. Higher-order names
    /// bind as [`MethodKind::HigherOrder`]; `Sequence` pipelines bind there too
    /// and are executed by the `Call` handler.
    fn reg_bind_method(&self, receiver: &Value, method: &str) -> Option<Value> {
        let type_name = receiver.type_name().to_string();
        if let Some((arity, func)) = self.method_natives.get(&(type_name, method.to_string())) {
            return Some(Value::bound_method(
                method.to_string(),
                *arity,
                receiver.clone(),
                MethodKind::Native(*func),
            ));
        }
        if matches!(
            method,
            "filter" | "transform" | "map" | "sort_by" | "any" | "all" | "flat_map"
        ) {
            return Some(Value::bound_method(
                method.to_string(),
                1,
                receiver.clone(),
                MethodKind::HigherOrder,
            ));
        }
        if method == "reduce" {
            return Some(Value::bound_method(
                method.to_string(),
                2,
                receiver.clone(),
                MethodKind::HigherOrder,
            ));
        }
        None
    }

    /// Rejects structural mutation of a collection that is currently being iterated
    /// (mirrors `Vm::ensure_mutation_allowed`).
    fn ensure_mutation_allowed(&self, receiver: &Value, method: &str) -> Result<(), VmFault> {
        if !MUTATING_METHODS.contains(&method) {
            return Ok(());
        }
        match Self::collection_identity(receiver) {
            Some(id) if self.active_iterations.contains(&id) => {
                Err(VmFault::MutationDuringIteration)
            }
            _ => Ok(()),
        }
    }

    /// Converts a bytecode constant into a runtime value.
    fn convert_constant(c: &Constant) -> Value {
        match c {
            Constant::Nil => Value::None,
            Constant::Bool(b) => Value::Bool(*b),
            Constant::Int(n) => Value::Int(*n),
            Constant::Float(f) => Value::Float(*f),
            Constant::String(s) => Value::String(Rc::new(s.clone())),
        }
    }

    /// Stable identity of a managed collection, used to detect structural mutation during
    /// iteration without borrowing the collection itself.
    fn collection_identity(value: &Value) -> Option<usize> {
        match value {
            Value::List(list) => Some(Rc::as_ptr(list) as usize),
            Value::Dict(dict) => Some(Rc::as_ptr(dict) as usize),
            Value::Bytes(bytes) => Some(Rc::as_ptr(bytes) as usize),
            Value::Set(items) => Some(Rc::as_ptr(items) as usize),
            _ => None,
        }
    }

    /// Returns whether `value` satisfies a written signature contract.
    fn contract_holds(&self, type_name: &str, nullable: bool, value: &Value) -> bool {
        if matches!(value, Value::None) {
            return nullable;
        }
        if type_name == "Function" {
            return matches!(
                value,
                Value::Function { .. }
                    | Value::Closure(_)
                    | Value::Native(_)
                    | Value::BoundMethod(_)
                    | Value::StructMethod(_)
            );
        }
        if let Some(tag) = TypeTag::from_name(type_name) {
            return tag.matches(value);
        }
        if self.struct_defs.contains_key(type_name)
            || self.struct_defs.keys().any(|k| {
                k.split_once('.')
                    .is_some_and(|(parent, _)| parent == type_name)
            })
        {
            return match value {
                Value::Struct(instance) => {
                    let actual = &instance.borrow().type_name;
                    actual == type_name
                        || actual
                            .split_once('.')
                            .is_some_and(|(parent, _)| parent == type_name)
                }
                _ => false,
            };
        }
        true
    }

    /// Caller-visible arity of `operation` on `value`, when the register engine can resolve it.
    ///
    /// The register engine only sees compiled functions, so interface operations declared as
    /// struct methods (`Type.method`) resolve here. Operations backed by native methods are not
    /// visible and report `None`, which the contract check treats as satisfied rather than
    /// inventing a failure.
    fn operation_arity(&self, value: &Value, operation: &str) -> Option<usize> {
        if let Value::Struct(instance) = value {
            let type_name = instance.borrow().type_name.clone();
            let name = format!("{type_name}.{operation}");
            if let Some(func) = self.functions.iter().find(|f| f.name == name) {
                return Some(func.arity.saturating_sub(1));
            }
        }
        None
    }

    /// Arity guard mirroring the stack VM's `check_arity`: `usize::MAX` is the open-ended
    /// (variadic) sentinel, so `path.join(a, b, c)` and friends accept extra arguments while
    /// fixed-arity natives still reject a wrong count. Keeping the rule identical is what makes
    /// `Value::native(.., usize::MAX, ..)` behave the same on both engines.
    fn check_arity(&self, provided: usize, expected: usize) -> Result<(), VmFault> {
        if expected == usize::MAX || provided == expected {
            Ok(())
        } else {
            Err(VmFault::TypeMismatch {
                expected: format!("{expected} arguments"),
                actual: format!("{provided} arguments"),
            })
        }
    }

    fn corrupted(&self, reason: impl Into<String>) -> VmFault {
        VmFault::CorruptedBytecode {
            offset: self.pc.saturating_sub(1),
            reason: reason.into(),
        }
    }

    fn checked_jump(&self, offset: i32, length: usize) -> Result<usize, VmFault> {
        self.pc
            .checked_add_signed(offset as isize)
            .filter(|target| *target < length)
            .ok_or_else(|| self.corrupted("jump target outside instruction stream"))
    }

    /// Resolves a struct method from the compiled function table.
    ///
    /// Methods are stored as `Type.method` functions whose arity counts the receiver, matching
    /// the stack VM's `lookup_struct_method` (including the dotted-parent fallback). Returns the
    /// function-table index, total arity (receiver included) and async flag.
    fn find_struct_method(&self, type_name: &str, method: &str) -> Option<(usize, usize, bool)> {
        let exact = format!("{type_name}.{method}");
        if let Some(idx) = self.functions.iter().position(|f| f.name == exact) {
            let func = &self.functions[idx];
            return Some((idx, func.arity, func.is_async));
        }
        let parent = type_name.split_once('.')?.0;
        let inherited = format!("{parent}.{method}");
        let idx = self.functions.iter().position(|f| f.name == inherited)?;
        let func = &self.functions[idx];
        Some((idx, func.arity, func.is_async))
    }

    /// Loads a compiled register module (function table for `MakeFunction`).
    pub fn load_module(&mut self, module: &RegCompiledModule) {
        self.functions = module.functions.clone();
        self.struct_defs = module.struct_defs.clone();
    }

    /// Executes a compiled register module's top-level script.
    ///
    /// # Errors
    /// Returns [`VmFault`] if a runtime error occurs.
    pub fn run_module(&mut self, module: &RegCompiledModule) -> Result<Value, VmFault> {
        self.functions = module.functions.clone();
        self.struct_defs = module.struct_defs.clone();
        self.frames.clear();
        self.upvalue_frames.clear();
        self.active_iterations.clear();
        self.handler_stack.clear();
        self.register_stack.clear();
        self.run_function(&module.top_level)
    }

    /// Invokes a compiled user function with isolated registers, driving the
    /// cooperative scheduler when the call parks on a task point.
    ///
    /// # Errors
    /// Returns [`VmFault`] on arity mismatch, depth overflow, or runtime faults.
    pub fn invoke_function(
        &mut self,
        func: &RegCompiledFunction,
        args: &[Value],
    ) -> Result<Value, VmFault> {
        self.run_driven(
            |s| match s.invoke_with_upvalues(func, args, Vec::new(), 0, true)? {
                RegCall::Value(value) => Ok(value),
                RegCall::Suspended => Ok(Value::None),
            },
        )
    }

    /// Invokes a compiled function with an explicit captured-upvalue frame.
    ///
    /// Returns [`RegCall::Suspended`] when the callee parks on a task point: the
    /// caller's activation then travels in the pending capture's chain and `run`
    /// must return without touching registers. `dest` records where the callee's
    /// result lands once the chain resumes.
    ///
    /// # Errors
    /// Returns [`VmFault`] on depth overflow or a runtime fault inside the callee.
    pub(crate) fn invoke_with_upvalues(
        &mut self,
        func: &RegCompiledFunction,
        args: &[Value],
        upvalues: Vec<Rc<RefCell<Value>>>,
        dest: u8,
        entry: bool,
    ) -> Result<RegCall, VmFault> {
        if self.frames.len() >= self.max_call_depth {
            return Err(VmFault::CorruptedBytecode {
                offset: self.pc.saturating_sub(1),
                reason: format!(
                    "call stack overflow: max depth {} exceeded",
                    self.max_call_depth
                ),
            });
        }
        self.frames.push(RegCallFrame {
            return_pc: self.pc,
            return_reg: dest,
        });
        self.upvalue_frames.push(upvalues);
        let callee_regs = func.num_registers.max(func.arity).max(args.len()).min(256);
        let window_start = self.register_stack.len();
        self.register_stack
            .extend_from_slice(&self.registers[..callee_regs]);
        let saved_constants = std::mem::take(&mut self.constants);
        let saved_contracts = std::mem::take(&mut self.contracts);
        let saved_pc = self.pc;
        for (i, arg) in args.iter().enumerate().take(callee_regs) {
            self.registers[i] = arg.clone();
        }
        for i in args.len()..func.arity.min(callee_regs) {
            self.registers[i] = Value::Unset;
        }
        for i in func.arity.min(callee_regs)..callee_regs {
            self.registers[i] = Value::None;
        }
        self.constants = func.constants.iter().map(Self::convert_constant).collect();
        self.contracts = func.contracts.clone();
        self.pc = 0;
        let outcome = self.run_loop(&func.instructions);
        let saved_slice = &self.register_stack[window_start..];
        self.registers[..callee_regs].clone_from_slice(saved_slice);
        self.register_stack.truncate(window_start);
        self.constants = saved_constants;
        self.contracts = saved_contracts;
        self.pc = saved_pc;
        let cells = self.upvalue_frames.pop().unwrap_or_default();
        self.frames.pop();
        if self.pending_capture.is_some() {
            // Cooperative unwind: the innermost frame was captured whole and
            // this activation joins the chain unless it is the task entry (the
            // chain bottom has no caller to resume into). The cell pop matches
            // what the suspend hook left: one pop per unwinding activation.
            if !entry && let Some(capture) = self.pending_capture.as_mut() {
                capture.chain.push(RegFrameSlot {
                    code: Vec::new(),
                    registers: self.registers.clone(),
                    constants: self.constants.clone(),
                    contracts: self.contracts.clone(),
                    pc: self.pc,
                    dest,
                    cells,
                });
            }
            return Ok(RegCall::Suspended);
        }
        outcome.map(RegCall::Value)
    }

    /// Executes a compiled register function as the main task body, driving the
    /// cooperative scheduler when it parks.
    ///
    /// # Errors
    /// Returns [`VmFault`] if a runtime error occurs.
    pub fn run_function(
        &mut self,
        func: &aipo_bytecode::RegCompiledFunction,
    ) -> Result<Value, VmFault> {
        self.run_driven(|s| {
            s.constants = func.constants.iter().map(Self::convert_constant).collect();
            s.contracts = func.contracts.clone();
            s.frames.clear();
            s.upvalue_frames.clear();
            s.register_stack.clear();
            s.pc = 0;
            s.run_loop(&func.instructions)
        })
    }

    /// Configures the maximum instruction execution budget (`None` = unlimited).
    pub fn set_max_instructions(&mut self, limit: Option<u64>) {
        self.max_instructions = limit;
    }

    /// Configures the periodic instruction metering interval (default: 64).
    pub fn set_meter_interval(&mut self, interval: u64) {
        self.meter_interval = interval.max(1);
    }

    /// Returns the current periodic instruction metering interval.
    #[must_use]
    pub fn meter_interval(&self) -> u64 {
        self.meter_interval
    }

    /// Returns the number of instructions executed since the last reset.
    #[must_use]
    pub fn instruction_count(&self) -> u64 {
        self.instructions_executed
    }

    /// Configures the maximum instruction budget (`None` disables).
    pub fn set_instruction_budget(&mut self, limit: Option<u64>) {
        self.max_instructions = limit;
        self.instructions_executed = 0;
    }

    /// Resets the executed instruction counter to zero.
    pub fn reset_instruction_count(&mut self) {
        self.instructions_executed = 0;
    }

    /// Executes a raw instruction array as the main task, driving the
    /// cooperative scheduler when it parks (embedding entry point).
    ///
    /// # Errors
    /// Returns [`VmFault`] if a runtime error (e.g. division by zero, overflow) occurs.
    pub fn run(&mut self, code: &[RegInstruction]) -> Result<Value, VmFault> {
        self.run_driven(|s| {
            s.pc = 0;
            s.run_loop(code)
        })
    }

    /// Executes instructions sequentially from the current program counter
    /// until `Return` or error. This is one activation's step: it does not
    /// drive the scheduler and reports a cooperative park through the pending
    /// capture (`Ok(_)` plus `pending_capture` set).
    ///
    /// # Errors
    /// Returns [`VmFault`] if a runtime error (e.g. division by zero, overflow) occurs.
    pub(crate) fn run_loop(&mut self, code: &[RegInstruction]) -> Result<Value, VmFault> {
        while self.pc < code.len() {
            // Instruction budget with interval metering (ADP-003 / AIPO-OMNI-004):
            // The counter accumulates across nested activations, but the limit check
            // is amortized across `meter_interval` steps to keep the hot path lean.
            if let Some(limit) = self.max_instructions {
                self.instructions_executed = self.instructions_executed.saturating_add(1);
                let interval = self.meter_interval.min(limit.max(1));
                if self.instructions_executed.is_multiple_of(interval)
                    && self.instructions_executed > limit
                {
                    return Err(VmFault::Overflow {
                        details: format!(
                            "execution budget exceeded: instruction limit of {limit} reached"
                        ),
                    });
                }
            }
            let inst = code[self.pc];
            self.pc += 1;

            let Some(op) = inst.opcode() else {
                return Err(VmFault::CorruptedBytecode {
                    offset: self.pc - 1,
                    reason: format!("unknown opcode {}", inst.raw_opcode()),
                });
            };

            let a = inst.a() as usize;
            let b = inst.b() as usize;
            let c = inst.c() as usize;

            if b >= self.registers.len()
                && matches!(
                    op,
                    RegOpCode::Move
                        | RegOpCode::Add
                        | RegOpCode::Sub
                        | RegOpCode::Mul
                        | RegOpCode::Div
                        | RegOpCode::IntDiv
                        | RegOpCode::Mod
                        | RegOpCode::Neg
                        | RegOpCode::Not
                        | RegOpCode::Equal
                        | RegOpCode::NotEqual
                        | RegOpCode::Less
                        | RegOpCode::LessEqual
                        | RegOpCode::Greater
                        | RegOpCode::GreaterEqual
                        | RegOpCode::CallPipe
                        | RegOpCode::GetIndex
                        | RegOpCode::SetIndex
                        | RegOpCode::IterAt
                        | RegOpCode::IterPrimary
                        | RegOpCode::IterKey
                        | RegOpCode::IterValue
                        | RegOpCode::NewList
                        | RegOpCode::NewDict
                        | RegOpCode::Range
                        | RegOpCode::Len
                        | RegOpCode::CloneStruct
                        | RegOpCode::TypeIs
                        | RegOpCode::TypeIsNullable
                )
            {
                return Err(self.corrupted(format!("register B={b} exceeds register file")));
            }

            match op {
                RegOpCode::Nop => {}
                RegOpCode::Move => {
                    self.registers[a] = self.registers[b].clone();
                }
                RegOpCode::LoadConst => {
                    let bx = inst.bx() as usize;
                    let val =
                        self.constants.get(bx).cloned().ok_or_else(|| {
                            self.corrupted(format!("invalid constant index {bx}"))
                        })?;
                    self.registers[a] = val;
                }
                RegOpCode::LoadInt => {
                    self.registers[a] = Value::Int(i64::from(inst.sbx()));
                }
                RegOpCode::LoadNil => {
                    self.registers[a] = Value::None;
                }
                RegOpCode::LoadUnset => {
                    self.registers[a] = Value::Unset;
                }
                RegOpCode::LoadBool => {
                    self.registers[a] = Value::Bool(b != 0);
                }
                // Arithmetic delegates to the shared `Value` canon so both
                // engines agree on widening (Int/Float mixing), `Duration`
                // arithmetic, overflow faults and division forms byte for byte.
                RegOpCode::Add => {
                    self.registers[a] = self.registers[b].add(&self.registers[c])?;
                }
                RegOpCode::Sub => {
                    self.registers[a] = self.registers[b].sub(&self.registers[c])?;
                }
                RegOpCode::Mul => {
                    self.registers[a] = self.registers[b].mul(&self.registers[c])?;
                }
                RegOpCode::Div => {
                    self.registers[a] = self.registers[b].div(&self.registers[c])?;
                }
                RegOpCode::IntDiv => {
                    self.registers[a] = self.registers[b].int_div(&self.registers[c])?;
                }
                RegOpCode::Mod => {
                    self.registers[a] = self.registers[b].modulo(&self.registers[c])?;
                }
                RegOpCode::Neg => {
                    self.registers[a] = self.registers[b].negate()?;
                }
                RegOpCode::Not => {
                    let b_val = self.registers[b].as_bool()?;
                    self.registers[a] = Value::Bool(!b_val);
                }
                RegOpCode::Equal => {
                    self.registers[a] = Value::Bool(self.registers[b] == self.registers[c]);
                }
                RegOpCode::NotEqual => {
                    self.registers[a] = Value::Bool(self.registers[b] != self.registers[c]);
                }
                RegOpCode::Less => {
                    self.registers[a] = self.registers[b].less(&self.registers[c])?;
                }
                RegOpCode::LessEqual => {
                    self.registers[a] = self.registers[b].less_equal(&self.registers[c])?;
                }
                RegOpCode::Greater => {
                    self.registers[a] = self.registers[b].greater(&self.registers[c])?;
                }
                RegOpCode::GreaterEqual => {
                    self.registers[a] = self.registers[b].greater_equal(&self.registers[c])?;
                }
                RegOpCode::Jump => {
                    self.pc = self.checked_jump(inst.sbx(), code.len())?;
                }
                RegOpCode::JumpIfTrue => {
                    let target = self.checked_jump(inst.sbx(), code.len())?;
                    if self.registers[a].as_bool()? {
                        self.pc = target;
                    }
                }
                RegOpCode::JumpIfFalse => {
                    let target = self.checked_jump(inst.sbx(), code.len())?;
                    if !self.registers[a].as_bool()? {
                        self.pc = target;
                    }
                }
                RegOpCode::JumpIfSetLocal => {
                    let target = self.checked_jump(inst.sbx(), code.len())?;
                    if !matches!(self.registers[a], Value::Unset) {
                        self.pc = target;
                    }
                }
                RegOpCode::GetGlobal => {
                    let bx = inst.bx() as usize;
                    if let Some(Value::String(name)) = self.constants.get(bx) {
                        let val = self
                            .globals
                            .get(name.as_str())
                            .cloned()
                            .unwrap_or(Value::None);
                        self.registers[a] = val;
                    }
                }
                RegOpCode::SetGlobal => {
                    let bx = inst.bx() as usize;
                    if let Some(Value::String(name)) = self.constants.get(bx) {
                        self.globals
                            .insert(name.to_string(), self.registers[a].clone());
                    }
                }
                RegOpCode::GetField => {
                    let bx = inst.bx() as usize;
                    let field_name = match self.constants.get(bx) {
                        Some(Value::String(s)) => s.as_str(),
                        _ => {
                            return Err(VmFault::CorruptedBytecode {
                                offset: self.pc - 1,
                                reason: "invalid field name constant".to_string(),
                            });
                        }
                    };
                    let target = self.registers[a].clone();
                    if let Value::Failure(err) = &target {
                        if field_name == "message" {
                            self.registers[a] = Value::String(Rc::new(err.message.clone()));
                            continue;
                        }
                        if field_name == "payload" {
                            self.registers[a] = err.payload.clone();
                            continue;
                        }
                        // Mirror the stack VM: any other member access on a
                        // recoverable failure passes the value through untouched.
                        self.registers[a] = target;
                        continue;
                    }
                    match target {
                        Value::Struct(inst) => {
                            let field = inst.borrow().get_field(field_name).cloned();
                            if let Some(val) = field {
                                self.registers[a] = val;
                            } else {
                                // Mirror the stack VM's `GetField` fallback: a missing field
                                // resolves `Type.method` from the function table, then the
                                // standard library's receiver-first natives, and binds the
                                // receiver either way. Unknown names stay `NoSuchField`.
                                let type_name = inst.borrow().type_name.clone();
                                if let Some((func_idx, total_arity, is_async)) =
                                    self.find_struct_method(&type_name, field_name)
                                {
                                    self.registers[a] = Value::struct_method(
                                        inst.clone(),
                                        func_idx as u32,
                                        total_arity as u16,
                                        is_async,
                                    );
                                } else if let Some((arity, func)) = self
                                    .method_natives
                                    .get(&(type_name.clone(), field_name.to_string()))
                                {
                                    self.registers[a] = Value::bound_method(
                                        format!("{type_name}.{field_name}"),
                                        *arity,
                                        Value::Struct(inst.clone()),
                                        MethodKind::Native(*func),
                                    );
                                } else {
                                    return Err(VmFault::NoSuchField {
                                        type_name,
                                        field: field_name.to_string(),
                                    });
                                }
                            }
                        }
                        Value::Dict(d) => {
                            let key = Value::String(Rc::new(field_name.to_string()));
                            let found = d.borrow().get(&key).cloned();
                            if let Some(val) = found {
                                self.registers[a] = val;
                            } else if let Some(method) =
                                self.reg_bind_method(&Value::Dict(d), field_name)
                            {
                                self.registers[a] = method;
                            } else {
                                return Err(VmFault::KeyNotFound {
                                    key: field_name.to_string(),
                                });
                            }
                        }
                        Value::Group(id) => {
                            // `group.spawn` / `group.wait` bind like the stack
                            // VM's receiver-first dispatch; the `Call` handler
                            // routes them to the cooperative scheduler.
                            self.registers[a] = Value::BoundMethod(Rc::new(BoundMethodData {
                                name: field_name.to_string(),
                                arity: if field_name == "wait" { 0 } else { 2 },
                                receiver: Value::Group(id),
                                kind: MethodKind::Native(|_, _| Ok(Value::None)),
                            }));
                        }
                        other => {
                            if let Some(method) = self.reg_bind_method(&other, field_name) {
                                self.registers[a] = method;
                            } else {
                                return Err(VmFault::TypeMismatch {
                                    expected: "struct, collection, or type with that member"
                                        .to_string(),
                                    actual: format!("{}.{field_name}", other.type_name()),
                                });
                            }
                        }
                    }
                }
                RegOpCode::SetField => {
                    let name_idx = b;
                    let val_reg = c;
                    let field_name = match self.constants.get(name_idx) {
                        Some(Value::String(s)) => s.as_str(),
                        _ => {
                            return Err(VmFault::CorruptedBytecode {
                                offset: self.pc - 1,
                                reason: "invalid field name constant".to_string(),
                            });
                        }
                    };
                    let new_val = self.registers[val_reg].clone();
                    match &self.registers[a] {
                        Value::Struct(inst) => {
                            inst.borrow_mut().set_field(field_name, new_val)?;
                        }
                        Value::Dict(d) => {
                            let key = Value::String(Rc::new(field_name.to_string()));
                            d.borrow_mut().upsert(key, new_val);
                        }
                        other => {
                            return Err(VmFault::NoSuchField {
                                type_name: other.type_name().to_string(),
                                field: field_name.to_string(),
                            });
                        }
                    }
                }
                RegOpCode::GetIndex => {
                    let target = self.registers[b].clone();
                    let key = self.registers[c].clone();
                    // Model B at the boundary: indexing a failure passes it through.
                    if target.is_failure() {
                        self.registers[a] = target;
                        continue;
                    }
                    if key.is_failure() {
                        self.registers[a] = key;
                        continue;
                    }
                    match (&target, &key) {
                        (Value::List(l), Value::Range(r)) => {
                            let items = l.borrow().to_vec();
                            let (from, to) = normalize_range(r.start, r.end, items.len());
                            self.registers[a] =
                                Value::List(Rc::new(RefCell::new(items[from..to].to_vec().into())));
                        }
                        (Value::String(s), Value::Range(r)) => {
                            let chars: Vec<char> = s.chars().collect();
                            let (from, to) = normalize_range(r.start, r.end, chars.len());
                            let slice: String = chars[from..to].iter().collect();
                            self.registers[a] = Value::String(Rc::new(slice));
                        }
                        (Value::Bytes(b), Value::Range(r)) => {
                            let bytes = b.borrow().to_vec();
                            let (from, to) = normalize_range(r.start, r.end, bytes.len());
                            self.registers[a] = Value::Bytes(Rc::new(RefCell::new(
                                bytes[from..to].to_vec().into(),
                            )));
                        }
                        (Value::Range(r), Value::Int(i)) => {
                            // Positional access into a half-open range, which is what makes
                            // `each i in 0..n` (documented canon) lower to the same
                            // index-based loop every other iterable uses.
                            let len = usize::try_from((r.end - r.start).max(0)).unwrap_or(0);
                            #[allow(clippy::cast_possible_wrap)]
                            let actual = if *i < 0 { (len as i64) + *i } else { *i };
                            let resolved = usize::try_from(actual)
                                .ok()
                                .filter(|&idx| idx < len)
                                .ok_or(VmFault::IndexOutOfRange { index: *i, len })?;
                            #[allow(clippy::cast_possible_wrap)]
                            let offset = resolved as i64;
                            self.registers[a] = Value::Int(r.start + offset);
                        }
                        (Value::List(l), Value::Int(i)) => {
                            let list = l.borrow();
                            let len = list.len();
                            #[allow(clippy::cast_possible_wrap)]
                            let actual = if *i < 0 { (len as i64) + *i } else { *i };
                            let idx = usize::try_from(actual)
                                .ok()
                                .filter(|&idx| idx < len)
                                .ok_or(VmFault::IndexOutOfRange { index: *i, len })?;
                            self.registers[a] = list[idx].clone();
                        }
                        (Value::Dict(d), k) => {
                            self.registers[a] = d.borrow().get(k).cloned().unwrap_or(Value::None);
                        }
                        (Value::Bytes(b), Value::Int(i)) => {
                            let bytes = b.borrow();
                            let len = bytes.len();
                            #[allow(clippy::cast_possible_wrap)]
                            let actual = if *i < 0 { (len as i64) + *i } else { *i };
                            let idx = usize::try_from(actual)
                                .ok()
                                .filter(|&idx| idx < len)
                                .ok_or(VmFault::IndexOutOfRange { index: *i, len })?;
                            self.registers[a] = Value::Byte(bytes[idx]);
                        }
                        (Value::String(s), Value::Int(i)) => {
                            let chars: Vec<char> = s.chars().collect();
                            let len = chars.len();
                            #[allow(clippy::cast_possible_wrap)]
                            let actual = if *i < 0 { (len as i64) + *i } else { *i };
                            let idx = usize::try_from(actual)
                                .ok()
                                .filter(|&idx| idx < len)
                                .ok_or(VmFault::IndexOutOfRange { index: *i, len })?;
                            self.registers[a] = Value::String(Rc::new(chars[idx].to_string()));
                        }
                        (other, _) => {
                            return Err(VmFault::TypeMismatch {
                                expected: "indexable collection".to_string(),
                                actual: other.type_name().to_string(),
                            });
                        }
                    }
                }
                RegOpCode::SetIndex => {
                    let key = &self.registers[b];
                    let val = self.registers[c].clone();
                    // Structural mutation of a collection under active iteration is a canon
                    // error, however it is spelled.
                    let identity = Self::collection_identity(&self.registers[a]);
                    if identity.is_some_and(|id| self.active_iterations.contains(&id)) {
                        return Err(VmFault::MutationDuringIteration);
                    }
                    match &self.registers[a] {
                        Value::List(l) => {
                            if let Value::Int(i) = key {
                                let mut list = l.borrow_mut();
                                let len = list.len();
                                #[allow(clippy::cast_possible_wrap)]
                                let actual = if *i < 0 { (len as i64) + *i } else { *i };
                                let idx = usize::try_from(actual)
                                    .ok()
                                    .filter(|&idx| idx < len)
                                    .ok_or(VmFault::IndexOutOfRange { index: *i, len })?;
                                list[idx] = val;
                            } else {
                                return Err(VmFault::TypeMismatch {
                                    expected: "Int index".to_string(),
                                    actual: key.type_name().to_string(),
                                });
                            }
                        }
                        Value::Dict(d) => {
                            d.borrow_mut().upsert(key.clone(), val);
                        }
                        Value::Bytes(bytes) => {
                            if let (Value::Int(i), Value::Byte(b)) = (key, &val) {
                                let mut b_vec = bytes.borrow_mut();
                                let len = b_vec.len();
                                #[allow(clippy::cast_possible_wrap)]
                                let actual = if *i < 0 { (len as i64) + *i } else { *i };
                                let idx = usize::try_from(actual)
                                    .ok()
                                    .filter(|&idx| idx < len)
                                    .ok_or(VmFault::IndexOutOfRange { index: *i, len })?;
                                b_vec[idx] = *b;
                            }
                        }
                        other => {
                            return Err(VmFault::TypeMismatch {
                                expected: "mutable collection".to_string(),
                                actual: other.type_name().to_string(),
                            });
                        }
                    }
                }
                RegOpCode::Call => {
                    let callee = self.registers[a].clone();
                    let arg_count = b;
                    let args = self
                        .registers
                        .get(a + 1..a + 1 + arg_count)
                        .ok_or_else(|| self.corrupted("call arguments exceed register file"))?
                        .to_vec();
                    match callee {
                        Value::Native(native) if is_task_native(&native.name) => {
                            match self.task_call(&native.name, &args, code)? {
                                RegCall::Value(result) => self.registers[a] = result,
                                RegCall::Suspended => return Ok(Value::None),
                            }
                        }
                        Value::Native(native) => {
                            self.check_arity(arg_count, native.arity)?;
                            let result = (native.func)(&args)?;
                            self.registers[a] = result;
                        }
                        Value::BoundMethod(bm) if matches!(&bm.receiver, Value::Group(_)) => {
                            // `Group` methods need the scheduler, mirroring the
                            // stack VM's receiver-first dispatch in `begin_call`.
                            match self.group_method(&bm.name, &bm.receiver, &args, code)? {
                                RegCall::Value(result) => self.registers[a] = result,
                                RegCall::Suspended => return Ok(Value::None),
                            }
                        }
                        Value::BoundMethod(bm) => match bm.kind {
                            MethodKind::Native(func) => {
                                self.ensure_mutation_allowed(&bm.receiver, &bm.name)?;
                                let result = func(&bm.receiver, &args)?;
                                self.registers[a] = result;
                            }
                            _ => {
                                return Err(VmFault::NotCallable {
                                    type_name: format!("<method {}>", bm.name),
                                });
                            }
                        },
                        Value::StructMethod(sm) => {
                            // User-defined `Type.method`: the receiver travels as argument 0,
                            // exactly like the stack VM's `MethodKind::Function` path.
                            if arg_count + 1 != sm.total_arity as usize {
                                return Err(VmFault::TypeMismatch {
                                    expected: format!(
                                        "{} arguments",
                                        sm.total_arity.saturating_sub(1)
                                    ),
                                    actual: format!("{arg_count} arguments"),
                                });
                            }
                            let func_idx = sm.entry_ip as usize;
                            let func = self.functions.get(func_idx).cloned().ok_or_else(|| {
                                VmFault::CorruptedBytecode {
                                    offset: self.pc - 1,
                                    reason: format!("unknown function index {func_idx}"),
                                }
                            })?;
                            let mut method_args = Vec::with_capacity(sm.total_arity as usize);
                            method_args.push(Value::Struct(sm.receiver.clone()));
                            method_args.extend(args.iter().cloned());
                            if sm.is_async {
                                // An `async fn` method is an `async fn`: calling it spawns a
                                // task carrying the receiver as argument 0.
                                let callee = Value::Function {
                                    entry_ip: sm.entry_ip,
                                    arity: sm.total_arity,
                                    is_async: true,
                                };
                                let id = self.spawn_reg_task(callee, method_args, None)?;
                                self.registers[a] = Value::Task(id);
                            } else {
                                match self.invoke_with_upvalues(
                                    &func,
                                    &method_args,
                                    Vec::new(),
                                    a as u8,
                                    false,
                                )? {
                                    RegCall::Value(result) => self.registers[a] = result,
                                    RegCall::Suspended => return Ok(Value::None),
                                }
                            }
                        }
                        Value::Function {
                            entry_ip,
                            arity,
                            is_async,
                        } => {
                            if (arity as usize) != arg_count {
                                return Err(VmFault::TypeMismatch {
                                    expected: format!("{arity} arguments"),
                                    actual: format!("{arg_count} arguments"),
                                });
                            }
                            let func_idx = entry_ip as usize;
                            let func = self.functions.get(func_idx).cloned().ok_or_else(|| {
                                VmFault::CorruptedBytecode {
                                    offset: self.pc - 1,
                                    reason: format!("unknown function index {func_idx}"),
                                }
                            })?;
                            if is_async {
                                // Calling an `async fn` spawns a task and
                                // yields its `Task` handle (never runs inline).
                                let callee = Value::Function {
                                    entry_ip,
                                    arity,
                                    is_async,
                                };
                                let id = self.spawn_reg_task(callee, args, None)?;
                                self.registers[a] = Value::Task(id);
                            } else {
                                match self.invoke_with_upvalues(
                                    &func,
                                    &args,
                                    Vec::new(),
                                    a as u8,
                                    false,
                                )? {
                                    RegCall::Value(result) => self.registers[a] = result,
                                    RegCall::Suspended => return Ok(Value::None),
                                }
                            }
                        }
                        Value::Closure(closure) => {
                            if closure.arity != arg_count {
                                return Err(VmFault::TypeMismatch {
                                    expected: format!("{} arguments", closure.arity),
                                    actual: format!("{arg_count} arguments"),
                                });
                            }
                            let func_idx = closure.entry_ip;
                            let func = self.functions.get(func_idx).cloned().ok_or_else(|| {
                                VmFault::CorruptedBytecode {
                                    offset: self.pc - 1,
                                    reason: format!("unknown function index {func_idx}"),
                                }
                            })?;
                            if closure.is_async {
                                let callee = Value::Closure(closure.clone());
                                let id = self.spawn_reg_task(callee, args, None)?;
                                self.registers[a] = Value::Task(id);
                            } else {
                                let upvalues = closure.upvalues.clone();
                                match self
                                    .invoke_with_upvalues(&func, &args, upvalues, a as u8, false)?
                                {
                                    RegCall::Value(result) => self.registers[a] = result,
                                    RegCall::Suspended => return Ok(Value::None),
                                }
                            }
                        }
                        // Core-type constructors (`String(x)`, `Int(x)`, ...) mirror the
                        // stack VM's `begin_call` path byte for byte, so both engines
                        // agree on conversions and on the errors of non-convertible types.
                        Value::Type(tag) => {
                            self.registers[a] = convert_via_type(tag, &args)?;
                        }
                        Value::UserType(name) => {
                            return Err(VmFault::TypeMismatch {
                                expected: "function or callable".to_string(),
                                actual: format!("Type {name}"),
                            });
                        }
                        other => {
                            return Err(VmFault::NotCallable {
                                type_name: other.type_name().to_string(),
                            });
                        }
                    }
                }
                RegOpCode::CallPipe => {
                    let callee = self.registers[c].clone();
                    let stream_arg = self.registers[b].clone();
                    match callee {
                        Value::Native(native) if is_task_native(&native.name) => {
                            match self.task_call(&native.name, &[stream_arg], code)? {
                                RegCall::Value(result) => self.registers[a] = result,
                                RegCall::Suspended => return Ok(Value::None),
                            }
                        }
                        Value::Native(native) => {
                            self.check_arity(1, native.arity)?;
                            let result = (native.func)(&[stream_arg])?;
                            self.registers[a] = result;
                        }
                        Value::BoundMethod(bm) if matches!(&bm.receiver, Value::Group(_)) => {
                            match self.group_method(&bm.name, &bm.receiver, &[stream_arg], code)? {
                                RegCall::Value(result) => self.registers[a] = result,
                                RegCall::Suspended => return Ok(Value::None),
                            }
                        }
                        Value::BoundMethod(bm) => match bm.kind {
                            MethodKind::Native(func) => {
                                self.ensure_mutation_allowed(&bm.receiver, &bm.name)?;
                                let result = func(&bm.receiver, &[stream_arg])?;
                                self.registers[a] = result;
                            }
                            _ => {
                                return Err(VmFault::NotCallable {
                                    type_name: format!("<method {}>", bm.name),
                                });
                            }
                        },
                        Value::Function {
                            entry_ip, arity, ..
                        } => {
                            if arity as usize != 1 {
                                return Err(VmFault::TypeMismatch {
                                    expected: "1 argument for piped call".to_string(),
                                    actual: format!("function arity {arity}"),
                                });
                            }
                            let func_idx = entry_ip as usize;
                            let func = self.functions.get(func_idx).cloned().ok_or_else(|| {
                                VmFault::CorruptedBytecode {
                                    offset: self.pc - 1,
                                    reason: format!("unknown function index {func_idx}"),
                                }
                            })?;
                            match self.invoke_with_upvalues(
                                &func,
                                &[stream_arg],
                                Vec::new(),
                                a as u8,
                                false,
                            )? {
                                RegCall::Value(result) => self.registers[a] = result,
                                RegCall::Suspended => return Ok(Value::None),
                            }
                        }
                        Value::Closure(closure) => {
                            if closure.arity != 1 {
                                return Err(VmFault::TypeMismatch {
                                    expected: "1 argument for piped call".to_string(),
                                    actual: format!("closure arity {}", closure.arity),
                                });
                            }
                            let func_idx = closure.entry_ip;
                            let func = self.functions.get(func_idx).cloned().ok_or_else(|| {
                                VmFault::CorruptedBytecode {
                                    offset: self.pc - 1,
                                    reason: format!("unknown function index {func_idx}"),
                                }
                            })?;
                            let upvalues = closure.upvalues.clone();
                            match self.invoke_with_upvalues(
                                &func,
                                &[stream_arg],
                                upvalues,
                                a as u8,
                                false,
                            )? {
                                RegCall::Value(result) => self.registers[a] = result,
                                RegCall::Suspended => return Ok(Value::None),
                            }
                        }
                        Value::Type(tag) => {
                            self.registers[a] = convert_via_type(tag, &[stream_arg])?;
                        }
                        Value::UserType(name) => {
                            return Err(VmFault::TypeMismatch {
                                expected: "function or callable".to_string(),
                                actual: format!("Type {name}"),
                            });
                        }
                        other => {
                            return Err(VmFault::NotCallable {
                                type_name: other.type_name().to_string(),
                            });
                        }
                    }
                }
                RegOpCode::Await => {
                    // `R[A] = await R[A]`: terminal tasks resolve inline, a
                    // pending task parks the waiter rewound onto this opcode.
                    match self.reg_await(a as u8, code)? {
                        RegCall::Value(result) => self.registers[a] = result,
                        RegCall::Suspended => return Ok(Value::None),
                    }
                }
                RegOpCode::Return => {
                    if let Some(limit) = self.max_instructions
                        && self.instructions_executed > limit
                    {
                        return Err(VmFault::Overflow {
                            details: format!(
                                "execution budget exceeded: instruction limit of {limit} reached"
                            ),
                        });
                    }
                    return Ok(self.registers[a].clone());
                }
                RegOpCode::MakeFunction => {
                    let bx = inst.bx() as usize;
                    if let Some(func) = self.functions.get(bx) {
                        let arity = u16::try_from(func.arity).unwrap_or(u16::MAX);
                        self.registers[a] = Value::Function {
                            entry_ip: bx as u32,
                            arity,
                            is_async: func.is_async,
                        };
                    } else if let Some(Value::Function {
                        entry_ip,
                        arity,
                        is_async,
                    }) = self.constants.get(bx).cloned()
                    {
                        self.registers[a] = Value::Function {
                            entry_ip,
                            arity,
                            is_async,
                        };
                    } else {
                        return Err(VmFault::CorruptedBytecode {
                            offset: self.pc - 1,
                            reason: format!("unknown function index {bx}"),
                        });
                    }
                }
                RegOpCode::NewList => {
                    let start = b;
                    let count = c;
                    if start + count > 256 {
                        return Err(VmFault::StackUnderflow);
                    }
                    let items = self.registers[start..start + count].to_vec();
                    self.registers[a] = Value::List(Rc::new(RefCell::new(items.into())));
                }
                RegOpCode::NewDict => {
                    let start = b;
                    let count = c;
                    if start + count * 2 > 256 {
                        return Err(VmFault::StackUnderflow);
                    }
                    let mut entries = Vec::with_capacity(count);
                    for i in 0..count {
                        let k = self.registers[start + i * 2].clone();
                        let v = self.registers[start + i * 2 + 1].clone();
                        entries.push((k, v));
                    }
                    self.registers[a] =
                        Value::Dict(Rc::new(RefCell::new(DictMap::from_entries(entries))));
                }
                RegOpCode::NewStruct => {
                    let type_c_idx = b;
                    let field_count = c;
                    let type_name = match self.constants.get(type_c_idx) {
                        Some(Value::String(s)) => s.clone(),
                        _ => {
                            return Err(VmFault::CorruptedBytecode {
                                offset: self.pc - 1,
                                reason: "invalid struct type name constant".to_string(),
                            });
                        }
                    };
                    if a + field_count > 256 {
                        return Err(VmFault::StackUnderflow);
                    }
                    let values = self.registers[a..a + field_count].to_vec();
                    let instance = if let Some(defs) = self.struct_defs.get(type_name.as_str()) {
                        let mut fields = Vec::with_capacity(defs.len());
                        let mut fixed_fields = HashSet::new();
                        for ((f_name, is_fixed), val) in defs.iter().cloned().zip(values) {
                            if is_fixed {
                                fixed_fields.insert(f_name.clone());
                            }
                            fields.push((f_name, val));
                        }
                        StructInstance {
                            type_name: (*type_name).clone(),
                            fields,
                            fixed_fields,
                            // Published as "under construction" so a deferred `init` may still
                            // assign `fixed` fields; the emitter seals it right away unless the
                            // construction is deferred to an explicit `SealStruct`.
                            under_construction: true,
                        }
                    } else {
                        let fields = values
                            .into_iter()
                            .enumerate()
                            .map(|(i, v)| (format!("field_{i}"), v))
                            .collect();
                        StructInstance {
                            type_name: (*type_name).clone(),
                            fields,
                            fixed_fields: HashSet::new(),
                            under_construction: true,
                        }
                    };
                    self.registers[a] = Value::Struct(Rc::new(RefCell::new(instance)));
                }
                RegOpCode::Range => {
                    let start = match self.registers[b] {
                        Value::Int(n) => n,
                        ref other => {
                            return Err(VmFault::TypeMismatch {
                                expected: "Int".to_string(),
                                actual: other.type_name().to_string(),
                            });
                        }
                    };
                    let end = match self.registers[c] {
                        Value::Int(n) => n,
                        ref other => {
                            return Err(VmFault::TypeMismatch {
                                expected: "Int".to_string(),
                                actual: other.type_name().to_string(),
                            });
                        }
                    };
                    self.registers[a] = Value::range(start, end);
                }
                RegOpCode::CloneStruct => {
                    let val = self.registers[b].clone();
                    match val {
                        Value::Struct(inst) => {
                            let cloned = inst.borrow().clone();
                            self.registers[a] = Value::Struct(Rc::new(RefCell::new(cloned)));
                        }
                        other => {
                            return Err(VmFault::TypeMismatch {
                                expected: "struct".to_string(),
                                actual: other.type_name().to_string(),
                            });
                        }
                    }
                }
                RegOpCode::IsVariant => {
                    let bx = inst.bx() as usize;
                    let expected = match self.constants.get(bx) {
                        Some(Value::String(s)) => s.as_str(),
                        _ => {
                            return Err(VmFault::CorruptedBytecode {
                                offset: self.pc - 1,
                                reason: "invalid variant name constant".to_string(),
                            });
                        }
                    };
                    let val = &self.registers[a];
                    let matches = match val {
                        Value::Struct(inst) => {
                            let t = &inst.borrow().type_name;
                            t == expected || t.ends_with(&format!(".{expected}"))
                        }
                        _ => false,
                    };
                    self.registers[a] = Value::Bool(matches);
                }
                RegOpCode::Len => {
                    let val = &self.registers[b];
                    let len = match val {
                        Value::String(s) => s.chars().count() as i64,
                        Value::List(l) => l.borrow().len() as i64,
                        Value::Dict(d) => d.borrow().len() as i64,
                        Value::Set(s) => s.borrow().len() as i64,
                        Value::Bytes(b) => b.borrow().len() as i64,
                        Value::Range(r) => (r.end - r.start).max(0),
                        other => {
                            return Err(VmFault::TypeMismatch {
                                expected: "collection, string or range".to_string(),
                                actual: other.type_name().to_string(),
                            });
                        }
                    };
                    self.registers[a] = Value::Int(len);
                }
                RegOpCode::IterAt
                | RegOpCode::IterPrimary
                | RegOpCode::IterKey
                | RegOpCode::IterValue => {
                    let coll = &self.registers[b];
                    let (idx_reg, mode) = match op {
                        RegOpCode::IterPrimary => (c, 0),
                        RegOpCode::IterKey => (c, 1),
                        RegOpCode::IterValue => (c, 2),
                        _ => (c >> 2, (c & 3) as u8),
                    };
                    let idx_val = &self.registers[idx_reg];
                    let ordinal = match idx_val {
                        Value::Int(n) => *n,
                        other => {
                            return Err(VmFault::TypeMismatch {
                                expected: "Int iteration index".to_string(),
                                actual: other.type_name().to_string(),
                            });
                        }
                    };
                    let projected = match coll {
                        Value::List(l) => {
                            let list = l.borrow();
                            let len = list.len();
                            #[allow(clippy::cast_possible_wrap)]
                            let actual = if ordinal < 0 {
                                (len as i64) + ordinal
                            } else {
                                ordinal
                            };
                            let pos = usize::try_from(actual).ok().filter(|&p| p < len).ok_or(
                                VmFault::IndexOutOfRange {
                                    index: ordinal,
                                    len,
                                },
                            )?;
                            if mode == 1 {
                                Value::Int(actual)
                            } else {
                                list[pos].clone()
                            }
                        }
                        Value::Dict(d) => {
                            let dict = d.borrow();
                            let len = dict.len();
                            #[allow(clippy::cast_possible_wrap)]
                            let actual = if ordinal < 0 {
                                (len as i64) + ordinal
                            } else {
                                ordinal
                            };
                            let pos = usize::try_from(actual).ok().filter(|&p| p < len).ok_or(
                                VmFault::IndexOutOfRange {
                                    index: ordinal,
                                    len,
                                },
                            )?;
                            let (k, v) = &dict.entries()[pos];
                            if mode == 2 { v.clone() } else { k.clone() }
                        }
                        Value::Range(r) => {
                            let len = (r.end - r.start).max(0) as usize;
                            #[allow(clippy::cast_possible_wrap)]
                            let actual = if ordinal < 0 {
                                (len as i64) + ordinal
                            } else {
                                ordinal
                            };
                            let pos = usize::try_from(actual).ok().filter(|&p| p < len).ok_or(
                                VmFault::IndexOutOfRange {
                                    index: ordinal,
                                    len,
                                },
                            )?;
                            Value::Int(r.start + (pos as i64))
                        }
                        Value::String(s) => {
                            let chars: Vec<char> = s.chars().collect();
                            let len = chars.len();
                            #[allow(clippy::cast_possible_wrap)]
                            let actual = if ordinal < 0 {
                                (len as i64) + ordinal
                            } else {
                                ordinal
                            };
                            let pos = usize::try_from(actual).ok().filter(|&p| p < len).ok_or(
                                VmFault::IndexOutOfRange {
                                    index: ordinal,
                                    len,
                                },
                            )?;
                            if mode == 1 {
                                Value::Int(actual)
                            } else {
                                Value::String(Rc::new(chars[pos].to_string()))
                            }
                        }
                        other => {
                            return Err(VmFault::TypeMismatch {
                                expected: "iterable collection".to_string(),
                                actual: other.type_name().to_string(),
                            });
                        }
                    };
                    self.registers[a] = projected;
                }
                RegOpCode::TypeIs => {
                    let val = &self.registers[b];
                    let tag = &self.registers[c];
                    let matches = match tag {
                        Value::Type(tag) => tag.matches(val),
                        Value::UserType(target_name) => match val {
                            Value::Struct(inst) => {
                                let name = &inst.borrow().type_name;
                                name == target_name.as_str()
                                    || name
                                        .split_once('.')
                                        .is_some_and(|(parent, _)| parent == target_name.as_str())
                            }
                            _ => false,
                        },
                        Value::Struct(target_inst) => match val {
                            Value::Struct(inst) => {
                                inst.borrow().type_name == target_inst.borrow().type_name
                            }
                            _ => false,
                        },
                        Value::String(s) => match val {
                            Value::Struct(inst) => inst.borrow().type_name == **s,
                            _ => false,
                        },
                        _ => false,
                    };
                    self.registers[a] = Value::Bool(matches);
                }
                RegOpCode::TypeIsNullable => {
                    let val = &self.registers[b];
                    let tag = &self.registers[c];
                    let matches = if matches!(val, Value::None) {
                        true
                    } else {
                        match tag {
                            Value::Type(tag) => tag.matches(val),
                            Value::UserType(target_name) => match val {
                                Value::Struct(inst) => {
                                    let name = &inst.borrow().type_name;
                                    name == target_name.as_str()
                                        || name.split_once('.').is_some_and(|(parent, _)| {
                                            parent == target_name.as_str()
                                        })
                                }
                                _ => false,
                            },
                            Value::Struct(target_inst) => match val {
                                Value::Struct(inst) => {
                                    inst.borrow().type_name == target_inst.borrow().type_name
                                }
                                _ => false,
                            },
                            Value::String(s) => match val {
                                Value::Struct(inst) => inst.borrow().type_name == **s,
                                _ => false,
                            },
                            _ => false,
                        }
                    };
                    self.registers[a] = Value::Bool(matches);
                }
                RegOpCode::PushHandler => {
                    let offset = inst.sbx();
                    let target = (self.pc as i32) + offset;
                    if target < 0 {
                        return Err(VmFault::CorruptedBytecode {
                            offset: self.pc - 1,
                            reason: format!("handler target {target} out of range"),
                        });
                    }
                    self.handler_stack.push((target as usize, a as u8));
                }
                RegOpCode::PopHandler => {
                    self.handler_stack.pop();
                }
                RegOpCode::Fail => {
                    let val = &self.registers[a];
                    let (msg, payload) = match val {
                        Value::String(s) => ((**s).clone(), Value::None),
                        Value::Failure(f) => (f.message.clone(), f.payload.clone()),
                        other => (other.to_string(), other.clone()),
                    };
                    let failure = Value::failure_with_payload(msg, payload);
                    if let Some((handler_pc, err_reg)) = self.handler_stack.pop() {
                        self.registers[err_reg as usize] = failure;
                        self.pc = handler_pc;
                    } else {
                        self.registers[a] = failure;
                    }
                }
                RegOpCode::PropagateFailure => {
                    if self.registers[a].is_failure() {
                        let failure = self.registers[a].clone();
                        if let Some((handler_pc, err_reg)) = self.handler_stack.pop() {
                            self.registers[err_reg as usize] = failure;
                            self.pc = handler_pc;
                        } else {
                            return Ok(failure);
                        }
                    }
                }
                RegOpCode::IterGuard => {
                    let identity =
                        Self::collection_identity(&self.registers[a]).unwrap_or(usize::MAX);
                    self.active_iterations.push(identity);
                }
                RegOpCode::IterGuardEnd => {
                    self.active_iterations.pop();
                }
                RegOpCode::SealStruct => {
                    let instance = match &self.registers[a] {
                        Value::Struct(instance) => Rc::clone(instance),
                        other => {
                            return Err(VmFault::TypeMismatch {
                                expected: "Struct under construction for SealStruct".to_string(),
                                actual: other.type_name().to_string(),
                            });
                        }
                    };
                    let type_name = instance.borrow().type_name.clone();
                    let fixed_fields: HashSet<String> = self
                        .struct_defs
                        .get(&type_name)
                        .map(|defs| {
                            defs.iter()
                                .filter(|(_, is_fixed)| *is_fixed)
                                .map(|(name, _)| name.clone())
                                .collect()
                        })
                        .unwrap_or_default();
                    instance.borrow_mut().seal(fixed_fields);
                }
                RegOpCode::AssertInvariant => {
                    let bx = inst.bx() as usize;
                    let type_name = match self.constants.get(bx) {
                        Some(Value::String(name)) => (**name).clone(),
                        _ => "?".to_string(),
                    };
                    if !matches!(self.registers[a], Value::Bool(true)) {
                        return Err(VmFault::InvariantViolation {
                            type_name,
                            message: "invariant() did not hold after construction".to_string(),
                        });
                    }
                }
                RegOpCode::AssertContract | RegOpCode::AssertContractNullable => {
                    let bx = inst.bx() as usize;
                    let (type_name, nullable, position, operations) =
                        match (self.constants.get(bx), self.constants.get(bx + 1)) {
                            (Some(Value::String(t)), Some(Value::String(p))) => (
                                (**t).clone(),
                                op == RegOpCode::AssertContractNullable,
                                (**p).clone(),
                                Vec::new(),
                            ),
                            _ => {
                                let contract =
                                    self.contracts.get(bx).cloned().ok_or_else(|| {
                                        VmFault::CorruptedBytecode {
                                            offset: self.pc - 1,
                                            reason: format!("contract index {bx} out of bounds"),
                                        }
                                    })?;
                                (
                                    contract.type_name,
                                    contract.nullable,
                                    contract.position,
                                    contract.operations,
                                )
                            }
                        };
                    let value = self.registers[a].clone();
                    // An omitted argument keeps the sentinel and `fail(...)` ends the path, so
                    // neither is a contract violation (mirrors the stack VM).
                    let skippable = matches!(value, Value::Unset | Value::Failure(_));
                    if !skippable && !self.contract_holds(&type_name, nullable, &value) {
                        let actual = match &value {
                            Value::Struct(instance) => instance.borrow().type_name.clone(),
                            other => other.type_name().to_string(),
                        };
                        return Err(VmFault::ContractViolation {
                            position,
                            expected: if nullable {
                                format!("{type_name}?")
                            } else {
                                type_name
                            },
                            actual,
                        });
                    }
                    if !skippable && !matches!(value, Value::None) {
                        for (operation, declared) in &operations {
                            if let Some(actual) = self.operation_arity(&value, operation)
                                && actual != *declared
                            {
                                return Err(VmFault::ContractViolation {
                                    position: position.clone(),
                                    expected: format!("{type_name}.{operation}/{declared}"),
                                    actual: format!("{}.{operation}/{actual}", value.type_name()),
                                });
                            }
                        }
                    }
                }
                RegOpCode::MakeClosure => {
                    let func_idx = b;
                    let count = c;
                    if a + count > 256 {
                        return Err(VmFault::StackUnderflow);
                    }
                    let func = self.functions.get(func_idx).cloned().ok_or_else(|| {
                        VmFault::CorruptedBytecode {
                            offset: self.pc - 1,
                            reason: format!("unknown closure function index {func_idx}"),
                        }
                    })?;
                    let mut upvalues = Vec::with_capacity(count);
                    for i in 0..count {
                        upvalues.push(Rc::new(RefCell::new(self.registers[a + i].clone())));
                    }
                    // Captures snapshot their value at creation time, matching the stack VM.
                    self.registers[a] =
                        Value::closure(func_idx, func.arity, upvalues, func.is_async);
                }
                RegOpCode::GetUpvalue => {
                    let bx = inst.bx() as usize;
                    let cell = self
                        .upvalue_frames
                        .last()
                        .and_then(|frame| frame.get(bx))
                        .cloned()
                        .ok_or(VmFault::StackUnderflow)?;
                    self.registers[a] = cell.borrow().clone();
                }
                RegOpCode::SetUpvalue => {
                    let bx = inst.bx() as usize;
                    let cell = self
                        .upvalue_frames
                        .last()
                        .and_then(|frame| frame.get(bx))
                        .cloned()
                        .ok_or(VmFault::StackUnderflow)?;
                    *cell.borrow_mut() = self.registers[a].clone();
                }
                RegOpCode::FillSelfCapture => {
                    let closure = match &self.registers[a] {
                        Value::Closure(closure) => Rc::clone(closure),
                        other => {
                            return Err(VmFault::TypeMismatch {
                                expected: "closure".to_string(),
                                actual: other.type_name().to_string(),
                            });
                        }
                    };
                    // The builder reserves exactly one `Unset` placeholder, the self capture;
                    // the capture is rewritten with the closure it belongs to.
                    let filled = closure
                        .upvalues
                        .iter()
                        .find(|cell| matches!(&*cell.borrow(), Value::Unset))
                        .cloned()
                        .ok_or_else(|| VmFault::CorruptedBytecode {
                            offset: self.pc - 1,
                            reason: "FillSelfCapture without an Unset placeholder cell".to_string(),
                        })?;
                    *filled.borrow_mut() = Value::Closure(closure);
                }
                RegOpCode::CheckMutations => {
                    // The register engine does not journal invariant-protected field
                    // assignments, so a stable boundary has nothing to commit or roll back
                    // yet. The instruction is kept so emitted boundaries stay balanced.
                }
            }
        }
        if let Some(limit) = self.max_instructions
            && self.instructions_executed > limit
        {
            return Err(VmFault::Overflow {
                details: format!("execution budget exceeded: instruction limit of {limit} reached"),
            });
        }
        Ok(Value::None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_vm_arithmetic_pipeline() {
        let mut vm = RegVm::new();
        let code = vec![
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 1, 10), // R[1] = 10
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 2, 25), // R[2] = 25
            RegInstruction::encode_abc(RegOpCode::Add, 3, 1, 2),    // R[3] = R[1] + R[2] (35)
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 4, 5),  // R[4] = 5
            RegInstruction::encode_abc(RegOpCode::Mul, 0, 3, 4),    // R[0] = R[3] * R[4] (175)
            RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0), // return R[0]
        ];

        let result = vm.run(&code).unwrap();
        assert_eq!(result, Value::Int(175));
    }

    #[test]
    fn test_register_vm_branching() {
        let mut vm = RegVm::new();
        let code = vec![
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 0, 10),
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 1, 20),
            RegInstruction::encode_abc(RegOpCode::Less, 2, 0, 1), // R[2] = 10 < 20 (true)
            RegInstruction::encode_asbx(RegOpCode::JumpIfFalse, 2, 2), // skip if false
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 0, 999), // R[0] = 999
            RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0), // return R[0]
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 0, -1), // R[0] = -1
            RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0), // return R[0]
        ];

        let result = vm.run(&code).unwrap();
        assert_eq!(result, Value::Int(999));
    }

    #[test]
    fn test_register_vm_size_in_memory() {
        // 256 registers of 16 bytes = 4096 bytes (exactly 4 KiB!).
        assert_eq!(std::mem::size_of::<[Value; 256]>(), 4096);
    }

    #[test]
    fn test_reg_emitter_to_reg_vm_end_to_end() {
        use aipo_bytecode::RegEmitter;
        use aipo_ir::{BinaryOp, CoreFunction, CoreInst};
        use aipo_source::SourceSpan;

        let span = SourceSpan::default();
        let func = CoreFunction {
            name: "calculate".to_string(),
            is_async: false,
            params: vec!["a".to_string(), "b".to_string()],
            locals: vec!["res".to_string()],
            upvalues: Vec::new(),
            instructions: vec![
                CoreInst::Load("a".to_string(), span),
                CoreInst::Load("b".to_string(), span),
                CoreInst::Binary(BinaryOp::Mul, span),
                CoreInst::Store("res".to_string(), span),
                CoreInst::Load("res".to_string(), span),
                CoreInst::Return {
                    has_value: true,
                    span,
                },
            ],
            span,
        };

        let compiled = RegEmitter::new().compile_function(&func).unwrap();
        let mut vm = RegVm::new();
        // Set parameters R[0] and R[1]
        vm.registers[0] = Value::Int(6);
        vm.registers[1] = Value::Int(7);

        let outcome = vm.run_function(&compiled).unwrap();
        assert_eq!(outcome, Value::Int(42));
    }

    #[test]
    fn test_register_vm_call_native_and_indexing() {
        let mut vm = RegVm::new();
        let double_native = Value::native("double", 1, |args| match args.first() {
            Some(Value::Int(n)) => Ok(Value::Int(n * 2)),
            _ => Ok(Value::None),
        });
        vm.registers[10] = double_native;
        vm.registers[11] = Value::Int(21);

        let code = vec![
            // R[10] = call R[10] with 1 argument (R[11])
            RegInstruction::encode_abc(RegOpCode::Call, 10, 1, 1),
            // return R[10]
            RegInstruction::encode_abc(RegOpCode::Return, 10, 0, 0),
        ];

        let result = vm.run(&code).unwrap();
        assert_eq!(result, Value::Int(42));
    }

    #[test]
    fn test_register_vm_list_get_and_set_index() {
        use std::cell::RefCell;
        let mut vm = RegVm::new();
        let list = Value::List(Rc::new(RefCell::new(
            vec![Value::Int(10), Value::Int(20)].into(),
        )));
        vm.registers[0] = list;
        vm.registers[1] = Value::Int(1); // index 1
        vm.registers[2] = Value::Int(99); // new value

        let code = vec![
            // SetIndex: R[0][R[1]] = R[2] (list[1] = 99)
            RegInstruction::encode_abc(RegOpCode::SetIndex, 0, 1, 2),
            // GetIndex: R[5] = R[0][R[1]]
            RegInstruction::encode_abc(RegOpCode::GetIndex, 5, 0, 1),
            // return R[5]
            RegInstruction::encode_abc(RegOpCode::Return, 5, 0, 0),
        ];

        let result = vm.run(&code).unwrap();
        assert_eq!(result, Value::Int(99));
    }

    #[test]
    fn test_register_vm_user_function_call() {
        use aipo_bytecode::RegCompiledFunction;
        // adder: R[0] = R[0] + R[1]; return R[0]
        let adder = RegCompiledFunction {
            name: "adder".to_string(),
            arity: 2,
            is_async: false,
            instructions: vec![
                RegInstruction::encode_abc(RegOpCode::Add, 0, 0, 1),
                RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0),
            ],
            constants: vec![],
            num_registers: 2,
            contracts: vec![],
        };
        let mut vm = RegVm::new();
        vm.functions = vec![adder];
        vm.registers[0] = Value::Function {
            entry_ip: 0,
            arity: 2,
            is_async: false,
        };
        vm.registers[1] = Value::Int(6);
        vm.registers[2] = Value::Int(7);

        let code = vec![
            RegInstruction::encode_abc(RegOpCode::Call, 0, 2, 1),
            RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0),
        ];

        let result = vm.run(&code).unwrap();
        assert_eq!(result, Value::Int(13));
    }

    #[test]
    fn test_register_vm_makefunction_table_call() {
        use aipo_bytecode::RegCompiledFunction;
        // double: R[0] = R[0] + R[0]; return R[0]
        let double = RegCompiledFunction {
            name: "double".to_string(),
            arity: 1,
            is_async: false,
            instructions: vec![
                RegInstruction::encode_abc(RegOpCode::Add, 0, 0, 0),
                RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0),
            ],
            constants: vec![],
            num_registers: 1,
            contracts: vec![],
        };
        let mut vm = RegVm::new();
        vm.functions = vec![double];

        let code = vec![
            RegInstruction::encode_abx(RegOpCode::MakeFunction, 0, 0),
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 1, 21),
            RegInstruction::encode_abc(RegOpCode::Call, 0, 1, 1),
            RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0),
        ];

        let result = vm.run(&code).unwrap();
        assert_eq!(result, Value::Int(42));
    }

    #[test]
    fn test_register_vm_call_depth_limit() {
        use aipo_bytecode::RegCompiledFunction;
        // recurse: R[1] = R[0]; call R[1] with 1 arg; return R[1]
        let recurse = RegCompiledFunction {
            name: "recurse".to_string(),
            arity: 1,
            is_async: false,
            instructions: vec![
                RegInstruction::encode_abx(RegOpCode::MakeFunction, 1, 0),
                RegInstruction::encode_abc(RegOpCode::Move, 2, 0, 0),
                RegInstruction::encode_abc(RegOpCode::Call, 1, 1, 1),
                RegInstruction::encode_abc(RegOpCode::Return, 1, 0, 0),
            ],
            constants: vec![],
            num_registers: 3,
            contracts: vec![],
        };
        let mut vm = RegVm::new();
        vm.max_call_depth = 8;
        vm.functions = vec![recurse];
        vm.registers[0] = Value::Function {
            entry_ip: 0,
            arity: 1,
            is_async: false,
        };
        vm.registers[1] = Value::Int(0);

        let code = vec![
            RegInstruction::encode_abc(RegOpCode::Call, 0, 1, 1),
            RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0),
        ];

        let err = vm.run(&code).unwrap_err();
        assert!(matches!(err, VmFault::CorruptedBytecode { .. }));
    }

    #[test]
    fn test_register_vm_new_list_and_len() {
        let mut vm = RegVm::new();
        let code = vec![
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 1, 10),
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 2, 20),
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 3, 30),
            // R[0] = [R[1], R[2], R[3]]
            RegInstruction::encode_abc(RegOpCode::NewList, 0, 1, 3),
            // R[4] = len(R[0])
            RegInstruction::encode_abc(RegOpCode::Len, 4, 0, 0),
            RegInstruction::encode_abc(RegOpCode::Return, 4, 0, 0),
        ];
        let result = vm.run(&code).unwrap();
        assert_eq!(result, Value::Int(3));
    }

    #[test]
    fn test_register_vm_new_dict() {
        let mut vm = RegVm::new();
        vm.constants = vec![
            Value::String(Rc::new("alpha".to_string())),
            Value::String(Rc::new("beta".to_string())),
        ];
        let code = vec![
            RegInstruction::encode_abx(RegOpCode::LoadConst, 1, 0),
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 2, 100),
            RegInstruction::encode_abx(RegOpCode::LoadConst, 3, 1),
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 4, 200),
            // R[0] = { R[1]: R[2], R[3]: R[4] }
            RegInstruction::encode_abc(RegOpCode::NewDict, 0, 1, 2),
            // GetIndex: R[5] = R[0][R[1]]
            RegInstruction::encode_abc(RegOpCode::GetIndex, 5, 0, 1),
            RegInstruction::encode_abc(RegOpCode::Return, 5, 0, 0),
        ];
        let result = vm.run(&code).unwrap();
        assert_eq!(result, Value::Int(100));
    }

    #[test]
    fn test_register_vm_new_struct_and_clone() {
        let mut vm = RegVm::new();
        vm.constants = vec![
            Value::String(Rc::new("Point".to_string())),
            Value::String(Rc::new("x".to_string())),
            Value::String(Rc::new("y".to_string())),
        ];
        vm.struct_defs.insert(
            "Point".to_string(),
            vec![("x".to_string(), true), ("y".to_string(), false)],
        );

        let code = vec![
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 0, 10),
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 1, 20),
            // R[0] = Point{ x: R[0], y: R[1] }
            RegInstruction::encode_abc(RegOpCode::NewStruct, 0, 0, 2),
            // Clone R[0] into R[1]
            RegInstruction::encode_abc(RegOpCode::CloneStruct, 1, 0, 0),
            // GetField 'x' from R[1] into R[2]
            RegInstruction::encode_abx(RegOpCode::GetField, 1, 1),
            RegInstruction::encode_abc(RegOpCode::Return, 1, 0, 0),
        ];
        let result = vm.run(&code).unwrap();
        assert_eq!(result, Value::Int(10));
    }

    #[test]
    fn test_register_vm_range_and_variant() {
        let mut vm = RegVm::new();
        vm.constants = vec![
            Value::String(Rc::new("Option.Some".to_string())),
            Value::String(Rc::new("val".to_string())),
        ];
        vm.struct_defs
            .insert("Option.Some".to_string(), vec![("val".to_string(), true)]);

        let code = vec![
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 1, 5),
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 2, 10),
            // R[0] = 5..10
            RegInstruction::encode_abc(RegOpCode::Range, 0, 1, 2),
            // R[3] = len(R[0])
            RegInstruction::encode_abc(RegOpCode::Len, 3, 0, 0),
            // Construct Option.Some
            RegInstruction::encode_abc(RegOpCode::NewStruct, 1, 0, 1),
            // IsVariant check
            RegInstruction::encode_abx(RegOpCode::IsVariant, 1, 0),
            RegInstruction::encode_abc(RegOpCode::Return, 1, 0, 0),
        ];
        let result = vm.run(&code).unwrap();
        assert_eq!(result, Value::Bool(true));
    }

    #[test]
    fn test_register_vm_closure_captures_upvalue() {
        // make_adder(base) captures `base` into a closure and returns it.
        let make_adder = RegCompiledFunction {
            name: "make_adder".to_string(),
            arity: 1,
            is_async: false,
            instructions: vec![
                RegInstruction::encode_abc(RegOpCode::Move, 1, 0, 0), // R[1] = base
                RegInstruction::encode_abc(RegOpCode::MakeClosure, 1, 1, 1), // closure#1
                RegInstruction::encode_abc(RegOpCode::Return, 1, 0, 0),
            ],
            constants: vec![],
            num_registers: 2,
            contracts: vec![],
        };
        // add_to(x) returns upvalue[0] + x.
        let add_to = RegCompiledFunction {
            name: "add_to".to_string(),
            arity: 1,
            is_async: false,
            instructions: vec![
                RegInstruction::encode_abx(RegOpCode::GetUpvalue, 1, 0),
                RegInstruction::encode_abc(RegOpCode::Add, 0, 1, 0),
                RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0),
            ],
            constants: vec![],
            num_registers: 2,
            contracts: vec![],
        };
        let mut vm = RegVm::new();
        vm.functions = vec![make_adder, add_to];

        let code = vec![
            RegInstruction::encode_abx(RegOpCode::MakeFunction, 10, 0),
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 11, 10),
            RegInstruction::encode_abc(RegOpCode::Call, 10, 1, 1),
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 11, 5),
            RegInstruction::encode_abc(RegOpCode::Call, 10, 1, 1),
            RegInstruction::encode_abc(RegOpCode::Return, 10, 0, 0),
        ];

        assert_eq!(vm.run(&code).unwrap(), Value::Int(15));
    }

    #[test]
    fn test_register_vm_fill_self_capture() {
        let target = RegCompiledFunction {
            name: "target".to_string(),
            arity: 0,
            is_async: false,
            instructions: vec![RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0)],
            constants: vec![],
            num_registers: 1,
            contracts: vec![],
        };
        let mut vm = RegVm::new();
        vm.functions = vec![target];
        let code = vec![
            RegInstruction::encode_abc(RegOpCode::LoadUnset, 0, 0, 0),
            RegInstruction::encode_abc(RegOpCode::MakeClosure, 0, 0, 1),
            RegInstruction::encode_abc(RegOpCode::FillSelfCapture, 0, 0, 0),
            RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0),
        ];
        let result = vm.run(&code).unwrap();
        let Value::Closure(closure) = result else {
            panic!("expected a closure, got {result:?}");
        };
        let Value::Closure(inner) = closure.upvalues[0].borrow().clone() else {
            panic!("self capture was not filled with the closure");
        };
        assert!(Rc::ptr_eq(&inner, &closure));
    }

    #[test]
    fn test_register_vm_mutation_during_iteration_faults() {
        let mut vm = RegVm::new();
        let code = vec![
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 1, 1),
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 2, 2),
            RegInstruction::encode_abc(RegOpCode::NewList, 0, 1, 2),
            RegInstruction::encode_abc(RegOpCode::IterGuard, 0, 0, 0),
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 3, 0),
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 4, 99),
            RegInstruction::encode_abc(RegOpCode::SetIndex, 0, 3, 4),
            RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0),
        ];
        let err = vm.run(&code).unwrap_err();
        assert!(matches!(err, VmFault::MutationDuringIteration));
    }

    #[test]
    fn test_register_vm_seal_struct_locks_fixed_fields() {
        let mut vm = RegVm::new();
        vm.struct_defs.insert(
            "Point".to_string(),
            vec![("x".to_string(), true), ("y".to_string(), false)],
        );
        vm.constants = vec![
            Value::String(Rc::new("Point".to_string())),
            Value::String(Rc::new("x".to_string())),
        ];
        let code = vec![
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 0, 1),
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 1, 2),
            RegInstruction::encode_abc(RegOpCode::NewStruct, 0, 0, 2),
            RegInstruction::encode_abc(RegOpCode::SealStruct, 0, 0, 0),
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 2, 5),
            RegInstruction::encode_abc(RegOpCode::SetField, 0, 1, 2),
            RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0),
        ];
        let err = vm.run(&code).unwrap_err();
        assert!(matches!(err, VmFault::FixedFieldMutation { .. }));
    }

    #[test]
    fn test_register_vm_assert_invariant_faults() {
        let mut vm = RegVm::new();
        vm.constants = vec![Value::String(Rc::new("Point".to_string()))];
        let code = vec![
            RegInstruction::encode_abc(RegOpCode::LoadBool, 0, 0, 0),
            RegInstruction::encode_abx(RegOpCode::AssertInvariant, 0, 0),
            RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0),
        ];
        let err = vm.run(&code).unwrap_err();
        assert!(
            matches!(err, VmFault::InvariantViolation { ref type_name, .. } if type_name == "Point")
        );
    }

    #[test]
    fn test_register_vm_instruction_budget_halts_infinite_loop() {
        let mut vm = RegVm::new();
        // `pc = 1 - 1 = 0`: jumps to itself forever.
        let code = vec![RegInstruction::encode_asbx(RegOpCode::Jump, 0, -1)];
        vm.set_max_instructions(Some(50));
        let err = vm.run(&code).unwrap_err();
        match err {
            VmFault::Overflow { details } => {
                assert!(
                    details.contains("instruction limit of 50 reached"),
                    "{details}"
                );
            }
            other => panic!("expected VmFault::Overflow, got {other:?}"),
        }
        assert!(vm.instruction_count() > 50);
    }

    #[test]
    fn test_register_vm_budget_counts_across_nested_calls() {
        // The budget is a session counter, so it must also bound work inside nested calls.
        let recurse = RegCompiledFunction {
            name: "recurse".to_string(),
            arity: 1,
            is_async: false,
            instructions: vec![
                RegInstruction::encode_abx(RegOpCode::MakeFunction, 1, 0),
                RegInstruction::encode_abc(RegOpCode::Move, 2, 0, 0),
                RegInstruction::encode_abc(RegOpCode::Call, 1, 1, 1),
                RegInstruction::encode_abc(RegOpCode::Return, 1, 0, 0),
            ],
            constants: vec![],
            num_registers: 3,
            contracts: vec![],
        };
        let mut vm = RegVm::new();
        vm.functions = vec![recurse];
        vm.registers[0] = Value::Function {
            entry_ip: 0,
            arity: 1,
            is_async: false,
        };
        vm.set_max_instructions(Some(40));
        let code = vec![
            RegInstruction::encode_abc(RegOpCode::Call, 0, 1, 1),
            RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0),
        ];
        let err = vm.run(&code).unwrap_err();
        assert!(matches!(err, VmFault::Overflow { .. }));
    }

    #[test]
    fn test_register_vm_assert_contract_faults() {
        let mut vm = RegVm::new();
        vm.contracts = vec![RegContract {
            type_name: "String".to_string(),
            nullable: false,
            position: "parameter `name`".to_string(),
            operations: vec![],
        }];
        let code = vec![
            RegInstruction::encode_asbx(RegOpCode::LoadInt, 0, 1),
            RegInstruction::encode_abx(RegOpCode::AssertContract, 0, 0),
            RegInstruction::encode_abc(RegOpCode::Return, 0, 0, 0),
        ];
        let err = vm.run(&code).unwrap_err();
        assert!(matches!(err, VmFault::ContractViolation { .. }));
    }
}
