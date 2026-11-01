# omniLua — source map and reference architecture

- **Repository:** [ianm199/omnilua](https://github.com/ianm199/omnilua)
- **Pinned commit:** `90dcf85a8cfb95305b77056af00a114fd0df30a1`
- **License:** MIT (Copyright (c) 2026 lua-rs-port contributors) with PUC-Rio NOTICE (Lua 5.4.7).
- **Declared invariant:** `#![forbid(unsafe_code)]` at the workspace root, with an automated oracle against Lua 5.1–5.5.

---

## 1. Mapping of crates and modules of interest

| omniLua crate | Source path | Central symbols / mechanisms | Problem it solves | Relevance / potential for Aipo |
|---|---|---|---|---|
| `lua-gc` | `crates/lua-gc/src/heap.rs` | `Heap`, `Gc<T>`, `HeapGuard`, `Marker`, `StepBudget`, `StepOutcome`, `detached_allocations` | Incremental mark-sweep GC in Pure Rust with an allocation pacer and strict guarantees against detached allocations (`HeapGuard`). | **INSPIRE / ADAPT:** allocation-accounting model and leak canaries for VM lifecycle tests. |
| `lua-rs-runtime` | `crates/lua-rs-runtime/src/lib.rs` | `Lua`, `Lua::scope`, `Scope`, `create_userdata_ref_mut`, `SandboxConfig` | High-level Rust embedding API with bounded-lifetime loans (`Lua::scope`), preventing use-after-free without FFI. | **ADAPT:** ergonomic scoped-loan pattern for the native Rust host API (`AipoEngine::scope`). |
| `lua-rs-runtime` (sandbox) | `crates/lua-rs-runtime/tests/sandbox.rs`, `docs/SANDBOXING_EXPLORATION.md` | `SandboxLimits`, `install_sandbox_limits`, `sandbox_charge_interval`, `aborting` flag | Sandbox against adversarial code: instruction budget spanning coroutines, sampled memory ceiling, uncatchable abort via `pcall`. | **ADAPT (P0 priority):** persistent shared budget and an uncatchable abort flag for Aipo's `attempt { ... }`. |
| `lua-vm` | `crates/lua-vm/src/state.rs`, `vm.rs` | `GlobalState`, `trace_exec`, `trap`, `hook_mask` | Register VM dispatch with zero-cost hook check on the hot path (`hook_mask == 0`). | **ADAPT:** periodic fuel metering strategy (e.g. every N instructions) instead of a branch per opcode. |
| `lua-types` | `crates/lua-types/src/lib.rs`, `value.rs` | `LuaValue`, `LuaString`, tagged representation, interned strings | Compact representation of dynamic values and string handling in Pure Rust. | **INFORMATIVE:** Aipo operates on strict UTF-8; Lua strings are arbitrary byte sequences. |
| `lua-stdlib` | `crates/lua-stdlib/src/lib.rs`, `base.rs` | `HostHooks`, capability stripping, IO and OS isolation | Isolation of side-effecting functions through capability removal at initialization. | **VERIFIED:** Aipo already has a superior `aipo-host::CapabilitySet` (hierarchical deny-by-default). |
| `lua-wasm` | `crates/lua-wasm/src/lib.rs` | Browser and Node execution without a heavy Wasmtime runtime | Lightweight Wasm runner for browser/web environments. | **INSPIRE:** alternative for Aipo's `nano` profile without dragging in the Wasmtime compile dependency. |
| `harness` | `harness/canaries/`, `tests/leak_canaries.rs` | Global allocator with atomic byte counting (`CountingAllocator`) | Empirical validation of net-zero live bytes after VM and script churn. | **ADAPT (P0 priority):** memory-leak canaries for Aipo runtime lifecycle tests. |

---

## 2. Technical highlights for Aipo

### 2.1 Uncatchable sandboxing
In omniLua (`docs/SANDBOXING_EXPLORATION.md`), the biggest challenge solved was preventing an
adversarial script from nullifying a budget overflow by catching the error in a `pcall` loop:

```lua
while true do
  pcall(function() while true do end end)
end
```

**The solution:** a sticky `aborting` flag in the VM's global state. When the instruction budget
exhausts, the dispatcher enters an irrevocable abort state; protected functions re-raise the
error immediately instead of returning a captured status.

- **Application in Aipo:** Aipo's failure model (Model B+) distinguishes recoverable `Failure`
  (handled by `attempt`) from `Fault` (violated invariant). Budget exhaustion must raise a
  specific `VmFault::BudgetExhausted` that is shielded against absorption by `attempt`.

### 2.2 Scope API and temporary host loans
`Lua::scope` lets a complex host structure (e.g. `&mut GameWorld` in a game) be lent to an
Aipo script for the exact duration of a scope function, with RAII and lifetimes guaranteeing
that residual references stored in globals or closures fail safely (`AIPO_RT_SCOPE_ESCAPE` or
stale handle) if accessed after the scope ends.

- **Application in Aipo:** connect with the existing `crates/aipo-host` infrastructure
  (`HandleTable`) to provide a pure-Rust native API `engine.scope(|scope| { ... })`.

### 2.3 Counting global allocator (leak canaries)
omniLua validates that the runtime discards 100% of its memory through a global allocator wrapper
in tests:

```rust
#[global_allocator]
static ALLOC: CountingAllocator = CountingAllocator::new();
```

It asserts that, at the end of `create_vm -> run_scripts -> destroy_vm`, the count of live bytes
allocated on the heap returns strictly to zero.

- **Application in Aipo:** integrate equivalent canaries in the integration test suite
  (`crates/aipo-testkit`) to prove the absence of cyclic `Rc<RefCell<...>>` leaks.
