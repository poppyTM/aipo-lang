# Continuity of studies: verification, calls and embedding

**Status:** directed reading applied · **Round:** P07-G02 · **Date:** 2026-10-09.
**Corpus:** the nine commits of `studies/refs.json`; no implicit repin to upstream HEAD.

This round continued the [master guide](llm-study-guide.md) with reading of the regions below.
There were no builds/benchmarks of external runtimes and no full audit of their files. Inferences
for Aipo are separated from properties observed in source.

## Tier A: call cost and host boundary

| Pinned reference | Region read | Code observation | Application in Aipo |
|---|---|---|---|
| [Wren](https://github.com/wren-lang/wren/blob/99d2f0b8fc2686134b32b18166e037639f7e9f2c/src/vm/wren_vm.c#L1392) | `wrenMakeCallHandle`, `wrenCall`, 1392–1469; declarations in `wren.h` | Call handle holds stub/closure; the API prepares slots and distinguishes host-stack execution | Explicit boundaries, refusal of C reentry during callback and value ownership; we do not copy its GC |
| [QuickJS-ng](https://github.com/quickjs-ng/quickjs/blob/dad13e33cee39b910de2e45ea940c08baf42dbf9/quickjs.c#L2436) | `JS_SetMemoryLimit`, `JS_SetInterruptHandler`, `__js_poll_interrupts`, 2434–2446, 2468–2478, 8655–8675 | Allocator limit and interrupt callback are separate mechanisms; polling uses a counter | Guest budget is not a memory/wall-clock limit; separate Wasm limits and external callback under host control |
| [Lua](https://github.com/lua/lua/blob/0b29f408433e92953cc72b1d3e06c7ac8139e439/ldo.c#L717) | `luaD_precall`, `luaD_poscall`, 717–742 and 607–629; protected-execution sites | Activation uses `maxstacksize`/CallInfo and return restores caller; C entry points are distinct | Live Reg window and caller restoration, without blindly copying 256 slots |
| [LuaJIT](https://github.com/LuaJIT/LuaJIT/blob/c6ffc141a8762b41703f9287d63d93622a13dd8f/src/lj_frame.h#L21) | Frame macros/layouts, 21–75; unwind declarations in `lj_vm.h` | Conditional layouts `LJ_FR2`; base/top and metadata depend on configuration | Measure target layout and preserve frames; do not transport sizes/NaN boxing to Rust without an experiment |

**Hypothesis measured in Aipo:** removing the total copy and deep cloning per call should reduce short-call cost. The benchmark uses the same source on baseline and candidate, returns Int or String actually used and measures the whole module. Result and limits are in [P07-G02](../evidence/P07-G02/README.md). It is not a Lua/Wren/QuickJS/LuaJIT benchmark.

## Tier B: proofs and resumable execution

| Pinned reference | Region read | Code observation | Application in Aipo |
|---|---|---|---|
| [Luau](https://github.com/luau-lang/luau/blob/74f768309c380d40601b28c4c4cdda27da415cef/Bytecode/src/BytecodeBuilder.cpp#L1620) | `validate`, `validateInstructions`, 1620–1692 | Validates register/window, constant type and destination boundary; separate captures/variadic passes | RegVerifier validates each operand's role, including unreachable; additional defined-initialization and scope analysis through the CFG |
| [Janet](https://github.com/janet-lang/janet/blob/246951c8090aa714342be246a7abdb7312cf7dae/src/core/vm.c#L1603) | `janet_continue`, `janet_continue_signal`, `janet_pcall`, 1603–1648; signals in header | Resuming a fiber checks state; a protected call delivers signal and result explicitly | C begin/pump/abort API, busy runtime and explicit completion/failure; shared canonical scheduler |

The Luau validator observed uses internal asserts; Aipo receives raw C buffers and returns an error before executing. Definite assignment and handler modeling are Aipo's own implementation based on Aipo's invariants, not copies of that Luau region.

Aipo's pump measures quanta, not preemptive execution of natives. Abort keeps completed effects. The event loop must return control between pumps; there is no deadline for a blocking callback.

## Tier C: Wasm resources by configuration

| Pinned reference | Region read | Code observation | Application in Aipo |
|---|---|---|---|
| [wasm3](https://github.com/wasm3/wasm3/blob/28ecb9af6d2040e474a70f7cb7f43666740141fb/source/m3_env.c#L252) | `m3_NewRuntime`/`m3_SetValidation`, 215–240; `m3_SetResourceLimit`, 252–304 | Configurable stack; memory/table/continuation/gas limits depend on build | Compare capabilities per configuration; do not announce equal budget/suspension across every build |
| [WAMR](https://github.com/wasm-micro-runtime/wasm-micro-runtime/blob/38b044a676893e678ceb19babaa431ca5e460114/core/iwasm/include/wasm_export.h#L219) | `RuntimeInitArgs`, 219–260; `wasm_runtime_create_exec_env`, 960–973; `wasm_runtime_terminate`, 1360–1375 | Init has allocator/mode options; exec_env receives stack; terminate is a separate boundary | Future choices must measure allocator/stack/WASI and lifetime; no integration or winner selection happened |
| [wasmi](https://github.com/wasmi-labs/wasmi/blob/2970aa871cc1001b57b267ccecdcd1e42306199e/crates/wasmi/src/engine/config.rs#L349) | `Config::consume_fuel`, 347–369 | Opt-in fuel instruments execution; Store receives fuel before execution | Document instrumentation/consumption and separate memory/output; current implementation remains Wasmtime |

## Decisions and limits

- Verifying operands/CFG before optimizing allows reusing the proof only in the matching image.
- Activation metadata determines windows; semantics remain in `Value` and canonical services.
- Guest snapshot preserves aliases through in-place restoration; it does not undo external resources.
- Wasm `is` needs semantic proof; a register's physical type is not that proof.
- Profiles remove dependencies by features, without duplicate crates or different languages.

Still pending: full audit of GC/cycles, cold start/RSS, native allocator limits, Miri/fuzz/ASan and executed conformance. Tests in this round were explicitly not executed. Compilation and microbenchmark do not replace those gates.
