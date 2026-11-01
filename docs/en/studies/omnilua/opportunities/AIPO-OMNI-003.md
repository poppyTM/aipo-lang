# AIPO-OMNI-003 — scoped Rust embedding API (`AipoEngine::scope` & safe userdata)

- **Status:** DONE & CERTIFIED (Phase 1 / P0 pilot 3)
- **Origin / reference:** omniLua (`Lua::scope`, `Scope`, `create_userdata_ref_mut` in
  `crates/lua-rs-runtime/src/lib.rs`)
- **Classification:** `ADAPT` (P0, score +28)

---

## 1. Problem identified in Aipo

1. **Dependence on global singletons in hosts:** in game-engine integrations such as
   `aipo-poppy`, communication between the Rust host and Aipo scripts depended on a
   process-global mutex (`static POPPY: LazyLock<Mutex<Option<PoppyService>>>`) and manual
   serialization (`POPPY_LOCK`). That prevented running multiple concurrent runtime instances
   in distinct threads.
2. **Absence of a safe Rust façade:** embedders had to deal directly with VM internals
   (`open_scope`, `close_scope`, manual registration of bytecode structs/methods, native
   dispatch). There was no idiomatic API with a compile-time guarantee that borrowed host data
   does not outlive execution.
3. **Non-escape guarantee for references:** it had to be guaranteed that:
   - In Rust, scope references or generational handles cannot be stored outside the `scope`
     closure.
   - In the VM, any attempt by the script to store the handle in globals or persistent
     collections immediately raises the canonical fault `[AIPO_RT_SCOPE_ESCAPE]`.
   - Zero `unsafe` code: strictly preserve `#![forbid(unsafe_code)]` in all production crates.

---

## 2. Implementation performed

### A. Generational userdata table in the host context (`crates/aipo-vm/src/host.rs`)
- `HostContext` extended with `userdata: HandleTable<Box<dyn std::any::Any>>`.
- Typed lifecycle operations implemented:
  - `hand_out_userdata<T: 'static>(&mut self, value: T) -> Handle`: registers the object in the
    generational table and associates it with the active scope.
  - `resolve_userdata<T: 'static>(&self, handle: Handle) -> Result<&T, VmFault>`: resolves an
    immutable reference, validating scope escape, generation and type (downcast).
  - `resolve_userdata_mut<T: 'static>(&mut self, handle: Handle) -> Result<&mut T, VmFault>`:
    resolves an exclusive mutable reference.
  - `release_userdata(&mut self, handle: Handle) -> Option<Box<dyn Any>>`: invalidates the slot
    immediately.
- `close_scope(scope_id)` updated to atomically remove and discard all userdata objects created
  in the scope, marking their handles in the `escaped` set.

### B. Embedding façade and RAII scope (`crates/aipo-vm/src/engine.rs`)
- **`AipoEngineBuilder`**: fluent configuration of capabilities (`CapabilitySet`) and
  instruction quotas (`limits`, `max_instructions`).
- **`AipoEngine`**:
  - `engine.scope(|scope| { ... })`: opens an RAII scope. Guarantees that, even if the scope
    body returns an `Err`, cleanup and invalidation of all scope handles runs on close.
  - `register_module_declarations(&mut self, module: &BytecodeModule)`: automatically registers
    all struct and method declarations from a bytecode module, eliminating host boilerplate.
  - `register_host_fn(&mut self, name, arity, callback)`: registers native callbacks in the VM.
  - `run(&mut self, module)`: runs compiled modules.
- **`AipoScope<'scope, 'engine>`**:
  - Bounded by the `'scope` lifetime.
  - `insert_userdata<T: 'static>(&mut self, value: T) -> ScopedHandle<'scope>`.
  - `resolve_userdata<T>` and `resolve_userdata_mut<T>`.
  - `expose_global(&mut self, name, handle)`: defines the handle in the VM's global environment.
  - `register_host_fn`: registers native functions bound to the scope.
  - `sub_scope`: support for nested sub-scopes with strict isolation between parent and child.
- **`ScopedHandle<'scope>`**:
  - Wraps the generational `Handle` with the `'scope` lifetime tag. Rust's borrow checker
    statically rejects at compile time any attempt to return or store the handle outside the
    scope closure.

---

## 3. Quality and conformance evidence

### A. Integration test suite (`crates/aipo-vm/tests/engine_scope_tests.rs`)
9 automated integration tests covering every scenario in the matrix:
1. `test_engine_builder_and_limits`: builder validation, capability grants and fuel interruption.
2. `test_scope_userdata_mut_happy_path`: mutation of `GameWorld` state through native callbacks
   inside the scope and invalidation post-scope.
3. `test_scope_userdata_invalidated_even_when_body_errors`: RAII teardown guarantee even after
   errors.
4. `test_nested_scopes_isolated`: isolation of nested scopes (closing an inner scope does not
   invalidate outer handles).
5. `test_scope_type_mismatch_fault`: safe emission of `AIPO_RT_TYPE_MISMATCH` on incompatible
   downcast attempts.
6. `test_scope_shared_rc_interior_mutability`: loan of shared structures with `Rc<RefCell<T>>`,
   keeping mutated state in the host after the scope clone is dropped.
7. `test_engine_register_module_declarations`: automatic registration of structs and methods.
8. `test_scope_escape_caught_by_publication_check`: VM's dynamic barrier raising
   `AIPO_RT_SCOPE_ESCAPE` when a script tries to publish a scope handle to a global after the
   scope.
9. `test_scope_callback_fault_does_not_panic`: controlled propagation of `VmFault` across the
   native boundary without panicking.

### B. Steady-state memory canary (`crates/aipo-testkit/tests/leak_canaries.rs`)
- Scenario 8 added (`engine_scope_churn`): 32 continuous iterations of `AipoEngine` creation,
  `scope` opening, userdata insertion, native callback registration, script execution and
  discard.
- Result: **`growth = 0 bytes`** (net allocation balance strictly zero).

### C. Compilation and style gates
- `#![forbid(unsafe_code)]` 100% preserved.
- `cargo clippy -p aipo-vm -p aipo-testkit -- -D warnings`: 0 warnings.
- `cargo fmt --all --check`: 100% compliant.
