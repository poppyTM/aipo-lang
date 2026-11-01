# Aipo — master guide for LLMs: deep study and implementation

**Version:** 2.2 · **Date:** 2026-10-09 · **Status:** normative-operational
**Audience:** any LLM that will execute deep study and implementation in this repository.
**Prerequisite:** read this document IN FULL, in this order, before any action.
**Base studied:** commit `488905ffb0b83ebbff458939542350d7280ad79b`.
**Integration base:** `bcbc4c991dee1d9f853c597aaed2790e8981b8b9`, after incorporating the
dependency update already published on `main`.

**Current round P07-G02:** [complete usage/reimplementation guide](../development/runtime-and-tooling-guide.md),
[continuity of studies](runtime-and-tooling-follow-up.md) and
[reproducible evidence](../evidence/P07-G02/README.md). Base:
`f0a0d70d176be031cd4b600fde1df6179429fb9a`, after merging P07-G01. Tests explicitly not
executed; goal stays DRAFT without simulated approvals/gates. Previous documentation is historical.

---

## 0. How to use this document

1. Read sections 1–4 for context and rules (mandatory, no exceptions).
2. Execute section 5 (fetch the corpus) and verify the lock.
3. Study in the order of section 6 (Tier A → B → C), filling the template in section 7.
4. Implement following section 8 (proposals per dimension) and the recipes in section 10.
5. **Definition of "done":** section 12 (output contract) 100% satisfied.

> Path convention: `$AIPO_ROOT` = root of this repository; `$AIPO_REFS` = reference clones
> (default `~/aipo-refs`, via `AIPO_REFS_DIR`). Never hardcode `/home/...`.

---

## 1. Mission, non-goals and philosophy

**Mission:** study reference VMs/runtimes in depth and implement the missing pieces (RegVm,
`aipo-wasm`, embedding profiles, C/Rust interop), **without fragmenting the language or the
repository**.

**Non-goals:** redesign syntax (frozen by ADR-001); create crates per profile; change the
Model B failure model; add heavy dependencies without an ADP.

**Philosophy (non-negotiable, source: ADR-001, ADP-012, `docs/llm/`):**
- **ND-first:** every surface decision is reviewed for cognitive overhead (ADHD, dyslexia).
  One form per thing; complete word beats symbol; zero ceremony for the common case; the error
  says what to do.
- **Determinism:** same input, same output (VM single-thread by decision; `Value` is `!Send` —
  one runtime per thread).
- **No-invention:** a semantic question without a canon answer becomes an ADP/issue, never
  silent code.
- **Evidence-first:** performance/security claims require attached measurement
  (`scripts/perf/paired.sh` + `cpu_ab.py`: ≥2 batches, ±5% = noise).

---

## 2. Project snapshot

Pipeline: `lexer → syntax → hir → sema → ir → bytecode → {stack-VM, RegVm, JS, Wasm}`.
Toolchain: MSRV Rust 1.96, aligned with the Wasmtime 49.0.2 / Cranelift 0.136.2 lock. Local
compilation with Rust 1.96 and Clippy review with 1.99; Node >= 20 for JS suites. Tests not
executed in this round. 24 crates in the workspace (`unsafe_code = "forbid"`, clippy `deny`
everywhere).

| Crate | Role |
|---|---|
| `aipo-lexer/syntax/ast/hir/sema` | Shared frontend (single semantics) |
| `aipo-ir` | Neutral Core IR (43 instructions) |
| `aipo-bytecode` | Stack emitter + `RegEmitter` + `RegOpCode` u32 |
| `aipo-vm` | Stack VM (reference) + `RegVm` + `Value` (layout per target) + `VmMetrics` + budget |
| `aipo-js` | `emit_js` → `JsBundle` (differential parity with the VM) |
| `aipo-wasm` | `compile_hir`, `WasmEmitter`, `execute_wasm`/`execute_wasm_with_options` (wasmtime feature-gated) |
| `aipo-cli` | Bins `aipo` (full) and `aipo-sh` (shell, REPL); `test` subcommand |
| `aipo-host` | `Capability`/`CapabilitySet` (deny-by-default), `HostValue` (7 variants), `Handle` |
| `aipo-c-abi` | Synchronous/cooperative C ABI + raw Reg; exports must match `include/aipo.h` (see §8.5) |
| `aipo-diagnostics` | Normative catalog + JSONL + En/PtBr locale |
| others | `runtime, stdlib (sh module: run/cd/pwd/env/which), formatter, package, poppy, testkit, bench, game-host, egui` |

**Practical warnings:** `target/` exceeds 40 GB (if `ENOSPC`, delete
`target/debug/incremental`); builds with wasmtime take minutes (timeout 300 s+); prefer
`cargo test -p <crate> --test <suite>`.

---

## 3. Iron rules

### 3.1 Anti-hallucination
1. Every fact cites `path:line` read via Read/Grep. 2. `ls` before assuming any path.
3. Never invent APIs/flags. 4. Numbers only with a method attached or marked "estimate".

### 3.2 Syntax taboos (ADR-001)
| FORBIDDEN | Use |
|---|---|
| `end` | `}` |
| `for x in xs`, `repeat N:`, `while` with `do` | `each x in xs { }`, `repeat N as i { }`, `while c { }` |
| `// comment`, `/* */` | `#` (`//` is integer division!) |
| `try/catch/throw`, `defer` | `attempt { } failed e { }`, `fail`, `?`, `or_else` |
| `class`, `impl T { }`, `satisfy`, `self!`, `div`, `&& || !`, `;`, `?:` | `struct`, `T:m()`, `#!satisfies`, `var self`, `//`, `and or not`, line break, `if c then a else b` |
| `else if`, `fn T:m()` | `elif`, `T:m()` |

### 3.3 Repository rules
- Do NOT commit/push without explicit request. Docs in PT-BR, code in EN.
- Grammar/semantics/IR/bytecode/diagnostic/CLI changes only with a conformance fixture + tests
  (`docs/conformance/`).

---

## 4. Implementation state

### 4.1 Ready (verify, do not redo)
Syntax T1–T7; V1 `enum` (VM+JS+Wasm); `?`, `with`, destructuring+guards; diagnostics §4bis +
narrowing; batch `Tipo::[...]`; RegVm (arithmetic, jumps, calls, defaults, `each`, `TypeIs`,
collections/structs, failures, handlers); Wasm (enum via tag-pointer, empty `[]`/`{}`,
`.len()`); `aipo-sh --engine=reg`.

### 4.2 Current implementation and limits

- Strict RegEmitter + RegVerifier: operands/pools/jumps/functions, CFG-defined initialization
  and handler/iteration scopes. External entries are verified before effects.
- `ExecutionPlan` represents native Reg or canonical bytecode with a reason. The Reg profile
  runs the shared surface; captures, async, interfaces, hooks and journal have not gained
  parity in the native Reg interpreter.
- Reg calls move/restore live windows; bodies use Rc and per-module prepared guest constants.
  The Int/String microbenchmark against the pinned baseline, three batches, is in the P07-G02
  evidence; do not extrapolate to the whole language.
- Collections use structural revision, independent of length. Guest snapshot preserves
  aliases/cycles through in-place restoration; it is not a GC and does not roll back external
  host resources.
- Session keeps globals/catalog/linker, multiline/history and reload with migration/rollback.
  `new`, `watch`, `plan`, `profile`, `debug`, basic LSP and offline vendor are implemented.
- Reg/C have budgets and explicit errors; C offers begin/pump/abort. Wasm offers fuel/memory/
  output and reusable Engine/cache. Native callback is not limited by the opcode budget.
- Wasm `is` compares a known semantic type; unknown types/ambiguous joins are rejected.
- `Value`/frames are measured per target; no multi-platform promise of 16 bytes.
- `no_std`, cycle GC, fully limited guest allocator and MCU port remain future evolution.
  `profile-embedded` and `nano` use `std`.
- New tests and conformance were written/compiled, not executed. The OS matrix is defined, not
  certified by local results.

### 4.3 Available studies

[reference-vms.md](reference-vms.md), [reference-runtimes.md](reference-runtimes.md) and
[lessons-for-aipo.md](lessons-for-aipo.md) record directed reading and actions. They do not
certify a full upstream audit or compilation of external runtimes.

**Premise corrections:** WAMR is not a measured or sole WASI candidate. wasmi has a `no_std`
core and a separate WASI adapter, with `std`. The ADR-001 and ADP-012 cited in the previous
version are not materialized in this checkout; apply the existing canon/authority map, without
inventing files or decisions.

---

## 5. Reference corpus

Fetch: `./studies/fetch-refs.sh` → `$AIPO_REFS` (**outside the repo, by decision**).
Lock: `$AIPO_REFS/refs.lock.json`. Re-pin only consciously, with a date.

| Ref | Repo | Branch | SHA 2026-10-08 | License | Tier | Focus |
|---|---|---|---|---|---|---|
| `lua` | `lua/lua` | `master` | `0b29f408433e` | MIT (`COPYRIGHT`) | A | `TValue`, upvalues, `lvm.c` |
| `luajit` | `LuaJIT/LuaJIT` | `v2.1` | `c6ffc141a876` | MIT/X custom | A | register windows, dispatch, `lj_func.c` |
| `wren` | `wren-lang/wren` | `main` | `99d2f0b8fc26` | MIT | A | minimal VM, single-pass, fibers |
| `quickjs` | `quickjs-ng/quickjs` | `master` | `dad13e33cee3` | MIT | A | refcount+cycles, shapes, atoms |
| `luau` | `luau-lang/luau` | `master` | `74f768309c38` | MIT | B | gradual typing, diagnostics, `buffer` |
| `janet` | `janet-lang/janet` | `master` | `246951c8090a` | MIT | B | tagged union, fibers, C core |
| `wasm3` | `wasm3/wasm3` | `main` | `28ecb9af6d20` | MIT | C | limits and flags; size to measure |
| `wamr` | `wasm-micro-runtime/wasm-micro-runtime` | `main` | `38b044a67689` | **Apache-2.0** | C | interp/AOT/JIT flags and optional WASI |
| `wasmi` | `wasmi-labs/wasmi` | `main` | `2970aa871cc1` | MIT/Apache-2.0 | C | no_std core; WASI separate |

Layouts that deviate from the obvious: `lua/` is flat at the root; QuickJS-ng has no
`internals.md` (read in source); Luau is nested (`VM/src/`…); Janet is in `src/core/`;
WAMR is in `core/iwasm/interpreter/`.

---

## 6. Methodology

Order: Wren → QuickJS → Lua → LuaJIT → Luau → Janet → runtimes.
Template per language: *What it is / How it works (real code) / Closed questions /
Lesson → Action (with crates)*. Without "Lesson → Action", it is passive reading and does
not enter.

---

## 7. Briefs (exact files + questions)

- **Lua** (`lobject.h, lstate.h, lvm.c, lfunc.c, lgc.c`): `TValue` layout in 64-bit? ordered
  upvalue list? worst case of `findupval`? `switch` vs `goto` in `luaV_execute`?
  `MAXUPVAL=255` fits in u32?
- **LuaJIT** (`src/lj_obj.h, lj_func.c, lj_bc.h, lj_gc.c, lj_vm.h`): frame base + slots vs
  our copy? BC word format? `GCRef` 32-bit vs `Rc`?
- **Wren** (`src/vm/wren_vm.c` in full + `wren_compiler.c, wren_value.h`,
  `src/include/wren.h`): `Value` width? frames? fibers vs `Task`? jump patching? inline
  caches?
- **QuickJS-ng** (`quickjs.c, quickjs-opcode.h, libregexp.c`): shapes ≈
  `struct_fixed_fields`? atoms ≈ interned fields? cycle-removal trigger?
- **Luau** (`Compiler/, Analysis/, VM/`, ≤8 files): typechecker entry, `buffer` ≈ `Bytes`,
  diagnostic format.
- **Janet** (`src/core/vm.c, gc.c, fiber.c`, `src/include/janet.h`): union (3 modes?),
  mark by type, fibers.
- **wasm3** (`source/m3_config.h`, dispatch): sizes, deterministic profile.
- **WAMR** (`core/iwasm/`, build options): WASI cost, smallest config.
- **wasmi** (`crates/`): separate no_std core from the WASI/std adapter and measure per
  configuration.
- **Tcl/PS/Nushell:** docs-only (reentrancy; value pipelines; `pipe2`).

---

## 8. Implementation proposals by dimension

Format of each item: *What / Why (source) / Where / Ready-when.*

### 8.1 Performance
- **P1 Register windows (Lua/LuaJIT):** live window with move/restore and shared constants
  implemented in `reg_vm.rs`; P07-G02 evidence with three batches. Not yet a single frame
  stack without copy; executed conformance still pending.
- **P2 Dispatch:** threaded/computed `goto` (Wren with fallback, WAMR classic-vs-fast) +
  narrowing `Result` in the hot path (cf. ADP-011, floor ~100ns).
- **P3 Field access:** end linear search (`value.rs:93,109`) via shapes/slots/atoms
  (QuickJS); verify/extend slot cache.
- **P4 Strings:** intern field/type names; audit NFC cost (feature `unicode`).
- **P5 Methods:** extend allocation-free `StructMethod` + monomorphic inline cache.
- **P6 Bytecode:** constant dedup + fusion (`GetField+Call`); `Call0..4`.
- **P7 Optional NaN-boxing** (precedent: Janet has 2 modes) as a tier-gated experiment.

### 8.2 ND accessibility
- **A1** Audit catalog: every error with a filled `suggestion`.
- **A2** All new code requires En+PtBr strings (checklist §10).
- **A3** ND gate in syntax ADPs (ADHD/dyslexia rationale mandatory).
- **A4** Canonical formatter + idempotency in CI (choice-overload reducer).
- **A5** Friendly REPL (multiline, history, errors with suggestion) — verify/extend.
- **A6** Readable `aipo test` + reading pass in the manual.
- **A7** Canonical vocabulary list (one term per concept).

### 8.3 Speed/startup
- **V1** Cold-start baseline per profile (target: shell <2 ms).
- **V2** Aliases `profile-shell`, `profile-web`, `profile-embedded` with
  `--no-default-features` implemented; `nano` is a Cargo profile. Default still includes
  Wasmtime.
- **V3** Lazy stdlib/native registration (measure current eager cost).
- **V4** Pre-compiled `.aibc` + cache (verify/extend serialization).
- **V5** AOT is **not** a startup strategy (compilation cost); fast startup = interpreter
  (WAMR spike).

### 8.4 Security
- **S1** Supply chain: keep deny/audit/review; audit the nano-profile dependency graph;
  lockfile policy.
- **S2** Fuzz: add to the 3 current targets bytecode verifier, VM step with corpus,
  VM-vs-RegVm differential, AHS validation.
- **S3** Capabilities: extend to new surfaces; tier by profile; fuzz the AHS.
- **S4** Input robustness: ADP-005 + size limits; depth guards in RegVm.
- **S5** Checklist per C export (null, UTF-8, reentrancy, unwind) — formalize the
  `c_api.rs` pattern.
- **S6** Budgets everywhere: bring the Stack VM/C ABI budget to RegVm and the wasm runner
  (fuel).
- **S7** Keep `unsafe_code = "forbid"` + Miri for RegVm in CI.
- **S8** Secret scanning (cf. threat-model) — verify the hook and extend.

### 8.5 C interop (header as the surface source)
Groups: lifecycle, load/call, capabilities, handles/values, budgets/errors, raw Reg and
cooperative begin/pump/abort. Compare the symbols of `c_api.rs` with `include/aipo.h`; do not
keep an outdated textual count.
- **C1 Delivered/limits:** begin/pump/abort and Reg budget/error; header compiled as C/C++
  in the evidence. Streaming, callback versioning and executed native proofs remain pending.
- **C2 Verified patterns:** Wren slots+handles (`wrenEnsureSlots`, `WrenHandle`); QuickJS
  runtime/context + `Dup`/`Free` (explicit ownership); WAMR `exec_env`+`custom_data`+natives
  per module; Lua `lua_State` per thread.
- **C3 Proposals:** variadic API by slots; header-vs-impl compat test; documented thread
  policy (`Value: !Send` → 1 runtime/thread); reentrancy policy; error-ownership rules;
  callback versioning.
- **C4** Natives namespaced per module (WAMR pattern).

### 8.6 Rust interop
- **R1** Minimal façade (`aipo::eval`), documented semver/MSRV policy.
- **R2** Surface budget + `VmMetrics` in the public API.
- **R3** `std::error::Error` + optional reports; keep JSONL.
- **R4** `no_std` roadmap (split core/alloc), dependency audit per feature.

### 8.7 Other surfaces
- **JS:** bidirectional callbacks (verify), shim versioning, source maps.
- **Formatter/test/REPL/i18n:** completeness, idempotency, synchronized EN mirror (checklist
  for every visible string), documented determinism.
- **Packages/observability:** registry audit; per-profile metrics; structured logs.

---

## 9. Synthesis + P2/P3/P4

- Gap→study: each §8 item references the lesson that informs it.
- **P2:** nano = native build with size optimization; size/startup require real measurement.
  MCU/`no_std` not implemented.
- **P3:** wasmtime feature-gated; compare WAMR and wasmi spikes before choosing an engine.
- **P4:** 1 semantics/N emitters; tiers 1/2/3 with friendly errors; conformance as referee;
  profiles via features+`[profile.*]`, never new crates; CI `full/shell/nano`.

---

## 10. Implementation recipes

**New instruction in RegVm:** `RegOpCode` (+`from_u8`) → lowering in `RegEmitter` (A/B/C +
`top`, without breaking `core_to_reg`) → operands/CFG in `RegVerifier` → handler in
`RegVm::run_verified` → unit test in `reg_vm.rs` → e2e in `shell_tests.rs`
(`eval_source_reg`) → conformance → fmt+clippy.

**New construction in Wasm:** layout (Pass 0) → static string → `compile_expr` → slots in
`pre_scan_stmts` → real Wasmtime test.

**New C export:** checklist S5 + ABI-pin test + entry in `aipo.h`.

**New diagnostic:** code + En+PtBr strings + fixture + entry in the catalog.

**New feature gate:** feature + CI matrix + friendly error when absent (P4.2).

---

## 11. Phases and acceptance

| Phase | DoD |
|---|---|
| 1 Infra | clean fetch; lock 9 SHAs; diff only `studies/`+`docs/studies/`+sidebar |
| 2 Tier A | 4 sections, facts with `path:line`, Lesson→Action each |
| 3 Tier B/C | same + wasm table with verified flags |
| 4 Synthesis | `lessons-for-aipo.md` + ADPs/issues |
| 5 Implementation | §8 by dimension, each item with test + green conformance |

---

## 12. Output contract

1. Created/changed files + diff stat. 2. Verification outputs.
3. Deviations from this guide + facts contradicting §4. 4. Explicit pending items.
5. Never commit/push without request.

## Appendix — sources and verification limits

Use symbols, not old line numbers: `RegEmitter::stack_heights/compile_function`,
`RegVm::run/invoke_function/set_instruction_budget`, `value::string_char_at`,
`execute_wasm_with_options` and `studies/refs.json`. The studies' links include the upstream SHA
and the regions actually read.

Section 11 keeps the future certification gates. The current publication is a draft, with
explicit waiver of test execution; it does not equal approval of all phases of this guide.

## P07-G02 continuity

The [reimplementation guide](../development/runtime-and-tooling-guide.md) details the five fronts,
the migration of Rust payloads to `Collection`, explicit engine selection, the linker/snapshot
and distribution limits. The recipes and future goals on this page must not be announced as
delivered features. Approval for commit/push/PR was given by the user; test execution was
explicitly waived. No historical or lost result certifies the current files.
