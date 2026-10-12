//! Safe runtime embedding context for Aipo (ADP-008, ADP-009).

use crate::types::{aipo_handle_t, aipo_host_fn_t, aipo_status_t, aipo_value_t};
use aipo_bytecode::BytecodeModule;
use aipo_diagnostics::Severity;
use aipo_host::{Capability, CapabilitySet, Handle, HostValue};
use aipo_sema::PreludeSurface;
use aipo_source::{Source, SourceId};
use aipo_vm::{Value, Vm, VmError, VmFault};
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::Arc;

thread_local! {
    static CURRENT_RUNTIME: Cell<*mut AipoRuntime> = const { Cell::new(std::ptr::null_mut()) };
    /// Set while a host callback is on the C stack. Any attempt to re-enter the
    /// runtime from inside that callback is refused before a `&mut` is formed.
    static IN_HOST_CALLBACK: Cell<bool> = const { Cell::new(false) };
}

struct CurrentRuntimeGuard(*mut AipoRuntime);

impl CurrentRuntimeGuard {
    fn new(rt: *mut AipoRuntime) -> Self {
        let prev = CURRENT_RUNTIME.get();
        CURRENT_RUNTIME.set(rt);
        Self(prev)
    }
}

impl Drop for CurrentRuntimeGuard {
    fn drop(&mut self) {
        CURRENT_RUNTIME.set(self.0);
    }
}

/// Refuses re-entry while a host callback owns the VM.
///
/// A C host receives a context, not the runtime, so the only way back into the
/// runtime is through a stored pointer. That is still possible, so the dispatcher
/// checks this flag before forming any `&mut` and fails closed.
struct CallbackDepthGuard;

impl CallbackDepthGuard {
    fn acquire() -> Result<Self, VmFault> {
        if IN_HOST_CALLBACK.get() {
            return Err(VmFault::NotCallable {
                type_name: "re-entrant call into a busy runtime".to_string(),
            });
        }
        IN_HOST_CALLBACK.set(true);
        Ok(Self)
    }
}

impl Drop for CallbackDepthGuard {
    fn drop(&mut self) {
        IN_HOST_CALLBACK.set(false);
    }
}

/// Whether a host callback is currently on the C stack.
///
/// Every entrypoint that takes `&mut AipoRuntime` checks this first. While a
/// callback runs, the VM is already mutably borrowed from the dispatcher, so
/// forming a second `&mut` would alias it.
#[must_use]
pub fn in_host_callback() -> bool {
    IN_HOST_CALLBACK.get()
}

/// Registration metadata for a host-provided C function.
#[derive(Clone, Copy)]
pub struct HostFnEntry {
    /// Expected arity.
    pub arity: usize,
    /// Function pointer to C callback.
    pub callback: aipo_host_fn_t,
}

/// Owns a snapshot handed across the C boundary.
///
/// Strings and bytes are copied out of the VM, so a returned `aipo_value_t` never
/// borrows from a value the guest can still mutate. The host releases the snapshot
/// with `aipo_value_release`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SnapshotId(u64);

/// Safe, self-contained Aipo runtime for host embedders.
pub struct AipoRuntime {
    /// Internal VM instance.
    pub vm: Vm,
    /// Granted capabilities for this runtime instance.
    pub capabilities: CapabilitySet,
    /// Loaded bytecode modules by module name.
    pub modules: HashMap<String, Arc<BytecodeModule>>,
    /// Name of the last module loaded on this runtime.
    pub last_loaded_module: Option<String>,
    linked: BytecodeModule,
    module_offsets: HashMap<String, usize>,
    active_execution: Option<Arc<BytecodeModule>>,
    /// Last error or fault message recorded.
    pub last_error: Option<String>,
    /// Owned snapshots for strings and bytes returned across the C boundary.
    snapshots: HashMap<SnapshotId, Box<[u8]>>,
    next_snapshot: u64,
    /// Registered host functions by name.
    pub host_functions: HashMap<String, (HostFnEntry, Option<String>)>,
}

impl Default for AipoRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl AipoRuntime {
    /// Creates a new, isolated runtime with deny-by-default capabilities.
    #[must_use]
    pub fn new() -> Self {
        let mut vm = Vm::new();
        let mut registry = aipo_runtime::NativeRegistry::new();
        aipo_stdlib::register_stdlib(&mut vm, &mut registry);

        Self {
            vm,
            capabilities: CapabilitySet::none(),
            modules: HashMap::new(),
            last_loaded_module: None,
            linked: BytecodeModule::new(),
            module_offsets: HashMap::new(),
            active_execution: None,
            last_error: None,
            snapshots: HashMap::new(),
            next_snapshot: 1,
            host_functions: HashMap::new(),
        }
    }

    /// Copies `data` into a runtime-owned snapshot and returns its address.
    ///
    /// The copy is what makes a returned pointer independent of the value it came
    /// from, and it is length-delimited: an embedded NUL is preserved rather than
    /// replaced by a placeholder whose length the caller would still trust.
    ///
    /// The buffer carries one extra `0` byte after the payload. A C consumer that
    /// reaches for `strlen`, `strcmp` or `printf("%s")` instead of the explicit
    /// `str_len` would otherwise read past the allocation looking for a
    /// terminator — the defect this snapshot mechanism exists to remove. Reported
    /// lengths come from the payload, never from the buffer, so the sentinel is
    /// never counted as data, including when the payload itself ends in `0`.
    pub fn alloc_snapshot(&mut self, data: &[u8]) -> SnapshotId {
        let id = SnapshotId(self.next_snapshot);
        self.next_snapshot = self.next_snapshot.wrapping_add(1);
        let mut buf = Vec::with_capacity(data.len() + 1);
        buf.extend_from_slice(data);
        buf.push(0);
        self.snapshots.insert(id, buf.into_boxed_slice());
        id
    }

    /// Frees a snapshot previously returned to the C boundary.
    pub fn release_snapshot(&mut self, id: SnapshotId) -> bool {
        self.snapshots.remove(&id).is_some()
    }

    /// Frees the snapshot backing `ptr`, if it belongs to this runtime.
    ///
    /// `aipo_value_t` is plain C and carries no owner field, so a released value
    /// is identified by the address it already exposes. Lookup is linear, which
    /// is fine: release is not a hot path and the map is small.
    pub fn release_snapshot_at(&mut self, ptr: *const u8) -> bool {
        if ptr.is_null() {
            return false;
        }
        let id = self
            .snapshots
            .iter()
            .find(|(_, buf)| buf.as_ptr() == ptr)
            .map(|(id, _)| *id);
        id.is_some_and(|id| self.snapshots.remove(&id).is_some())
    }

    /// Builds a closure that snapshots heap payloads for [`aipo_value_t::from_vm_value`].
    pub fn snapshotter(&mut self) -> impl FnMut(&[u8]) -> *const u8 + '_ {
        move |data: &[u8]| {
            let id = self.alloc_snapshot(data);
            self.snapshots
                .get(&id)
                .map_or(std::ptr::null(), |b| b.as_ptr())
        }
    }

    /// Grants a host capability path (e.g. `"clock"`, `"io"`, `"poppy"`).
    pub fn grant(&mut self, capability_name: &str) -> Result<(), String> {
        let cap = Capability::parse(capability_name).map_err(|e| e.to_string())?;
        self.capabilities.grant(cap);
        Ok(())
    }

    /// Revokes a host capability path.
    pub fn revoke(&mut self, capability_name: &str) -> Result<(), String> {
        let cap = Capability::parse(capability_name).map_err(|e| e.to_string())?;
        self.capabilities.revoke(&cap);
        Ok(())
    }

    /// Internal helper that registers definitions and executes top-level code of a compiled module.
    pub fn load_module_internal(
        &mut self,
        name: &str,
        module: BytecodeModule,
    ) -> Result<(), (aipo_status_t, String)> {
        let mut candidate = self.linked.clone();
        let offset = aipo_bytecode::append_unit(&mut candidate, &module)
            .map_err(|errors| (aipo_status_t::AIPO_ERR_DIAGNOSTIC, errors.join("; ")))?;
        let mut catalog = BytecodeModule::new();
        let mut previous: Vec<_> = self
            .modules
            .iter()
            .filter(|(key, _)| key.as_str() != name)
            .collect();
        previous.sort_by_key(|(key, _)| *key);
        for (key, loaded) in previous {
            catalog.structs.extend(loaded.structs.iter().cloned());
            catalog
                .functions
                .extend(loaded.functions.iter().cloned().map(|mut function| {
                    function.entry_ip += self.module_offsets[key];
                    function
                }));
        }
        catalog.structs.extend(module.structs.iter().cloned());
        catalog.functions.extend(module.functions.iter().cloned());
        candidate.structs = catalog.structs.clone();
        // Capture definition snapshot before any module mutations for atomic commit/rollback
        let snapshot = self.vm.snapshot_definitions();
        self.vm.replace_user_methods(&catalog);
        if let Some(old) = self.modules.get(name) {
            for declaration in &old.structs {
                if !catalog
                    .structs
                    .iter()
                    .any(|current| current.name == declaration.name)
                {
                    self.vm.remove_definition(&declaration.name);
                }
            }
            for function in &old.functions {
                if !catalog
                    .functions
                    .iter()
                    .any(|current| current.name == function.name)
                {
                    self.vm.remove_definition(&function.name);
                }
            }
        }

        // Register structs and methods on VM
        for decl in &catalog.structs {
            let fields: Vec<(&str, bool)> = decl
                .fields
                .iter()
                .map(|(name, fixed)| (name.as_str(), *fixed))
                .collect();
            self.vm.register_struct(decl.name.clone(), fields);
        }
        for function in &module.functions {
            if let Some((type_name, method)) = function.name.split_once('.') {
                self.vm.register_struct_method(
                    type_name,
                    method,
                    function.entry_ip + offset,
                    function.params,
                    function.is_async,
                );
            }
        }

        // Execute top-level module code
        let _guard = CurrentRuntimeGuard::new(self as *mut AipoRuntime);
        if let Err(err) = self.vm.run_persistent_at(&candidate, offset) {
            // Atomic rollback: restore VM definitions and globals completely on failure
            self.vm.restore_definitions(snapshot);
            let msg = format!("runtime error initializing module '{name}': {err}");
            self.last_error = Some(msg.clone());
            let status = match err {
                VmError::UncaughtFailure(_) => aipo_status_t::AIPO_ERR_UNCAUGHT_FAILURE,
                _ => aipo_status_t::AIPO_ERR_FAULT,
            };
            return Err((status, msg));
        }
        if let Err(message) = self.vm.validate_guest_layouts() {
            self.vm.restore_definitions(snapshot);
            self.last_error = Some(message.clone());
            return Err((aipo_status_t::AIPO_ERR_USAGE, message));
        }

        self.linked = candidate;
        self.module_offsets.insert(name.to_string(), offset);
        self.last_loaded_module = Some(name.to_string());
        self.last_error = None;
        self.modules.insert(name.to_string(), Arc::new(module));
        Ok(())
    }

    /// Compiles and loads an Aipo module from source text.
    pub fn load_module(
        &mut self,
        name: &str,
        source_text: &str,
    ) -> Result<(), (aipo_status_t, String)> {
        if self.active_execution.is_some() {
            return Err((
                aipo_status_t::AIPO_ERR_USAGE,
                "runtime is executing; finish pumping before loading".into(),
            ));
        }
        let source = Source::new(SourceId::next(), name, source_text);
        let (program, diagnostics) = aipo_syntax::parse(&source);
        if diagnostics.iter().any(|d| d.severity == Severity::Error) {
            let details = diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .map(|d| d.message.clone())
                .collect::<Vec<_>>()
                .join("; ");
            let msg = format!("parse error in module '{name}': {details}");
            self.last_error = Some(msg.clone());
            return Err((aipo_status_t::AIPO_ERR_DIAGNOSTIC, msg));
        }

        let hir = aipo_hir::lower(program);
        let mut surface = PreludeSurface::fundamental();
        for host_fn in self.host_functions.keys() {
            surface.add_variable(host_fn);
        }
        for mod_name in self.modules.keys() {
            surface.add_variable(mod_name);
        }

        let (_, sema_diagnostics) = aipo_sema::check_with_prelude(&source, &hir, &surface);
        if sema_diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
        {
            let details = sema_diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .map(|d| d.message.clone())
                .collect::<Vec<_>>()
                .join("; ");
            let msg = format!("semantic error in module '{name}': {details}");
            self.last_error = Some(msg.clone());
            return Err((aipo_status_t::AIPO_ERR_DIAGNOSTIC, msg));
        }

        let ir = aipo_ir::lower_to_ir(&hir);
        let module = aipo_bytecode::compile(&ir).map_err(|errors| {
            let msg = format!("compilation error: {:?}", errors);
            (aipo_status_t::AIPO_ERR_DIAGNOSTIC, msg)
        })?;

        self.load_module_internal(name, module)
    }


    /// Loads and initializes an Aipo module from precompiled bytecode (.aibc).
    pub fn load_bytecode(
        &mut self,
        name: &str,
        bytes: &[u8],
    ) -> Result<(), (aipo_status_t, String)> {
        let module = BytecodeModule::from_bytes(bytes).map_err(|err| {
            let msg = format!("failed to decode bytecode for '{name}': {err}");
            self.last_error = Some(msg.clone());
            (aipo_status_t::AIPO_ERR_USAGE, msg)
        })?;

        aipo_bytecode::BytecodeVerifier::verify(&module).map_err(|errors| {
            let msg = format!("bytecode verification failed for '{name}': {errors:?}");
            self.last_error = Some(msg.clone());
            (aipo_status_t::AIPO_ERR_DIAGNOSTIC, msg)
        })?;

        self.load_module_internal(name, module)
    }

    /// Compiles Aipo source text into a serialized `.aibc` binary module.
    pub fn compile_bytecode(source_text: &str) -> Result<Vec<u8>, (aipo_status_t, String)> {
        let source = Source::new(SourceId::next(), "compile_bytecode.aipo", source_text);
        let (program, diagnostics) = aipo_syntax::parse(&source);
        if diagnostics.iter().any(|d| d.severity == Severity::Error) {
            let details = diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .map(|d| d.message.clone())
                .collect::<Vec<_>>()
                .join("; ");
            return Err((
                aipo_status_t::AIPO_ERR_DIAGNOSTIC,
                format!("parse error: {details}"),
            ));
        }

        let hir = aipo_hir::lower(program);
        let surface = PreludeSurface::fundamental();
        let (_, sema_diagnostics) = aipo_sema::check_with_prelude(&source, &hir, &surface);
        if sema_diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
        {
            let details = sema_diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .map(|d| d.message.clone())
                .collect::<Vec<_>>()
                .join("; ");
            return Err((
                aipo_status_t::AIPO_ERR_DIAGNOSTIC,
                format!("semantic error: {details}"),
            ));
        }

        let ir = aipo_ir::lower_to_ir(&hir);
        let module = aipo_bytecode::compile(&ir).map_err(|errors| {
            (
                aipo_status_t::AIPO_ERR_DIAGNOSTIC,
                format!("compilation error: {errors:?}"),
            )
        })?;

        Ok(module.to_bytes())
    }

    /// Compiles Aipo source text into a WebAssembly binary module (.wasm).
    pub fn compile_wasm(source_text: &str) -> Result<Vec<u8>, (aipo_status_t, String)> {
        let source = Source::new(SourceId::next(), "compile_wasm.aipo", source_text);
        let (program, diagnostics) = aipo_syntax::parse(&source);
        if diagnostics.iter().any(|d| d.severity == Severity::Error) {
            let details = diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .map(|d| d.message.clone())
                .collect::<Vec<_>>()
                .join("; ");
            return Err((
                aipo_status_t::AIPO_ERR_DIAGNOSTIC,
                format!("parse error: {details}"),
            ));
        }

        let hir = aipo_hir::lower(program);
        let surface = PreludeSurface::fundamental();
        let (_, sema_diagnostics) = aipo_sema::check_with_prelude(&source, &hir, &surface);
        if sema_diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error)
        {
            let details = sema_diagnostics
                .iter()
                .filter(|d| d.severity == Severity::Error)
                .map(|d| d.message.clone())
                .collect::<Vec<_>>()
                .join("; ");
            return Err((
                aipo_status_t::AIPO_ERR_DIAGNOSTIC,
                format!("semantic error: {details}"),
            ));
        }

        aipo_wasm::compile_hir(&hir).map_err(|err| {
            (
                aipo_status_t::AIPO_ERR_DIAGNOSTIC,
                format!("wasm compilation error: {err}"),
            )
        })
    }

    /// Prepares a loaded module for execution or stepping from instruction 0.
    pub fn prepare_module(&mut self, name: &str) -> Result<(), (aipo_status_t, String)> {
        let module = self.modules.get(name).cloned().ok_or_else(|| {
            let msg = format!("module '{name}' is not loaded");
            self.last_error = Some(msg.clone());
            (aipo_status_t::AIPO_ERR_USAGE, msg)
        })?;

        self.vm.prepare_module_execution(&module).map_err(|err| {
            let msg = format!("failed to prepare module '{name}': {err}");
            self.last_error = Some(msg.clone());
            (aipo_status_t::AIPO_ERR_FAULT, msg)
        })?;
        self.vm.ip = 0;
        self.vm.stack.clear();
        self.vm.frames.clear();
        self.last_loaded_module = Some(name.to_string());
        Ok(())
    }

    /// Steps execution by a single instruction on the specified or last loaded module.
    ///
    /// Returns `Ok(true)` if execution completed, or `Ok(false)` if more instructions remain.
    pub fn step(&mut self, module_name: Option<&str>) -> Result<bool, (aipo_status_t, String)> {
        let name: String = match module_name {
            Some(n) => n.to_string(),
            None => match self.last_loaded_module.as_deref() {
                Some(n) => n.to_string(),
                None => {
                    let msg = "no module loaded or specified to step".to_string();
                    self.last_error = Some(msg.clone());
                    return Err((aipo_status_t::AIPO_ERR_USAGE, msg));
                }
            },
        };

        let module = self.modules.get(&name).cloned().ok_or_else(|| {
            let msg = format!("module '{name}' is not loaded");
            self.last_error = Some(msg.clone());
            (aipo_status_t::AIPO_ERR_USAGE, msg)
        })?;

        let _guard = CurrentRuntimeGuard::new(self as *mut AipoRuntime);
        match self.vm.step(&module) {
            Ok(done) => Ok(done),
            Err(VmError::Suspended) => Ok(false),
            Err(err) => {
                let msg = format!("runtime error stepping module '{name}': {err}");
                self.last_error = Some(msg.clone());
                let status = match err {
                    VmError::UncaughtFailure(_) => aipo_status_t::AIPO_ERR_UNCAUGHT_FAILURE,
                    _ => aipo_status_t::AIPO_ERR_FAULT,
                };
                Err((status, msg))
            }
        }
    }

    /// Executes instructions on the specified (or last loaded) module until it finishes or hits `max_steps`.
    ///
    /// If `max_steps == 0`, execution proceeds without a step limit until completion or suspension.
    /// Returns `Ok(true)` if execution completed, or `Ok(false)` if it hit `max_steps` or suspended.
    pub fn run_until_idle(
        &mut self,
        module_name: Option<&str>,
        max_steps: u64,
    ) -> Result<bool, (aipo_status_t, String)> {
        let mut steps = 0u64;
        loop {
            let done = self.step(module_name)?;
            if done {
                return Ok(true);
            }
            steps = steps.saturating_add(1);
            if max_steps > 0 && steps >= max_steps {
                return Ok(false);
            }
        }
    }

    /// Starts a function as the cooperative main computation; never executes guest code here.
    pub fn begin(
        &mut self,
        module_name: &str,
        function_name: &str,
        args: &[aipo_value_t],
    ) -> Result<(), (aipo_status_t, String)> {
        if self.active_execution.is_some() {
            return Err((aipo_status_t::AIPO_ERR_USAGE, "runtime is busy".into()));
        }
        let module = self
            .modules
            .get(module_name)
            .ok_or_else(|| (aipo_status_t::AIPO_ERR_USAGE, "unknown module".into()))?;
        let function = module
            .function_by_name(function_name)
            .ok_or_else(|| (aipo_status_t::AIPO_ERR_USAGE, "unknown function".into()))?;
        if function.upvalues != 0 || args.len() != function.params {
            return Err((
                aipo_status_t::AIPO_ERR_USAGE,
                "function requires matching arguments and no captures".into(),
            ));
        }
        let values = args
            .iter()
            .map(aipo_value_t::to_vm_value)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| (aipo_status_t::AIPO_ERR_USAGE, error.to_string()))?;
        let linked = Arc::new(self.linked.clone());
        self.vm
            .start_at(
                &linked,
                function.entry_ip + self.module_offsets[module_name],
            )
            .map_err(|error| (aipo_status_t::AIPO_ERR_FAULT, error.to_string()))?;
        for value in values {
            self.vm
                .push(value)
                .map_err(|error| (aipo_status_t::AIPO_ERR_FAULT, error.to_string()))?;
        }
        self.active_execution = Some(linked);
        self.last_error = None;
        Ok(())
    }

    /// Runs at most `steps` scheduler quanta. Host callbacks can still block.
    pub fn pump(&mut self, steps: usize) -> Result<Option<aipo_value_t>, (aipo_status_t, String)> {
        if steps == 0 {
            return Err((
                aipo_status_t::AIPO_ERR_USAGE,
                "pump steps must be positive".into(),
            ));
        }
        let module = self.active_execution.clone().ok_or_else(|| {
            (
                aipo_status_t::AIPO_ERR_USAGE,
                "no active computation".into(),
            )
        })?;
        let _guard = CurrentRuntimeGuard::new(self as *mut AipoRuntime);
        for _ in 0..steps {
            match self.vm.debug_step(&module) {
                Ok(true) => {
                    self.active_execution = None;
                    let result = self
                        .vm
                        .take_completion()
                        .unwrap_or(Ok(Value::None))
                        .map_err(|error| {
                            let message = error.to_string();
                            self.last_error = Some(message.clone());
                            (
                                if matches!(error, VmError::UncaughtFailure(_)) {
                                    aipo_status_t::AIPO_ERR_UNCAUGHT_FAILURE
                                } else {
                                    aipo_status_t::AIPO_ERR_FAULT
                                },
                                message,
                            )
                        })?;
                    let value = aipo_value_t::from_vm_value(&result, self.snapshotter());
                    return Ok(Some(value));
                }
                Ok(false) => {}
                Err(error) => {
                    self.active_execution = None;
                    let message = error.to_string();
                    self.last_error = Some(message.clone());
                    return Err((aipo_status_t::AIPO_ERR_FAULT, message));
                }
            }
        }
        Ok(None)
    }

    /// Stops a cooperative computation while retaining its current globals.
    /// This is cancellation, not rollback of I/O or completed guest mutations.
    pub fn abort(&mut self) {
        if let Some(module) = self.active_execution.take() {
            let _ = self.vm.start_at(&module, module.code.len());
        }
    }

    /// Calls a function from the requested module on the shared linked code image.
    pub fn call(
        &mut self,
        module_name: &str,
        func_name: &str,
        args: &[aipo_value_t],
    ) -> Result<aipo_value_t, (aipo_status_t, String)> {
        if self.active_execution.is_some() {
            return Err((
                aipo_status_t::AIPO_ERR_USAGE,
                "runtime is executing; use pump".into(),
            ));
        }
        let module = self.modules.get(module_name).cloned().ok_or_else(|| {
            let msg = format!("module '{module_name}' is not loaded");
            self.last_error = Some(msg.clone());
            (aipo_status_t::AIPO_ERR_USAGE, msg)
        })?;

        // Resolve function value strictly from the requested module's function table
        let callee = module
            .functions
            .iter()
            .find(|f| f.name == func_name)
            .map(|f| Value::Function {
                entry_ip: (f.entry_ip + self.module_offsets[module_name]) as u32,
                arity: f.params as u16,
                is_async: f.is_async,
            })
            .ok_or_else(|| {
                let msg = format!("function '{func_name}' not found in module '{module_name}'");
                self.last_error = Some(msg.clone());
                (aipo_status_t::AIPO_ERR_USAGE, msg)
            })?;

        let mut vm_args = Vec::with_capacity(args.len());
        for arg in args {
            let v = arg.to_vm_value().map_err(|e| {
                let msg = e.to_string();
                self.last_error = Some(msg.clone());
                (aipo_status_t::AIPO_ERR_USAGE, msg)
            })?;
            vm_args.push(v);
        }

        let _guard = CurrentRuntimeGuard::new(self as *mut AipoRuntime);
        let linked = self.linked.clone();
        let result = self.vm.invoke(&linked, callee, &vm_args).map_err(|err| {
            let msg = format!("runtime error calling '{func_name}': {err}");
            self.last_error = Some(msg.clone());
            let status = match err {
                VmError::UncaughtFailure(_) => aipo_status_t::AIPO_ERR_UNCAUGHT_FAILURE,
                _ => aipo_status_t::AIPO_ERR_FAULT,
            };
            (status, msg)
        })?;

        let c_val = aipo_value_t::from_vm_value(&result, self.snapshotter());
        Ok(c_val)
    }

    /// Registers a host native function implemented in C.
    pub fn register_host_fn(
        &mut self,
        name: &str,
        arity: usize,
        required_capability: Option<&str>,
        callback: aipo_host_fn_t,
    ) {
        let entry = HostFnEntry { arity, callback };
        self.host_functions.insert(
            name.to_string(),
            (entry, required_capability.map(str::to_string)),
        );

        // Register dummy native on VM that will be routed via host_natives
        self.vm
            .define_global(name, Value::native(name, arity, dummy_native_fn));
        self.vm
            .register_host_native(name, arity, host_native_dispatcher);
    }

    /// Creates a generational handle for a host value.
    pub fn handle_create(&mut self, val: HostValue) -> aipo_handle_t {
        let handle = self.vm.host_context().hand_out(val);
        handle.into()
    }

    /// Resolves a live generational handle to its host value.
    pub fn handle_resolve(&self, handle: aipo_handle_t) -> Result<HostValue, VmFault> {
        let h: Handle = handle.into();
        self.vm.host().resolve(h).cloned()
    }

    /// Releases a generational handle, invalidating it forever.
    pub fn handle_release(&mut self, handle: aipo_handle_t) -> Result<(), VmFault> {
        let h: Handle = handle.into();
        self.vm
            .host_context()
            .release(h)
            .map(|_| ())
            .ok_or_else(|| VmFault::StaleHandle {
                handle: h.to_string(),
            })
    }

    /// Sets the maximum instruction execution budget (None = unlimited).
    pub fn set_instruction_budget(&mut self, limit: Option<u64>) {
        self.vm.set_max_instructions(limit);
    }

    /// Returns the number of instructions executed on this runtime.
    #[must_use]
    pub fn instruction_count(&self) -> u64 {
        self.vm.instruction_count()
    }

    /// Resets the instruction execution counter to zero.
    pub fn reset_instruction_count(&mut self) {
        self.vm.reset_instruction_count();
    }
}

fn dummy_native_fn(_args: &[Value]) -> Result<Value, VmFault> {
    Ok(Value::None)
}

fn host_native_dispatcher(vm: &mut Vm, args: &[Value]) -> Result<Value, VmError> {
    let rt_ptr = CURRENT_RUNTIME.get();
    if rt_ptr.is_null() {
        return Err(VmFault::TypeMismatch {
            expected: "active runtime context".to_string(),
            actual: "null".to_string(),
        }
        .into());
    }

    // Identity comes from the callee itself, never from its arity. Two host
    // functions can share an arity and require different capabilities, so
    // guessing by shape would run the wrong operation under the wrong grant.
    let callee_name = vm
        .stack
        .len()
        .checked_sub(1 + args.len())
        .and_then(|idx| vm.stack.get(idx))
        .and_then(|val| match val {
            Value::Native(data) => Some(data.name.clone()),
            _ => None,
        })
        .ok_or_else(|| {
            VmError::from(VmFault::NotCallable {
                type_name: "host function reached without an identifiable callee".to_string(),
            })
        })?;

    let (entry, required_cap, c_args) = {
        let rt = unsafe { &mut *rt_ptr };

        let (entry, required_cap) = rt
            .host_functions
            .get(&callee_name)
            .map(|(e, cap)| (*e, cap.clone()))
            .ok_or_else(|| {
                VmError::from(VmFault::NotCallable {
                    type_name: format!("unregistered host function '{callee_name}'"),
                })
            })?;

        // Capability check
        if let Some(ref cap_name) = required_cap {
            let cap = Capability::parse(cap_name).map_err(|e| VmFault::CapabilityDenied {
                capability: cap_name.clone(),
                operation: e.to_string(),
            })?;
            if let Err(e) = rt.capabilities.require(&cap, &callee_name) {
                return Err(VmFault::CapabilityDenied {
                    capability: cap_name.clone(),
                    operation: e.to_string(),
                }
                .into());
            }
        }

        // Convert arguments to aipo_value_t, each heap payload copied into a snapshot
        // so the callback can keep the pointers past the end of this call.
        let c_args: Vec<aipo_value_t> = args
            .iter()
            .map(|v| aipo_value_t::from_vm_value(v, rt.snapshotter()))
            .collect();

        Ok::<_, VmError>((entry, required_cap, c_args))
    }?;

    let name = callee_name;

    let mut out_result = aipo_value_t::none();
    // Refuse re-entry before the callback can reach the runtime again through a
    // stored pointer, and before any further `&mut` is formed.
    let _depth = CallbackDepthGuard::acquire()?;
    let status = (entry.callback)(
        rt_ptr as *mut crate::types::aipo_runtime_t,
        c_args.as_ptr(),
        c_args.len(),
        &mut out_result,
    );
    drop(_depth);

    match status {
        aipo_status_t::AIPO_OK => {
            let val = out_result
                .to_vm_value()
                .map_err(|e| VmFault::TypeMismatch {
                    expected: "valid return value".to_string(),
                    actual: e.to_string(),
                })?;
            Ok(val)
        }
        aipo_status_t::AIPO_ERR_UNCAUGHT_FAILURE => {
            let msg = out_result
                .to_vm_value()
                .ok()
                .and_then(|v| match v {
                    Value::Failure(f) => Some(f.message.to_string()),
                    _ => None,
                })
                .unwrap_or_else(|| "host failure".to_string());
            Ok(Value::failure(msg))
        }
        aipo_status_t::AIPO_ERR_CAPABILITY_DENIED => Err(VmFault::CapabilityDenied {
            capability: required_cap.unwrap_or_default(),
            operation: name,
        }
        .into()),
        aipo_status_t::AIPO_ERR_STALE_HANDLE => Err(VmFault::StaleHandle {
            handle: Handle::from(out_result.handle_val).to_string(),
        }
        .into()),
        _ => Err(VmFault::TypeMismatch {
            expected: "AIPO_OK".to_string(),
            actual: format!("{status:?}"),
        }
        .into()),
    }
}
