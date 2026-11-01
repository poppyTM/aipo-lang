# Reference runtimes — Tier B + C

**Round:** 2026-10-09. [Nine pinned references](../../studies/refs.json), directed reading of source, no builds/benchmarks of upstreams. Third-party announced sizes or speeds are not measurements of this revision. Luau and Janet (cloned and read outside the repo via `./studies/fetch-refs.sh`), docs-only notes on Tcl/PowerShell/Nushell (**no clone** — focus on host patterns), and analysis of WebAssembly runtimes. Pinning in `studies/refs.json` and `refs.lock.json`.

> Layout deviations recorded:
> - Luau: `Compiler/`, `Analysis/`, `VM/` contain `include/` + `src/` (e.g. `VM/src/lbuffer.h`).
> - Janet: core in `src/core/*.c` + API in `src/include/janet.h` (`vm.c`, `gc.c`, `fiber.c`, `bytecode.c` in `src/core/`).
> - WAMR: interpreter in `core/iwasm/interpreter/` (`wasm_interp_classic.c` / `wasm_interp_fast.c`); docs in `doc/`.
> - wasmi: crates in `crates/` (`wasmi`, `wasi`, `cli`, `core`, `ir`, …).

---

## 1. Luau (`luau-lang/luau`, `master`)

**What it is:** language and runtime with compiler, static analysis and VM in separate components.

**How it works:**
- [`VM/src/lobject.h`](https://github.com/luau-lang/luau/blob/74f768309c380d40601b28c4c4cdda27da415cef/VM/src/lobject.h#L46) defines `TValue` with payload, extra fields and tag.
- [`lbuffer.h`](https://github.com/luau-lang/luau/blob/74f768309c380d40601b28c4c4cdda27da415cef/VM/src/lbuffer.h#L6): limit `MAX_BUFFER_SIZE (1 << 30)`, layout `offsetof(Buffer, data) + max(len, 8)`, constructor/destructor `luaB_newbuffer / luaB_freebuffer`, with implementation in `VM/src/lbuffer.cpp` and stdlib in `VM/src/lbuflib.cpp`. Buffer is a concrete GC category of bytes with a read/write API, distinct from `String`.
- `Analysis/` implements constraint-based inference (`ConstraintGenerator`, `ConstraintSolver` in `Analysis/src/`), with public boundary in `Analysis/include/Luau/Frontend.h`: `struct Frontend`, `check(name)` returning `CheckResult`, and the error catalog in `Analysis/include/Luau/Error.h` (`TypeMismatch`, `TypeErrorData = Variant<…>`, `TypeErrorSummary`).
- `Compiler/src/` contains named passes: `ConstantFolding`, `BuiltinFolding`, `ValueTracking`, `TableShape`, `CostModel`, with entry `compile(...)` in `Compiler/include/Luau/Compiler.h`.

**Closed questions:**
- Buffer is a concrete runtime category, with explicit limits; it is not just text under another name.
- Gradual types work as contracts diagnosed by the frontend (`field: Type` ⇒ diagnostic error, never silent runtime coercion).
- Optimizations are small, testable passes, not a monolithic flag.

**Lesson → action in Aipo:**
- Aipo keeps `Bytes` distinct from `String`; RegVM requires an `Int` index and a `Byte` value before writing. Check limits and layout against `lbuffer.h`. Crates: `aipo-vm`, `aipo-stdlib`.
- Type contracts: `field: Type` checked in the frontend with diagnostics in the style of `Error.h` (typed variant + summary), without changing runtime semantics. Crates: `aipo-compiler` (diagnostics), `aipo-vm` (`AssertContract`).
- Folding passes: `ConstantFolding`/`BuiltinFolding` as a model of incremental passes in the emitter. Crates: `aipo-compiler`. Deeper study of `Analysis`/`Compiler`, diagnostics and incremental GC remains a proposal.

---

## 2. Janet (`janet-lang/janet`, `master`)

**What it is:** lean C runtime with VM, fibers and GC, with a configurable value representation.

**How it works:**
- [`janet.h`](https://github.com/janet-lang/janet/blob/246951c8090aa714342be246a7abdb7312cf7dae/src/include/janet.h#L311): 17 types (`JANET_NUMBER … JANET_POINTER`). Triple representation — `JANET_NANBOX_64` (u64/i64/number/pointer), `JANET_NANBOX_32` (payload + u32 tag) and struct fallback `{ as; type; }`. Verification via `janet_checktype`/`janet_type`.
- Lean C core: `src/core/` totals ~5.1k lines across 4 files (`vm.c` + `bytecode.c` + `gc.c` + `fiber.c`). Proof that VM+GC+fibers fit in a compact architecture.
- VM (`src/core/vm.c`): `VM_START` = `goto *op_lookup[first_opcode]`, `vm_next()` = `goto *op_lookup[*pc & 0xFF]`; fallback `switch(opcode)` in `for(;;)`. Confirms that the `switch` → computed `goto` step is mechanical and reversible.
- Fibers (`src/core/fiber.c`): `janet_fiber_reset` / `janet_fiber` / `janet_fiber_push`; the stack grows via `setcapacity`/`janet_fiber_grow`. [`vm.c`](https://github.com/janet-lang/janet/blob/246951c8090aa714342be246a7abdb7312cf7dae/src/core/vm.c#L1321) exposes step/continue for fibers.
- GC (`src/core/gc.c`): marking by type (`janet_mark_*`). [`janet_collect`](https://github.com/janet-lang/janet/blob/246951c8090aa714342be246a7abdb7312cf7dae/src/core/gc.c#L578) is the explicit entry point to study roots and collection.

**Closed questions:**
- Compact representation depends on the target and pointer premises.
- Fibers require their own scheduler; a C fiber is not automatically equivalent to Aipo's deterministic `Task` scheduler.
- The `switch` → computed `goto` step is viable where the C compiler supports it.

**Lesson → action in Aipo:**
- Keep NaN boxing as a separate experiment, with gates and portability justification; the three-mode model (nanbox64/nanbox32/struct) is the reference if Aipo needs a 32-bit build (nano profile).
- Preserve Model B failures when studying suspension. `fiber.c` serves as a minimal conceptual model for `Task` (reset/push/grow). Crates: `aipo-vm`.
- Core budget: ~5k lines as a conceptual complexity ceiling for `aipo-vm`.

---

## 3. Docs-only notes (no clone)

Tcl, PowerShell and Nushell **were not cloned** — studied through official documentation to
calibrate the Aipo host:

- **Tcl (host reentrancy):** the Tcl interpreter is reentrant by design (`eval` nesting,
  recursive safe `Tcl_Eval`). Point for the AHS host: host→Aipo→host calls must have bounded
  depth and per-invocation error state, not global.
- **PowerShell/Nushell (structured-value pipelines):** commands exchange structured/typed
  objects, not raw text. Point for Aipo: the `task`/stream stdlib must carry typed `Value`
  between stages, with a verifiable shape contract — converging with QuickJS shapes.

**Lesson → action in Aipo:**
- Reentrant host with depth budget + per-invocation error. Crates: `aipo-host`, `aipo-vm`.
- Typed pipelines end to end. Crates: `aipo-stdlib`, `aipo-vm`.

---

## 4. WebAssembly runtimes

### 4.1 Comparative table

| Runtime | Size / profile | WASI | Relevant configuration | Role in Aipo |
|---|---|---|---|---|
| `wasmtime` (external reference) | ~25 MB (full-only) | yes | Standard JIT runner | Full ceiling; optional runner; never in nano/shell |
| `wasm3` (`main`) | ~64 KB code / ~10 KB RAM announced; stack `d_m3MaxFunctionStackHeight 8000` | via `m3_api_wasi.c` | Flags in `source/m3_config.h` | Deterministic profile model; nano candidate if WASI suffices |
| `WAMR` (`main`) | Small footprint, multiplatform; RISC-V 32/64 | yes (libc-wasi, uvwasi) | `WAMR_BUILD_TARGET`, `WAMR_BUILD_FAST_INTERP`, `WAMR_BUILD_MINI_LOADER` | Experiment candidate: WASI + configurable + RISC-V32 |
| `wasmi` (`main`) | Pure-Rust interpreter, `no_std` in core, fuel metering | yes (`crates/wasi`, `wasip1`) | License MIT/Apache-2.0; features in Cargo.toml | Rust-native alternative; separates `no_std` from WASI adapters |

*Methodological note:* footprint numbers ("~64 KB", "95 KB RISC-V32", "25 MB") come from
third-party documentation and vary by flags/target. They must be measured in a benchmark with
the same Wasm binaries before any decision.

### 4.2 wasm3 (`wasm3/wasm3`, `main`)

**What it is:** WebAssembly interpreter with C configuration and its own metacode.

**How it works:** [`m3_config.h`](https://github.com/wasm3/wasm3/blob/28ecb9af6d2040e474a70f7cb7f43666740141fb/source/m3_config.h#L14)
defines stack, memory-page, slot and validation limits. Default function stack
`d_m3MaxFunctionStackHeight 8000`. Documented deterministic profile
(`source/m3_config.h:162-185`, `source/m3_deterministic.h:1-8`) with a fixed clock and
`random_get` with a fixed seed.

**Closed question:** the cost of a runtime is not just its executable size; build limits and
flags are part of the evaluated profile.

**Lesson → action:** any experiment in Aipo must record build flags, memory, validation, imports
and checksum. wasm3's deterministic profile serves as a reference for Aipo's replay mode.

### 4.3 WAMR (`wasm-micro-runtime/wasm-micro-runtime`, `main`)

**What it is:** WebAssembly runtime with classic/fast interpreters, AOT/JIT and configurable host
integrations.

**How it works:** [`runtime_lib.cmake`](https://github.com/wasm-micro-runtime/wasm-micro-runtime/blob/38b044a676893e678ceb19babaa431ca5e460114/build-scripts/runtime_lib.cmake)
selects components; [`config_common.cmake`](https://github.com/wasm-micro-runtime/wasm-micro-runtime/blob/38b044a676893e678ceb19babaa431ca5e460114/build-scripts/config_common.cmake#L301)
conditions the fast interpreter on the interpreter being enabled. Libc WASI and uvwasi are
separate configurations.

| Dimension | Observed gate | Implication for an experiment |
| --- | --- | --- |
| Interpreter | `WAMR_BUILD_INTERP` | Define execution mode |
| Fast interpreter | `WAMR_BUILD_FAST_INTERP` | Compare the same load with recorded flags |
| AOT/JIT | `WAMR_BUILD_AOT`, `WAMR_BUILD_JIT` | Separate startup, compilation and execution |
| WASI | `WAMR_BUILD_LIBC_WASI`, `WAMR_BUILD_LIBC_UVWASI` | Define host imports and permissions |
| Target | `WAMR_BUILD_TARGET` | Do not transport RISC-V sizes to x86-64 |

**Closed question:** there are configurations with and without WASI. WAMR is an experiment
candidate, not an automatic replacement. The license contains parts under
`Apache-2.0 WITH LLVM-exception`.

**Lesson → action:** produce an isolated spike before proposing an engine swap; record imports,
footprint and license per file. No upstream C code is copied into the Aipo repository.

### 4.4 wasmi (`wasmi-labs/wasmi`, `main`)

**What it is:** pure-Rust WebAssembly interpreter, with core and WASI adapter separated in the
workspace.

**How it works:**
- [`crates/wasmi/src/lib.rs`](https://github.com/wasmi-labs/wasmi/blob/2970aa871cc1001b57b267ccecdcd1e42306199e/crates/wasmi/src/lib.rs#L87)
  declares `#![no_std]`, uses `alloc` and enables `std` by feature.
- The [core manifest](https://github.com/wasmi-labs/wasmi/blob/2970aa871cc1001b57b267ccecdcd1e42306199e/crates/wasmi/Cargo.toml#L37)
  includes gates such as `std`, `wat`, `validate`, `memory64`, `auto-dispatch`,
  `portable-dispatch`, `indirect-dispatch` and `deterministic`.
- The [WASI adapter](https://github.com/wasmi-labs/wasmi/blob/2970aa871cc1001b57b267ccecdcd1e42306199e/crates/wasi/Cargo.toml)
  depends on the core with `std`, on `wasi-common` and on `wiggle`.
- The [workspace license](https://github.com/wasmi-labs/wasmi/blob/2970aa871cc1001b57b267ccecdcd1e42306199e/Cargo.toml#L25)
  is dual `MIT/Apache-2.0`.

**Closed questions:** "wasmi with WASI" does not cancel the core's `no_std` support; WASI
integration and core are distinct components.

**Lesson → action:** include wasmi alongside WAMR in the comparison before choosing a runtime for
shell/nano. The existing Wasmtime runner gained fuel/budget support and feature isolation;
alternative engines remain an investigative spike.

---

## 5. Future comparison protocol

Pin the same `.wasm` binary, inputs, checksum and imports; measure cold/warm start separately;
record build flags, dependencies, target architecture, toolchain and memory consumption; verify
licenses and error handling per limit. Do not disable validation in one candidate and keep it in
another without explicitly recording the divergence.
