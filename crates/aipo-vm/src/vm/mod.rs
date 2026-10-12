//! Stack-based Virtual Machine execution engine for Aipo.

use crate::fault::{VmError, VmFault};
use crate::frame::{CallFrame, HandlerFrame};
use crate::host::HostContext;
use crate::value::{GroupId, StructInstance, TaskId, Value, check_finite_float, check_safe_int};
use aipo_bytecode::BytecodeModule;
use aipo_bytecode::opcode::Constant;
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::rc::Rc;

mod call;
mod contract;
mod dispatch;
mod failure;
pub(crate) mod helpers;
mod journal;
mod method;
mod task;

/// Receiver-first native method implementation.
pub type MethodNative = fn(&Value, &[Value]) -> Result<Value, VmFault>;

/// Native callback that receives the VM context and the call arguments.
pub type HostNativeCallback = fn(&mut Vm, &[Value]) -> Result<Value, VmError>;

/// A typed host-native registration entry.
#[derive(Clone, Copy)]
pub struct HostNative {
    /// Expected number of call arguments.
    pub arity: usize,
    /// Callback executed by the VM.
    pub callback: HostNativeCallback,
    /// Whether the callback runs in a hosted task.
    pub is_async: bool,
}

/// Descriptive alias for [`HostNative`].
pub type HostNativeEntry = HostNative;

impl HostNative {
    /// Creates a typed host-native entry.
    #[must_use]
    pub const fn new(arity: usize, callback: HostNativeCallback, is_async: bool) -> Self {
        Self {
            arity,
            callback,
            is_async,
        }
    }
}

/// Execution mode for canonical unit tests (`test("name") do ... end`).
#[derive(Debug, Clone, Default)]
pub enum TestMode {
    /// Tests execute their bodies sequentially on discovery (default, for `aipo run`).
    #[default]
    Disabled,
    /// Discovers test names without executing their bodies.
    Discover(Rc<RefCell<Vec<String>>>),
    /// Executes only the matching test in this isolated VM instance.
    Execute {
        /// Target test name to execute.
        target: String,
        /// Flag set to true if the targeted test was encountered and dispatched.
        ran: Rc<Cell<bool>>,
    },
}

/// Methods that structurally mutate their receiver, so the VM can enforce the
/// canonical `MutationDuringIteration` fault.
pub(crate) const MUTATING_METHODS: [&str; 6] = [
    "add",
    "insert",
    "remove",
    "remove_at",
    "remove_last",
    "clear",
];

/// Type alias for struct invariant validation functions.
pub type InvariantValidator = Box<dyn Fn(&StructInstance) -> Result<(), String>>;

/// One provisional, invariant-protected field assignment.
///
/// Canon makes a guarded update follow `candidate -> provisional apply -> verify -> commit`,
/// so the value the field held when the frame started mutating it is kept until a stable
/// mutable boundary verifies the instance.
#[derive(Debug, Clone)]
struct MutationEntry {
    /// Instance whose direct field was assigned.
    instance: Rc<RefCell<StructInstance>>,
    /// Name of the assigned field.
    field: String,
    /// Value the field held when the frame first mutated it.
    previous: Value,
}

/// Captured upvalue cells of one call frame.
///
/// `None` marks a plain call; a closure call carries the cells it closed over, shared by
/// reference so that writes through an upvalue are visible to every holder.
pub type UpvalueFrame = Option<Rc<Vec<Rc<RefCell<Value>>>>>;

#[allow(missing_docs)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VmMetrics {
    pub instructions: u64,
    pub calls: u64,
    pub function_calls: u64,
    pub native_calls: u64,
    pub bound_method_calls: u64,
    pub field_lookups: u64,
    pub field_hits: u64,
    pub field_cache_hits: u64,
    pub field_cache_misses: u64,
    pub global_lookups: u64,
    pub global_hits: u64,
    pub constant_loads: u64,
    pub local_clones: u64,
    pub call_argument_copies: u64,
}

#[derive(Debug, Clone, Copy)]
enum FieldLookup {
    Missing,
    Field(usize),
}

/// Virtual Machine executing Aipo bytecode.
pub struct Vm {
    /// Operand stack.
    pub stack: Vec<Value>,
    /// Call frames stack.
    pub frames: Vec<CallFrame>,
    /// Cached stack base of the current call frame; zero when no frame is active.
    frame_base: usize,
    /// Active recovery handler frames stack.
    pub handlers: Vec<HandlerFrame>,
    /// Global variable environment.
    pub globals: HashMap<String, Value>,
    global_slots: Vec<Option<Value>>,
    global_name_slots: HashMap<String, usize>,
    /// Instruction pointer within current code buffer.
    pub ip: usize,
    /// Registered struct definitions: type name -> [(field_name, is_fixed)].
    pub struct_defs: HashMap<String, Vec<(String, bool)>>,
    struct_field_indices: HashMap<String, HashMap<String, usize>>,
    field_cache: Vec<Option<(String, FieldLookup)>>,
    /// Registered struct invariant validators: type name -> validator.
    pub struct_invariants: HashMap<String, InvariantValidator>,
    /// Compiled `<Type>.invariant` predicate entry points, resolved from the running module.
    ///
    /// The module records the predicate as a function named `Type.invariant`, so the runtime
    /// reaches it without a second expression evaluator.
    struct_invariant_entries: HashMap<String, usize>,
    /// Provisional field mutations of the active frames, oldest first.
    mutation_journal: Vec<MutationEntry>,
    /// Closures' captured cells, one entry per call frame (`None` for plain calls).
    pub upvalue_frames: Vec<UpvalueFrame>,
    /// Receiver-first native methods registered by the standard library.
    pub method_natives: HashMap<(String, String), (usize, MethodNative)>,
    method_natives_by_type: HashMap<String, HashMap<String, (usize, MethodNative)>>,
    /// Native callbacks that receive the current VM context.
    host_natives: HashMap<String, HostNative>,
    /// User methods registered per struct type: (type, method) -> (entry ip, total arity,
    /// is async).
    pub struct_methods: HashMap<(String, String), (usize, usize, bool)>,
    pub(crate) struct_methods_by_type: HashMap<String, HashMap<String, (usize, usize, bool)>>,
    /// Identities of collections currently under an active `each` iteration (stack order).
    active_iterations: Vec<usize>,
    iteration_revisions: Vec<(Value, Option<u64>, usize)>,
    active_user_functions: Option<std::collections::HashSet<String>>,
    /// Failure that ended the program because nothing was left to handle it.
    ///
    /// Canon makes recoverable `Failure` values propagate automatically; once propagation
    /// escapes the module entry script there is no code left to run, so execution stops and
    /// [`Vm::run`] reports it as an uncaught failure.
    halted_with: Option<Value>,
    /// Maximum allowed operand stack depth.
    pub max_stack_depth: usize,
    constant_values: Vec<Value>,
    metrics_enabled: bool,
    metrics: VmMetrics,
    /// Scheduler-managed task states, including the suspended main task under id 0.
    tasks: HashMap<TaskId, task::TaskState>,
    /// Runnable tasks in FIFO order; sleepers stay queued until their deadline passes.
    run_queue: VecDeque<TaskId>,
    /// Tasks waiting on a task or join completing: target task id -> waiters.
    waiters: HashMap<TaskId, Vec<TaskId>>,
    /// Active structured joins (`all`, `race`, `timeout`, group waits).
    joins: HashMap<task::JoinId, task::JoinState>,
    /// Structured-concurrency groups created by `task.group()`.
    groups: HashMap<GroupId, task::GroupState>,
    /// Task currently loaded on the machine (`None` while the never-suspended
    /// main script runs on the bare machine).
    current: Option<TaskId>,
    /// Host callback and arguments for the currently loaded hosted task.
    host_task: Option<task::HostTask>,
    /// Next fresh task, join, and group identifiers.
    next_task: TaskId,
    next_join: task::JoinId,
    next_group: GroupId,
    /// Virtual clock in ticks. Time advances only while tasks sleep or wait on
    /// deadlines; pure computation never moves the clock, so scheduling is
    /// fully deterministic for a fixed program input.
    tick: u64,
    /// Outcome of the module entry script once it completes.
    main_outcome: Option<task::TaskOutcome>,
    /// Reentrancy depth of [`Vm::invoke`]: blocking operations fault instead of
    /// suspending while positive, because the host Rust stack cannot resume.
    invoke_depth: usize,
    /// Host services this profile granted, plus the objects behind their handles and the
    /// scopes a host callback opened.
    host: HostContext,
    /// Byte length of the triggering call instruction (1 for Call0..4, 2 for Call).
    pub(crate) call_inst_len: u8,
    /// Test execution mode for canonical unit testing.
    pub test_mode: TestMode,
    /// Maximum allowed instructions before execution faults with budget overflow.
    pub max_instructions: Option<u64>,
    /// Interval in instructions between fuel budget checks (default: 64).
    pub meter_interval: u64,
}

/// Default periodic instruction metering interval (64 instructions).
pub const DEFAULT_METER_INTERVAL: u64 = 64;

/// Snapshot of VM global environment and type definitions, enabling atomic rollback on load failure.
#[derive(Clone, Debug)]
pub struct VmDefinitionSnapshot {
    scheduler: task::SchedulerSnapshot,
    active_user_functions: Option<std::collections::HashSet<String>>,
    heap: crate::HeapSnapshot,
    globals: HashMap<String, Value>,
    global_slots: Vec<Option<Value>>,
    global_name_slots: HashMap<String, usize>,
    constant_values: Vec<Value>,
    struct_invariant_entries: HashMap<String, usize>,
    struct_defs: HashMap<String, Vec<(String, bool)>>,
    struct_field_indices: HashMap<String, HashMap<String, usize>>,
    struct_methods: HashMap<(String, String), (usize, usize, bool)>,
    struct_methods_by_type: HashMap<String, HashMap<String, (usize, usize, bool)>>,
}

/// Execution caches bound to a specific module during `step` and `invoke`.
#[derive(Clone, Debug, Default)]
pub struct VmModuleExecutionState {
    field_cache: Vec<Option<(String, FieldLookup)>>,
    global_slots: Vec<Option<Value>>,
    global_name_slots: HashMap<String, usize>,
    constant_values: Vec<Value>,
    struct_invariant_entries: HashMap<String, usize>,
}

impl Default for Vm {
    fn default() -> Self {
        Self::new()
    }
}

impl Vm {
    /// Constructs a new virtual machine instance with default limits.
    #[must_use]
    pub fn new() -> Self {
        Self {
            stack: Vec::with_capacity(128),
            frames: Vec::with_capacity(16),
            frame_base: 0,
            handlers: Vec::new(),
            globals: HashMap::new(),
            global_slots: Vec::new(),
            global_name_slots: HashMap::new(),
            ip: 0,
            struct_defs: HashMap::new(),
            struct_field_indices: HashMap::new(),
            field_cache: Vec::new(),
            struct_invariants: HashMap::new(),
            struct_invariant_entries: HashMap::new(),
            mutation_journal: Vec::new(),
            upvalue_frames: Vec::new(),
            method_natives: HashMap::new(),
            method_natives_by_type: HashMap::new(),
            host_natives: HashMap::new(),
            struct_methods: HashMap::new(),
            struct_methods_by_type: HashMap::new(),
            active_iterations: Vec::new(),
            iteration_revisions: Vec::new(),
            active_user_functions: None,
            halted_with: None,
            max_stack_depth: 1024,
            constant_values: Vec::new(),
            metrics_enabled: false,
            metrics: VmMetrics::default(),
            tasks: HashMap::new(),
            run_queue: VecDeque::new(),
            waiters: HashMap::new(),
            joins: HashMap::new(),
            groups: HashMap::new(),
            current: None,
            host_task: None,
            next_task: task::MAIN_TASK + 1,
            next_join: 1,
            next_group: 1,
            tick: 0,
            main_outcome: None,
            invoke_depth: 0,
            host: HostContext::denied(),
            call_inst_len: 2,
            test_mode: TestMode::Disabled,
            max_instructions: None,
            meter_interval: DEFAULT_METER_INTERVAL,
        }
    }

    /// Sets the test execution mode for this VM instance.
    pub fn set_test_mode(&mut self, mode: TestMode) {
        self.test_mode = mode;
    }

    /// Returns the current test execution mode.
    #[must_use]
    pub fn test_mode(&self) -> &TestMode {
        &self.test_mode
    }

    /// The host services of this run, for the embedder that owns them.
    ///
    /// Capabilities are granted here, host values are handed out here, and a scoped callback
    /// opens and closes its scope here — the VM only ever *checks* against what the host
    /// decided, which keeps the trust boundary in one place.
    pub fn host_context(&mut self) -> &mut HostContext {
        &mut self.host
    }

    /// The host services of this run, read-only.
    #[must_use]
    pub fn host(&self) -> &HostContext {
        &self.host
    }

    /// Takes the unhandled failure that halted execution, if any.
    pub fn take_halted_failure(&mut self) -> Option<Value> {
        self.halted_with.take()
    }

    #[allow(missing_docs)]
    pub fn enable_metrics(&mut self) {
        self.metrics_enabled = true;
        let consumed = self.metrics.instructions;
        self.metrics = VmMetrics::default();
        if self.max_instructions.is_some() {
            self.metrics.instructions = consumed;
        }
    }

    #[allow(missing_docs)]
    #[must_use]
    pub fn metrics(&self) -> VmMetrics {
        self.metrics
    }

    /// Configures the maximum instruction execution budget.
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

    /// Returns the number of instructions executed since the last reset or run; budgeted runs retain consumption until explicitly reset.
    #[must_use]
    pub fn instruction_count(&self) -> u64 {
        self.metrics.instructions
    }

    /// Resets the executed instruction counter to zero.
    pub fn reset_instruction_count(&mut self) {
        self.metrics.instructions = 0;
    }

    /// Creates a snapshot of definition tables and global variables.
    #[must_use]
    pub fn snapshot_definitions(&self) -> VmDefinitionSnapshot {
        let scheduler = self.snapshot_scheduler();
        let mut heap =
            crate::HeapSnapshot::capture(self.globals.values().cloned().chain(scheduler.roots()));
        heap.capture_cells(scheduler.cells());
        VmDefinitionSnapshot {
            scheduler,
            active_user_functions: self.active_user_functions.clone(),
            heap,
            globals: self.globals.clone(),
            global_slots: self.global_slots.clone(),
            global_name_slots: self.global_name_slots.clone(),
            constant_values: self.constant_values.clone(),
            struct_invariant_entries: self.struct_invariant_entries.clone(),
            struct_defs: self.struct_defs.clone(),
            struct_field_indices: self.struct_field_indices.clone(),
            struct_methods: self.struct_methods.clone(),
            struct_methods_by_type: self.struct_methods_by_type.clone(),
        }
    }

    /// Restores definitions and globals to a prior snapshot, cleaning execution residue.
    pub fn restore_definitions(&mut self, snapshot: VmDefinitionSnapshot) {
        snapshot.heap.restore();
        self.active_user_functions = snapshot.active_user_functions;
        self.globals = snapshot.globals;
        self.global_slots = snapshot.global_slots;
        self.global_name_slots = snapshot.global_name_slots;
        self.constant_values = snapshot.constant_values;
        self.struct_invariant_entries = snapshot.struct_invariant_entries;
        self.struct_defs = snapshot.struct_defs;
        self.struct_field_indices = snapshot.struct_field_indices;
        self.field_cache.clear();
        self.struct_methods = snapshot.struct_methods;
        self.struct_methods_by_type = snapshot.struct_methods_by_type;
        self.stack.clear();
        self.frames.clear();
        self.frame_base = 0;
        self.handlers.clear();
        self.ip = 0;
        self.halted_with = None;
        self.active_iterations.clear();
        self.iteration_revisions.clear();
        self.mutation_journal.clear();
        self.upvalue_frames.clear();
        self.current = None;
        self.host_task = None;
        self.main_outcome = None;
        self.restore_scheduler(snapshot.scheduler);
    }

    /// Replaces compiled user method tables for a complete session declaration catalog.
    /// Native host methods use separate tables and remain registered.
    pub fn replace_user_methods(&mut self, module: &BytecodeModule) {
        self.active_user_functions = Some(
            module
                .functions
                .iter()
                .map(|function| function.name.clone())
                .collect(),
        );
        self.struct_methods.clear();
        self.struct_methods_by_type.clear();
        self.struct_invariant_entries.clear();
        self.field_cache.clear();
    }

    /// Removes an obsolete public definition during a staged reload.
    pub fn remove_definition(&mut self, name: &str) {
        self.globals.remove(name);
        self.global_name_slots.remove(name);
        self.struct_defs.remove(name);
        self.struct_field_indices.remove(name);
        self.struct_invariant_entries.remove(name);
        self.struct_methods
            .retain(|(ty, method), _| ty != name && format!("{ty}.{method}") != name);
        self.struct_methods_by_type.remove(name);
        self.field_cache.clear();
    }
    /// Requires explicit migration when retained instances have incompatible field layouts.
    pub fn validate_guest_layouts(&self) -> Result<(), String> {
        let scheduler = self.snapshot_scheduler();
        let mut heap =
            crate::HeapSnapshot::capture(self.globals.values().cloned().chain(scheduler.roots()));
        heap.capture_cells(scheduler.cells());
        heap.validate_layouts(&self.struct_defs)
    }

    /// Prepares module-scoped caches (constants, global slots, field cache, struct invariants)
    /// for executing bytecode belonging to `module`.
    ///
    /// # Errors
    /// Returns `VmFault` if constant validation fails.
    pub fn prepare_module_execution(&mut self, module: &BytecodeModule) -> Result<(), VmFault> {
        self.field_cache.clear();
        self.field_cache.resize(module.names.len(), None);

        self.global_slots = vec![None; module.names.len()];
        self.global_name_slots = module
            .names
            .iter()
            .enumerate()
            .map(|(index, name)| (name.clone(), index))
            .collect();
        for s in &module.structs {
            if let Some((parent_enum, _)) = s.name.split_once('.') {
                let enum_str = parent_enum.to_string();
                self.globals
                    .entry(enum_str.clone())
                    .or_insert_with(|| Value::UserType(Rc::new(enum_str)));
            } else {
                self.globals
                    .entry(s.name.clone())
                    .or_insert_with(|| Value::UserType(Rc::new(s.name.clone())));
            }
        }
        for (index, name) in module.names.iter().enumerate() {
            self.global_slots[index] = self.globals.get(name).cloned();
        }
        self.constant_values = module
            .constants
            .iter()
            .map(|constant| match constant {
                Constant::Nil => Ok(Value::None),
                Constant::Bool(value) => Ok(Value::Bool(*value)),
                Constant::Int(value) => check_safe_int(*value).map(Value::Int),
                Constant::Float(value) => check_finite_float(*value).map(Value::Float),
                Constant::String(value) => Ok(Value::String(Rc::new(value.clone()))),
            })
            .collect::<Result<Vec<_>, VmFault>>()?;

        self.struct_invariant_entries.clear();
        for function in &module.functions {
            if self
                .active_user_functions
                .as_ref()
                .is_some_and(|names| !names.contains(&function.name))
            {
                continue;
            }
            if let Some((type_name, method)) = function.name.split_once('.')
                && method == "invariant"
            {
                self.struct_invariant_entries
                    .insert(type_name.to_string(), function.entry_ip);
            }
        }
        Ok(())
    }

    /// Saves the current module execution caches and prepares new ones for `module`.
    ///
    /// # Errors
    /// Returns `VmFault` if constant validation fails.
    pub fn push_module_execution(
        &mut self,
        module: &BytecodeModule,
    ) -> Result<VmModuleExecutionState, VmFault> {
        let prev = VmModuleExecutionState {
            field_cache: std::mem::take(&mut self.field_cache),
            global_slots: std::mem::take(&mut self.global_slots),
            global_name_slots: std::mem::take(&mut self.global_name_slots),
            constant_values: std::mem::take(&mut self.constant_values),
            struct_invariant_entries: std::mem::take(&mut self.struct_invariant_entries),
        };
        if let Err(err) = self.prepare_module_execution(module) {
            self.pop_module_execution(prev);
            return Err(err);
        }
        Ok(prev)
    }

    /// Restores previously saved module execution caches.
    pub fn pop_module_execution(&mut self, prev: VmModuleExecutionState) {
        self.field_cache = prev.field_cache;
        self.global_slots = prev.global_slots;
        self.global_name_slots = prev.global_name_slots;
        self.constant_values = prev.constant_values;
        self.struct_invariant_entries = prev.struct_invariant_entries;
    }

    #[inline]
    pub(super) fn refresh_frame_base(&mut self) {
        self.frame_base = self.frames.last().map_or(0, |frame| frame.stack_base);
    }

    fn lookup_struct_field(
        &mut self,
        name_idx: usize,
        type_name: &str,
        field_name: &str,
    ) -> FieldLookup {
        if let Some(Some((cached_type, result))) = self.field_cache.get(name_idx)
            && cached_type == type_name
        {
            if self.metrics_enabled {
                self.metrics.field_cache_hits = self.metrics.field_cache_hits.saturating_add(1);
            }
            return *result;
        }
        if self.metrics_enabled {
            self.metrics.field_cache_misses = self.metrics.field_cache_misses.saturating_add(1);
        }
        let result = self
            .struct_field_indices
            .get(type_name)
            .and_then(|fields| fields.get(field_name))
            .copied()
            .map_or(FieldLookup::Missing, FieldLookup::Field);
        if let Some(slot) = self.field_cache.get_mut(name_idx) {
            *slot = Some((type_name.to_string(), result));
        }
        result
    }

    /// Enforces the scoped-escape rule at one heap-publication point.
    ///
    /// The check is skipped while no scope has closed, so the ordinary path pays a single
    /// boolean test: the rule only becomes reachable once a host callback ended holding
    /// bindings it minted, which is exactly when a leak is possible. The site is built only
    /// when the check actually runs.
    ///
    /// # Errors
    ///
    /// [`VmFault::ScopeEscape`] when a value about to become heap-reachable carries a handle
    /// from a scope that has closed.
    pub(crate) fn publish_check(
        &self,
        value: &Value,
        site: impl FnOnce() -> String,
    ) -> Result<(), VmError> {
        if !self.host.has_escapes() {
            return Ok(());
        }
        let site = site();
        self.host
            .ensure_publishable(value, &site)
            .map_err(VmError::from)
    }

    /// Registers a receiver-first native method for a type (for example `String.len`).
    pub fn register_method_native(
        &mut self,
        type_name: &str,
        method: &str,
        arity: usize,
        func: MethodNative,
    ) {
        self.method_natives
            .insert((type_name.to_string(), method.to_string()), (arity, func));
        self.method_natives_by_type
            .entry(type_name.to_string())
            .or_default()
            .insert(method.to_string(), (arity, func));
    }

    /// Registers a synchronous native callback that receives the current VM context.
    pub fn register_host_native(
        &mut self,
        name: impl Into<String>,
        arity: usize,
        callback: HostNativeCallback,
    ) {
        self.register_host_entry(name, HostNative::new(arity, callback, false));
    }

    /// Registers an asynchronous native callback that receives the current VM context.
    ///
    /// Calling the native returns a task immediately. The callback runs only when the scheduler
    /// loads that task.
    pub fn register_host_async_native(
        &mut self,
        name: impl Into<String>,
        arity: usize,
        callback: HostNativeCallback,
    ) {
        self.register_host_entry(name, HostNative::new(arity, callback, true));
    }

    /// Registers a complete typed host-native entry.
    pub fn register_host_entry(&mut self, name: impl Into<String>, entry: HostNative) {
        let name = name.into();
        if name.is_empty() || name.trim() != name {
            return;
        }
        self.host_natives.insert(name, entry);
    }

    /// Returns a registered host-native entry.
    #[must_use]
    pub fn host_native(&self, name: &str) -> Option<HostNative> {
        self.host_natives.get(name).copied()
    }

    /// Creates a task that runs a host callback when scheduled.
    ///
    /// This is the explicit counterpart to [`Self::register_host_async_native`] and is useful to
    /// host adapters that need to create a hosted task without exposing a bytecode callable.
    pub fn spawn_host_task(
        &mut self,
        callback: HostNativeCallback,
        args: Vec<Value>,
    ) -> Result<TaskId, VmError> {
        self.spawn_host_task_state(callback, args)
    }

    /// Creates a task value around a host callback.
    pub fn create_host_task(
        &mut self,
        callback: HostNativeCallback,
        args: Vec<Value>,
    ) -> Result<Value, VmError> {
        self.spawn_host_task(callback, args).map(Value::Task)
    }

    /// Registers a user-defined method of a struct type.
    ///
    /// `is_async` mirrors the method's declaration: an `async fn` method called through the
    /// receiver yields a `Task` instead of running, exactly like a free `async fn`.
    pub fn register_struct_method(
        &mut self,
        type_name: &str,
        method: &str,
        entry_ip: usize,
        total_arity: usize,
        is_async: bool,
    ) {
        self.struct_methods.insert(
            (type_name.to_string(), method.to_string()),
            (entry_ip, total_arity, is_async),
        );
        self.struct_methods_by_type
            .entry(type_name.to_string())
            .or_default()
            .insert(method.to_string(), (entry_ip, total_arity, is_async));
    }

    /// Looks up a struct method by type name and method name.
    ///
    /// A payload variant is stored as a struct named `Enum.Variant`, but canon
    /// declares its methods on the enum (`Enum:metodo`), so a variant receiver
    /// falls back to the parent type. This mirrors `TypeIs` and the written
    /// contract check, which both accept a variant where the enum is named.
    #[inline]
    #[must_use]
    pub fn lookup_struct_method(
        &self,
        type_name: &str,
        method: &str,
    ) -> Option<(usize, usize, bool)> {
        if let Some(found) = self
            .struct_methods_by_type
            .get(type_name)
            .and_then(|methods| methods.get(method))
        {
            return Some(*found);
        }
        let parent = type_name.split_once('.')?.0;
        self.struct_methods_by_type
            .get(parent)?
            .get(method)
            .copied()
    }

    /// Registers a struct type with its field names and `fixed` flags.
    pub fn register_struct(&mut self, type_name: impl Into<String>, fields: Vec<(&str, bool)>) {
        let type_name = type_name.into();
        let field_defs = fields
            .into_iter()
            .map(|(name, fixed)| (name.to_string(), fixed))
            .collect::<Vec<_>>();
        self.struct_field_indices.insert(
            type_name.clone(),
            field_defs
                .iter()
                .enumerate()
                .map(|(index, field)| (field.0.clone(), index))
                .collect(),
        );
        self.field_cache.clear();
        self.struct_defs.insert(type_name, field_defs);
    }

    /// Registers an invariant validator for a struct type.
    pub fn register_struct_invariant<F>(&mut self, type_name: &str, validator: F)
    where
        F: Fn(&StructInstance) -> Result<(), String> + 'static,
    {
        self.struct_invariants
            .insert(type_name.to_string(), Box::new(validator));
    }

    /// Defines or updates a global variable.
    pub fn define_global(&mut self, name: impl Into<String>, value: Value) {
        let name = name.into();
        if let Some(slot) = self.global_name_slots.get(&name).copied() {
            self.global_slots[slot] = Some(value.clone());
        }
        self.globals.insert(name, value);
    }

    /// Retrieves a global variable's value.
    #[must_use]
    pub fn get_global(&self, name: &str) -> Option<&Value> {
        self.globals.get(name)
    }

    /// Pushes a value onto the operand stack.
    ///
    /// # Errors
    /// Returns `VmFault::StackUnderflow` / overflow if stack exceeds limit.
    pub fn push(&mut self, val: Value) -> Result<(), VmFault> {
        if self.stack.len() >= self.max_stack_depth {
            return Err(VmFault::Overflow {
                details: format!(
                    "operand stack exceeded depth limit {} ({} active call frames; deep or unbounded recursion is the usual cause)",
                    self.max_stack_depth,
                    self.frames.len()
                ),
            });
        }
        self.stack.push(val);
        Ok(())
    }

    /// Pops a value from the operand stack.
    ///
    /// # Errors
    /// Returns `VmFault::StackUnderflow` if the stack is empty.
    pub fn pop(&mut self) -> Result<Value, VmFault> {
        self.stack.pop().ok_or(VmFault::StackUnderflow)
    }

    /// Peeks at the top value of the operand stack.
    ///
    /// # Errors
    /// Returns `VmFault::StackUnderflow` if the stack is empty.
    pub fn peek(&self) -> Result<&Value, VmFault> {
        self.stack.last().ok_or(VmFault::StackUnderflow)
    }

    /// Runs a bytecode module from instruction 0 until completion.
    ///
    /// # Errors
    /// Returns `VmError` if a runtime fault occurs or an uncaught failure reaches top level.
    pub fn run(&mut self, module: &BytecodeModule) -> Result<Value, VmError> {
        self.run_at(module, 0)
    }

    /// Runs a verified linked compilation unit from its relocated entry offset.
    /// Previous global values and function bodies remain accessible.
    /// # Errors
    /// Returns a bytecode fault for an invalid offset or a normal execution error.
    pub fn start_at(&mut self, module: &BytecodeModule, entry: usize) -> Result<(), VmError> {
        if entry > module.code.len() {
            return Err(VmFault::CorruptedBytecode {
                offset: entry,
                reason: "entry outside linked module".into(),
            }
            .into());
        }
        let mut cursor = 0;
        while cursor < entry {
            let opcode = aipo_bytecode::OpCode::try_from(module.code[cursor]).map_err(|byte| {
                VmFault::CorruptedBytecode {
                    offset: cursor,
                    reason: format!("unknown opcode {byte}"),
                }
            })?;
            cursor +=
                aipo_bytecode::BytecodeVerifier::instruction_size(opcode, &module.code[cursor..]);
            if cursor > entry {
                return Err(VmFault::CorruptedBytecode {
                    offset: entry,
                    reason: "entry is not an instruction boundary".into(),
                }
                .into());
            }
        }
        self.ip = entry;
        self.stack.clear();
        self.frames.clear();
        self.frame_base = 0;
        self.handlers.clear();
        self.upvalue_frames.clear();
        self.active_iterations.clear();
        self.iteration_revisions.clear();
        self.halted_with = None;
        self.mutation_journal.clear();
        self.tasks.clear();
        self.run_queue.clear();
        self.waiters.clear();
        self.joins.clear();
        self.groups.clear();
        self.current = Some(task::MAIN_TASK);
        self.host_task = None;
        self.next_task = task::MAIN_TASK + 1;
        self.next_join = 1;
        self.next_group = 1;
        self.tick = 0;
        self.main_outcome = None;
        self.invoke_depth = 0;
        let consumed = self.metrics.instructions;
        self.metrics = VmMetrics::default();
        if self.max_instructions.is_some() {
            self.metrics.instructions = consumed;
        }
        self.prepare_module_execution(module)?;

        for function in &module.functions {
            if self
                .active_user_functions
                .as_ref()
                .is_some_and(|names| !names.contains(&function.name))
            {
                continue;
            }
            if let Some((type_name, method)) = function.name.split_once('.')
                && method != "invariant"
            {
                self.register_struct_method(
                    type_name,
                    method,
                    function.entry_ip,
                    function.params,
                    function.is_async,
                );
            }
        }

        Ok(())
    }

    /// Drives one scheduler quantum for debugger and cooperative host integrations.
    /// # Errors
    /// Returns normal runtime faults; suspended guest tasks remain scheduled.
    pub fn debug_step(&mut self, module: &BytecodeModule) -> Result<bool, VmError> {
        if self.main_outcome.is_some() {
            return Ok(true);
        }
        if self.current.is_none() && self.frames.is_empty() {
            self.select_next(module)?;
            if self.main_outcome.is_some() {
                return Ok(true);
            }
        }
        match self.step(module) {
            Err(VmError::Suspended) => Ok(false),
            Err(error) => {
                if self.current.is_some_and(|id| id != task::MAIN_TASK)
                    && self
                        .current
                        .and_then(|id| self.tasks.get(&id))
                        .is_some_and(|state| state.status == task::TaskStatus::Cancelled)
                {
                    self.complete_current(task::TaskOutcome::Cancelled);
                    Ok(self.main_outcome.is_some())
                } else {
                    Err(error)
                }
            }
            Ok(_) => Ok(self.main_outcome.is_some()),
        }
    }

    /// Takes a completed main outcome after cooperative stepping.
    pub fn take_completion(&mut self) -> Option<Result<Value, VmError>> {
        self.main_outcome.take().map(|outcome| match outcome {
            task::TaskOutcome::Ready(value) => Ok(value),
            task::TaskOutcome::Failed(Value::Failure(failure)) => {
                Err(VmError::UncaughtFailure(failure.message.clone()))
            }
            task::TaskOutcome::Failed(value) => Err(VmError::UncaughtFailure(value.to_string())),
            task::TaskOutcome::Cancelled => Err(VmFault::Cancelled {
                details: "main task cancelled".into(),
            }
            .into()),
        })
    }

    /// Runs a linked unit to completion from an explicit byte offset.
    /// # Errors
    /// Returns normal runtime faults or an invalid entry offset.
    pub fn run_at(&mut self, module: &BytecodeModule, entry: usize) -> Result<Value, VmError> {
        self.start_at(module, entry)?;
        self.run_started(module)
    }

    /// Executes a new linked unit while retaining task handles and queued guest work.
    /// Only use with an append-only code image whose previous offsets remain valid.
    /// # Errors
    /// Returns normal runtime faults or an invalid entry offset.
    pub fn run_persistent_at(
        &mut self,
        module: &BytecodeModule,
        entry: usize,
    ) -> Result<Value, VmError> {
        let scheduler = self.snapshot_scheduler();
        self.start_at(module, entry)?;
        self.restore_scheduler(scheduler);
        self.current = Some(task::MAIN_TASK);
        self.run_started(module)
    }

    fn run_started(&mut self, module: &BytecodeModule) -> Result<Value, VmError> {
        while !self.debug_step(module)? {}

        let result = match self.main_outcome.take() {
            Some(task::TaskOutcome::Ready(value)) => value,
            Some(task::TaskOutcome::Failed(failure)) => failure,
            Some(task::TaskOutcome::Cancelled) => {
                return Err(VmFault::Cancelled {
                    details: "main task was cancelled".to_string(),
                }
                .into());
            }
            None => match self.halted_with.take() {
                Some(failure) => failure,
                None => self.stack.pop().unwrap_or(Value::None),
            },
        };
        if let Value::Failure(err) = &result {
            return Err(VmError::UncaughtFailure(err.message.clone()));
        }

        Ok(result)
    }
}
