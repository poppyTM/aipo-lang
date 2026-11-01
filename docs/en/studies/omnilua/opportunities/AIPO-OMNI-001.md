# AIPO-OMNI-001 — memory-leak canaries (`CountingAllocator` & steady-state lifecycle)

- **Status:** DONE & CERTIFIED (Phase 1 / P0 pilot 1)
- **Origin / reference:** omniLua (`crates/lua-rs-runtime/tests/leak_canaries.rs` and
  `CountingAllocator`)
- **Classification:** `ADAPT` (P0, score +38)

---

## 1. Problem identified in Aipo

1. **Absence of steady-state canaries:** Aipo's object model uses `Rc<RefCell<...>>` for structs,
   lists, dictionaries and closures. Although local protections such as `CycleGuard` prevent
   obvious reference cycles in mutable assignments, there was no integration test with a global
   allocator instrumented to verify that creating 1,000 `Vm` instances, running scripts with
   nested closures and destroying them results in zero live bytes on the heap.
2. **Discovery of a hidden `Rc` cycle (160 bytes per iteration):** activating the memory-leak
   canary in `aipo-testkit` immediately detected a constant leak of exactly **160 bytes per
   iteration** (5,120 bytes over 32 iterations) in local closures.
   - **Root cause:** in `crates/aipo-ir/src/builder.rs` (old line 1046), every local function
     declared (`Stmt::Function`) was unconditionally wrapped with `self_capture`, emitting an
     `Opcode::FillSelfCapture`. That forced injection of the closure itself into an upvalue cell
     inside its own capture vector (`ClosureData -> upvalue cell -> ClosureData`), creating a
     strong `Rc` cycle even for pure or non-recursive functions.

---

## 2. Implementation performed

### A. Upvalue-cycle fix in the IR compiler (`crates/aipo-ir`)
- In `crates/aipo-ir/src/builder.rs`:
  - Implemented the static function `references_name(expr: &Expr, target: &str)`, inspecting the
    function's AST to determine whether the body actually self-references its identifier.
  - Self-capture (`self_capture`) and the `FillSelfCapture` instruction are now generated
    **only when the function is provably recursive**.
  - Non-recursive local functions have their upvalues deallocated and cleared immediately, with
    no `Rc` cycle.
  - **Result:** the 160 bytes/iteration leak dropped to **exactly 0 bytes**.

### B. Memory-leak canaries (`crates/aipo-testkit`)
- In `crates/aipo-testkit/Cargo.toml`:
  - Added dependencies on `aipo-host` and `aipo-wasm`.
  - Configured the `unsafe_code = "allow"` lint at the test/binary-auxiliary level (required to
    implement the `std::alloc::GlobalAlloc` trait), strictly preserving
    `#![forbid(unsafe_code)]` in the production library `src/lib.rs`.
- In `crates/aipo-testkit/tests/leak_canaries.rs`:
  - Implemented `CountingAllocator` with atomic counters `ALLOCATED_BYTES` and
    `DEALLOCATED_BYTES`, tracking `live_bytes()` with single-byte precision.
  - Implemented the `assert_steady_state(name, scenario)` harness running 4 warmup iterations
    followed by 32 continuous churn iterations, checking `growth <= TOLERANCE`
    (tolerance = 0 bytes).
  - Coverage of 7 complete lifecycle scenarios:
    1. **`vm_churn`:** instantiation of `Vm`, execution of scripts with dictionaries, strings
       and collections, then discard.
    2. **`closure_churn`:** instantiation of higher-order closures capturing local variables in
       nested scopes, execution and discard.
    3. **`reg_vm_churn`:** compilation to Register VM bytecode, execution on `RegVm` and discard
       of instances and modules.
    4. **`frontend_churn`:** syntactic analysis, lowering to HIR and semantic verification
       (`aipo-sema`) of interfaces and methods.
    5. **`compiler_churn`:** full compilation of source to verified bytecode module and its
       discard.
    6. **`wasm_compile_churn`:** compilation of HIR to a WebAssembly binary and disassembly to
       WAT via `aipo-wasm`.
    7. **`handle_table_churn`:** massive allocation and deallocation of generational handles in
       `aipo-host::HandleTable`.

---

## 3. Quality evidence and results

Canary test run:

```bash
$ cargo test -p aipo-testkit --test leak_canaries -- --nocapture
vm_churn: baseline=5516, final=5516, growth=0
closure_churn: baseline=6859, final=6859, growth=0
reg_vm_churn: baseline=6859, final=6859, growth=0
frontend_churn: baseline=6859, final=6859, growth=0
compiler_churn: baseline=6859, final=6859, growth=0
wasm_compile_churn: baseline=6859, final=6859, growth=0
handle_table_churn: baseline=6859, final=6859, growth=0
test test_embedding_lifecycle_is_steady_state ... ok
```

- **Net memory growth:** **0 bytes** across all 7 tested subsystems.
- **Practical detection:** proved the effectiveness of the omniLua pattern by capturing and
  eradicating a real `Rc` leak in Aipo's codebase.
- **Quality gates:** Clippy approved with no warnings (`-D warnings`), `cargo fmt` 100%
  compliant, `#![forbid(unsafe_code)]` intact in production crates.
