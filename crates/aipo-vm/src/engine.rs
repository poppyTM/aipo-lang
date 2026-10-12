//! High-level, safe Rust embedding engine and scoped execution context (`ADP-011`, `AIPO-OMNI-003`).
//!
//! This module provides the primary safe Rust embedding facade for Aipo:
//! - [`AipoEngine`]: Configurable, isolated execution engine wrapping a [`Vm`].
//! - [`AipoEngineBuilder`]: Builder pattern for configuring capabilities and execution limits.
//! - [`AipoScope`]: RAII-bounded execution scope guaranteeing that borrowed host objects and
//!   scoped generational handles cannot outlive the scope, neither statically in Rust nor
//!   dynamically inside scripts.
//! - [`ScopedHandle`]: Lifetime-tagged wrapper around [`Handle`].

use aipo_bytecode::BytecodeModule;
use aipo_host::{CapabilitySet, Handle, HostValue};

use crate::fault::{VmError, VmFault};
use crate::host::{HostContext, ScopeId};
use crate::value::Value;
use crate::vm::{HostNativeCallback, Vm};

/// Builder for constructing configured [`AipoEngine`] instances.
#[derive(Debug, Default)]
pub struct AipoEngineBuilder {
    capabilities: Option<CapabilitySet>,
    max_instructions: Option<u64>,
    meter_interval: Option<u64>,
}

impl AipoEngineBuilder {
    /// Creates a fresh engine builder with deny-by-default capabilities.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Configures the capability set granted to the engine.
    #[must_use]
    pub fn capabilities(mut self, caps: CapabilitySet) -> Self {
        self.capabilities = Some(caps);
        self
    }

    /// Sets the maximum instruction execution budget (fuel limit).
    #[must_use]
    pub fn max_instructions(mut self, limit: u64) -> Self {
        self.max_instructions = Some(limit);
        self
    }

    /// Sets or clears the execution budget.
    #[must_use]
    pub fn limits(mut self, limit: Option<u64>) -> Self {
        self.max_instructions = limit;
        self
    }

    /// Configures the periodic instruction metering interval (default: 64).
    #[must_use]
    pub fn meter_interval(mut self, interval: u64) -> Self {
        self.meter_interval = Some(interval);
        self
    }

    /// Builds the configured [`AipoEngine`].
    pub fn build(self) -> Result<AipoEngine, VmFault> {
        let mut vm = Vm::new();
        if let Some(caps) = self.capabilities {
            *vm.host_context() = HostContext::with_capabilities(caps);
        }
        vm.set_max_instructions(self.max_instructions);
        if let Some(interval) = self.meter_interval {
            vm.set_meter_interval(interval);
        }
        Ok(AipoEngine { vm })
    }
}

/// Safe Rust embedding engine for Aipo scripts.
#[derive(Default)]
pub struct AipoEngine {
    vm: Vm,
}

fn placeholder_native(_args: &[Value]) -> Result<Value, VmFault> {
    Ok(Value::None)
}

impl AipoEngine {
    /// Creates a fresh engine builder.
    #[must_use]
    pub fn builder() -> AipoEngineBuilder {
        AipoEngineBuilder::default()
    }

    /// Creates a new isolated engine with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self { vm: Vm::new() }
    }

    /// Borrows the underlying VM immutably.
    #[must_use]
    pub fn vm(&self) -> &Vm {
        &self.vm
    }

    /// Borrows the underlying VM mutably.
    pub fn vm_mut(&mut self) -> &mut Vm {
        &mut self.vm
    }

    /// Registers all structs and methods declared in a bytecode module.
    pub fn register_module_declarations(&mut self, module: &BytecodeModule) {
        for decl in &module.structs {
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
                    function.entry_ip,
                    function.params,
                    function.is_async,
                );
            }
        }
    }

    /// Registers a host native callback on the engine.
    pub fn register_host_fn(&mut self, name: &str, arity: usize, callback: HostNativeCallback) {
        self.vm
            .define_global(name, Value::native(name, arity, placeholder_native));
        self.vm.register_host_native(name, arity, callback);
    }

    /// Opens an isolated RAII execution scope where host objects can be lent to scripts
    /// with static and dynamic guarantees that references do not escape.
    ///
    /// When the closure returns (or errors), every handle minted within this scope is
    /// immediately closed and invalidated.
    pub fn scope<'engine, R, F>(&'engine mut self, f: F) -> Result<R, VmError>
    where
        F: for<'scope> FnOnce(&mut AipoScope<'scope, 'engine>) -> Result<R, VmError>,
    {
        let scope_id = self.vm.host_context().open_scope();
        let mut scope = AipoScope {
            engine: self,
            scope_id,
            _marker: std::marker::PhantomData,
        };
        let result = f(&mut scope);
        scope.engine.vm.host_context().close_scope(scope_id);
        result
    }

    /// Executes a precompiled bytecode module on the engine.
    pub fn run(&mut self, module: &BytecodeModule) -> Result<Value, VmError> {
        self.vm.run(module)
    }

    /// Configures the maximum instruction execution budget (fuel limit).
    pub fn set_instruction_budget(&mut self, limit: Option<u64>) {
        self.vm.set_max_instructions(limit);
    }

    /// Configures the periodic instruction metering interval (default: 64).
    pub fn set_meter_interval(&mut self, interval: u64) {
        self.vm.set_meter_interval(interval);
    }

    /// Returns the current periodic instruction metering interval.
    #[must_use]
    pub fn meter_interval(&self) -> u64 {
        self.vm.meter_interval()
    }

    /// Returns the number of instructions executed since the last reset.
    #[must_use]
    pub fn instruction_count(&self) -> u64 {
        self.vm.instruction_count()
    }

    /// Resets the executed instruction counter to zero.
    pub fn reset_instruction_count(&mut self) {
        self.vm.reset_instruction_count();
    }
}

/// An RAII-bounded execution scope for host embeddings.
///
/// Any [`ScopedHandle`] minted inside this scope is statically bounded by `'scope`
/// and dynamically invalidated when the scope finishes.
pub struct AipoScope<'scope, 'engine> {
    engine: &'engine mut AipoEngine,
    scope_id: ScopeId,
    _marker: std::marker::PhantomData<&'scope mut ()>,
}

impl<'scope, 'engine> AipoScope<'scope, 'engine> {
    /// Returns the identifier of this host scope.
    #[must_use]
    pub fn scope_id(&self) -> ScopeId {
        self.scope_id
    }

    /// Borrows the enclosing engine.
    #[must_use]
    pub fn engine(&self) -> &AipoEngine {
        self.engine
    }

    /// Borrows the enclosing engine mutably.
    pub fn engine_mut(&mut self) -> &mut AipoEngine {
        self.engine
    }

    /// Borrows the underlying VM.
    #[must_use]
    pub fn vm(&self) -> &Vm {
        &self.engine.vm
    }

    /// Borrows the underlying VM mutably.
    pub fn vm_mut(&mut self) -> &mut Vm {
        &mut self.engine.vm
    }

    /// Mints a generational handle for a plain host value, bounded to this scope.
    pub fn mint_handle(&mut self, value: HostValue) -> ScopedHandle<'scope> {
        let handle = self.engine.vm.host_context().hand_out(value);
        ScopedHandle {
            handle,
            scope_id: self.scope_id,
            _marker: std::marker::PhantomData,
        }
    }

    /// Mints a generational handle for an arbitrary Rust host object, bounded to this scope.
    pub fn insert_userdata<T: 'static>(&mut self, value: T) -> ScopedHandle<'scope> {
        let handle = self.engine.vm.host_context().hand_out_userdata(value);
        ScopedHandle {
            handle,
            scope_id: self.scope_id,
            _marker: std::marker::PhantomData,
        }
    }

    /// Resolves an immutable reference to a scoped host object.
    ///
    /// # Errors
    /// Returns [`VmFault::StaleHandle`] or [`VmFault::ScopeEscape`] if the handle is invalid,
    /// or [`VmFault::TypeMismatch`] if the downcast fails.
    pub fn resolve_userdata<T: 'static>(
        &self,
        handle: &ScopedHandle<'scope>,
    ) -> Result<&T, VmFault> {
        self.engine.vm.host().resolve_userdata::<T>(handle.raw())
    }

    /// Resolves a mutable reference to a scoped host object.
    ///
    /// # Errors
    /// Returns [`VmFault::StaleHandle`] or [`VmFault::ScopeEscape`] if the handle is invalid,
    /// or [`VmFault::TypeMismatch`] if the downcast fails.
    pub fn resolve_userdata_mut<T: 'static>(
        &mut self,
        handle: &ScopedHandle<'scope>,
    ) -> Result<&mut T, VmFault> {
        self.engine
            .vm
            .host_context()
            .resolve_userdata_mut::<T>(handle.raw())
    }

    /// Exposes a scoped handle as a global variable in the VM.
    pub fn expose_global(&mut self, name: &str, handle: &ScopedHandle<'scope>) {
        self.engine
            .vm
            .define_global(name, Value::HostHandle(handle.raw()));
    }

    /// Exposes an arbitrary language value as a global variable in the VM.
    pub fn define_global(&mut self, name: &str, value: Value) {
        self.engine.vm.define_global(name, value);
    }

    /// Registers a host native callback on the VM.
    pub fn register_host_fn(&mut self, name: &str, arity: usize, callback: HostNativeCallback) {
        self.engine
            .vm
            .define_global(name, Value::native(name, arity, placeholder_native));
        self.engine.vm.register_host_native(name, arity, callback);
    }

    /// Executes a bytecode module within this scope.
    pub fn run(&mut self, module: &BytecodeModule) -> Result<Value, VmError> {
        self.engine.vm.run(module)
    }

    /// Opens a nested RAII sub-scope.
    pub fn sub_scope<'nested, R, F>(&'nested mut self, f: F) -> Result<R, VmError>
    where
        F: for<'sub_scope> FnOnce(&mut AipoScope<'sub_scope, 'nested>) -> Result<R, VmError>,
    {
        let scope_id = self.engine.vm.host_context().open_scope();
        let mut sub = AipoScope {
            engine: self.engine,
            scope_id,
            _marker: std::marker::PhantomData,
        };
        let result = f(&mut sub);
        sub.engine.vm.host_context().close_scope(scope_id);
        result
    }
}

/// A generational handle statically and dynamically bounded to a host scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ScopedHandle<'scope> {
    handle: Handle,
    scope_id: ScopeId,
    _marker: std::marker::PhantomData<&'scope ()>,
}

impl<'scope> ScopedHandle<'scope> {
    /// Returns the raw generational handle.
    #[must_use]
    pub fn raw(self) -> Handle {
        self.handle
    }

    /// Returns the scope ID that minted this handle.
    #[must_use]
    pub fn scope_id(self) -> ScopeId {
        self.scope_id
    }
}
