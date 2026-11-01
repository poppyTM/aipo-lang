# Opportunity matrix: omniLua reuse → Aipo-lang

- **Audit status:** Phase 0 (initial classification)
- **Ordering formula:**
  $$\text{Score} = 3B + 3F + 2R + 2M - 2C - 3G - 2U$$
  *(Scale 0 to 5 per criterion; higher score = higher viability over risk)*

---

## 1. General audited-opportunity table

| ID | Opportunity / technique | Proposed via | Priority | $B$ | $F$ | $R$ | $M$ | $C$ | $G$ | $U$ | Score | Preliminary decision |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| **AIPO-OMNI-001** | **Memory-leak canaries (`CountingAllocator`)** | `ADAPT` | **P0** | 4 | 5 | 5 | 5 | 1 | 1 | 1 | **+38** | **DONE & CERTIFIED** ([Doc](opportunities/AIPO-OMNI-001.md)) |
| **AIPO-OMNI-002** | **Sandboxing: uncatchable instruction budget (`Fault` sticky)** | `ADAPT` | **P0** | 5 | 5 | 5 | 4 | 2 | 1 | 1 | **+37** | **DONE & CERTIFIED** ([Doc](opportunities/AIPO-OMNI-002.md)) |
| **AIPO-OMNI-003** | **Scoped Rust embedding API (`AipoEngine::scope`)** | `ADAPT` | **P0** | 5 | 5 | 4 | 4 | 3 | 2 | 2 | **+28** | **DONE & CERTIFIED** ([Doc](opportunities/AIPO-OMNI-003.md)) |
| **AIPO-OMNI-004** | **Periodic fuel metering in dispatch (interval metering)** | `ADAPT` | **P1** | 4 | 5 | 3 | 3 | 2 | 2 | 1 | **+27** | **DONE & CERTIFIED** ([Doc](opportunities/AIPO-OMNI-004.md)) |
| **AIPO-OMNI-005** | **Register windows in RegVm (replace 256-reg copy)** | `ADAPT` | **P1** | 5 | 4 | 2 | 3 | 4 | 3 | 2 | **+18** | **DONE & CERTIFIED** ([Doc](opportunities/AIPO-OMNI-005.md)) |
| **AIPO-OMNI-006** | **Tracing garbage collector (replace `Rc`/arenas model)** | `DEFER` | **P2** | 3 | 2 | 2 | 1 | 5 | 5 | 4 | **-14** | **DEFERRED (requires profiling)** |
| **AIPO-OMNI-007** | **Arbitrary byte strings as text representation** | `REJECT` | — | 1 | 0 | 0 | 0 | 5 | 5 | 5 | **-32** | **REJECTED (breaks UTF-8)** |
| **AIPO-OMNI-008** | **Hybrid tables / dynamic Lua metatables** | `REJECT` | — | 1 | 0 | 0 | 0 | 5 | 5 | 5 | **-32** | **REJECTED (breaks typing)** |
| **AIPO-OMNI-009** | **Grammar / block delimiters (`do...end`)** | `REJECT` | — | 0 | 0 | 0 | 0 | 5 | 5 | 5 | **-35** | **REJECTED (ADR-001)** |

---

## 2. Detail of the three P0 candidates approved for proof of concept

### Candidate P0-1: AIPO-OMNI-001 — Memory-leak canaries (`CountingAllocator`)
- **Problem in Aipo:** the object model uses `Rc<RefCell<...>>` for structs, lists, dictionaries and
  closures. Although `CycleGuard` prevents obvious reference cycles in mutable assignments, there
  was no integration test with a global allocator instrumented to verify that creating 1,000
  `Vm` instances, running scripts with nested closures and destroying them results in zero live
  bytes on the heap.
- **Reference in omniLua:** `crates/lua-rs-runtime/tests/leak_canaries.rs` and
  `harness/strict_guard_check.sh`.
- **Proposed implementation:** create a test in `crates/aipo-testkit` configuring a counting
  global allocator during tests, verifying that the full lifecycle of VMs, tasks and modules
  discards 100% of allocated memory.
- **Success metric:** $\Delta \text{bytes} = 0$ after a 100-iteration churn lifecycle.

### Candidate P0-2: AIPO-OMNI-002 — Uncatchable instruction budget (`Fault` sticky)
- **Problem in Aipo:** plugin scripts in games (`aipo-game-host`) or CLI tools must not be able
  to freeze the process with `while true { }`. Aipo already has `VmBudget`, but if a user wraps
  an infinite loop inside an `attempt { ... }` block, a naive error handler could swallow the
  fuel failure and loop forever.
- **Reference in omniLua:** `crates/lua-vm/src/state.rs:SandboxLimits` and
  `docs/SANDBOXING_EXPLORATION.md`.
- **Proposed implementation:** signal fuel exhaustion through an uncatchable
  `VmFault::BudgetExhausted`. When emitted, the dispatcher unwinds the stack unconditionally,
  ignoring any active `attempt` handler until control returns to the host.
- **Success metric:** script `attempt { while true { } }` aborts in under 10 ms with clean host
  output, without freezing the thread.

### Candidate P0-3: AIPO-OMNI-003 — Scoped Rust embedding API (`AipoEngine::scope`)
- **Problem in Aipo:** integrating the Rust host with Aipo scripts (e.g. in the Miniquad engine
  or in plugins) today depends on global registries or the C ABI. There is no safe pure-Rust API
  that lends a mutable host reference (`&mut GameWorld`) for the exact duration of a callback or
  script, with static and dynamic guarantees that the reference does not escape.
- **Reference in omniLua:** `Lua::scope` and `create_userdata_ref_mut` in
  `crates/lua-rs-runtime/src/lib.rs:50-86`.
- **Proposed implementation:** build a high-level pure-Rust façade connected to the generational
  `HandleTable` of `aipo-host`, immediately invalidating handles at scope end. Attempts by the
  script to store the reference in globals raise `AIPO_RT_SCOPE_ESCAPE` or a stale-handle error
  on the next frame.
- **Success metric:** zero `unsafe` added; test coverage proving safe failure when a script tries
  to access the handle outside the `scope` block.
