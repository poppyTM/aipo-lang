# Lessons for Aipo — actionable synthesis and application plan

Synthesis of the studies ([`reference-vms.md`](reference-vms.md),
[`reference-runtimes.md`](reference-runtimes.md)).
**Status:** study record and application plan. Semantic corrections come from Aipo's canon.
External references help understand mechanisms; they have no authority to change the language.
Each proposal becomes an ADP/issue before it becomes code.

---

## 1. What became implementation

| Lesson | Reference | Delivered application | Limit |
| --- | --- | --- | --- |
| Frames have explicit state and lifetime | Wren, Lua, LuaJIT | Handlers isolated per activation; caller restored on return/fault | Still saves 256 registers per call |
| Bytecode fields have specific roles | LuaJIT | B checked per opcode, jumps/pools/calls and iteration with full C | Complete Reg verifier still missing |
| One semantics must not be duplicated in emitters | Shared Aipo frontend | Reg arithmetic/equality delegated to Value; Dict fixed in VM/Reg/JS | General parity still incomplete |
| Counts and buffers require limits | Luau, wasm3 | Bytes rejects invalid categories; Reg has budget; Wasm exposes fuel | Does not cover memory/output/host time |
| Engine errors must reach embedding | Wasm runtimes and Rust contract | MissingExport and writer errors propagated | String binding stays silent on invalid pointers |
| Layouts and gains depend on target/flags | QuickJS, Janet, LuaJIT | Layout probe and reproducible measurement harness | No new quantitative result |
| Core features differ from integration | wasmi, WAMR | Documentation fixes no_std/WASI confusion and keeps optional runner | No alternative runtime integrated |

## 2. RegVm gaps and roadmap (gap list)

| RegVm gap | Informing study | Proposed action |
|---|---|---|
| `MakeClosure` | Lua `OP_CLOSURE` (`reference-vms.md §1.3`); `lvm.c:1935` | Closure captures upvalues through the ordered list, not a flat copy |
| `GetUpvalue` / `SetUpvalue` | Lua `luaF_findupval` (`lfunc.c:87-99`); LuaJIT `func_finduv` (`lj_func.c:36-69`) | O(n) search in ordered list + `MAXUPVAL=255` in 8 bits |
| `FillSelfCapture` | Wren `ObjUpvalue.closed` (`reference-vms.md §3.4`) | On close, hoist value into own slot and repoint |
| `Await` | Wren `ObjFiber.caller` (`wren_value.h:348`); Janet `fiber.c:48-136` | Suspension as fiber switch with `caller`, not OS stack |
| `SealStruct` | QuickJS `JSShape` (`quickjs.c:1108-1140`) | Shared shape; invalid transition becomes a friendly error |
| `AssertInvariant` | Luau `Frontend::check` (`reference-runtimes.md §1.1`) | Invariant as a frontend diagnostic, not an internal `panic` |
| `AssertContract` | Luau `Error.h:32-43,668` | `campo: Tipo` contracts with typed error + summary |
| `CheckMutations` | QuickJS `gc_phase` (`quickjs.c:362`) | Explicit phase/mode during mutable iteration |
| `IterGuardEnd` | Wren loop-exit `patchJump` (`wren_compiler.c:2982-3007`) | Iteration guard with corrected jump and documented limit |
| Register-copy bottleneck | Lua `updatebase` (`lvm.c:1122`); `crates/aipo-vm/src/reg_vm.rs:122-123` | Replace `mem::replace` + `from_fn` of 256 `Value`s per call with base+slots window |

Current implementations of these opcodes live in
`crates/aipo-vm/src/vm/dispatch.rs:928-1274` (`SealStruct`, `AssertInvariant`, `AssertContract`,
`CheckMutations`, `MakeClosure`, `GetUpvalue`, `SetUpvalue`, `IterGuardEnd`,
`FillSelfCapture`) — the study above says **how** they should evolve, not that they are absent.

## 3. Proposals that remain proposals

**Register windows:** store base, limit and return PC per activation, widen slot storage and close
captures before invalidating frames. Requires unwind semantics and call benchmarking, not just
replacing an array with a Vec.

**Shapes/slots/atoms:** separate type identity, interned names and field position; keep fallback
for different layouts and measure cache hits/misses. Do not turn structs into JS-style objects.

**Closures and async in Reg:** implement shared cells, self-capture, await and scheduler per the
existing runtime. Rejecting unsupported programs remains better than omitting instructions.

**Structural versioning:** every add/remove operation increments a version; IterGuard records
version and identity. This closes the gap for size-preserving changes, but requires changing
every mutator and the callback policy.

**Host/sandbox:** bytecode budget and fuel do not limit blocking callbacks, filesystem/process
capacity, memory or output. Reuse Aipo's AHS contract and capabilities; do not assume copying
globals from the Stack VM installs its services in RegVM.

**Alternative Wasm engines:** compare wasmi, WAMR and other candidates with the same modules,
imports and checksums. The wasmi no_std core and its WASI/std adapter are distinct paths.
Selection requires measurements and a documented decision.

**Shell/nano profiles:** audit the graph by features and measure real startup/size; the current
Cargo profile does not deliver no_std. Keep profiles in the same workspace and preserve one
semantics.

## 4. Dispatch floor (candidates)

- Working target: **~100 ns/opcode** (order-of-magnitude estimate for calibrating benchmarks,
  **not** measured in this study).
- Verified ladder in the sources: `switch` (Lua, `lvm.c:1199-1204`) → computed goto with
  fallback (Wren, `wren_vm.c:890-918`; Janet, `vm.c:54-68`) → generated assembly (LuaJIT,
  `vm_*.dasc`).
- Recommendation: RegVm starts on `switch`, adds computed goto behind a feature/flag like Wren,
  measures; DynASM only with profiler evidence.

## 5. P2 — profile matrix (nano/shell/full)

| Profile | Now | Later | Design rule |
|---|---|---|---|
| `nano` | Small/fast Linux | MCU | `no_std` from the start; representation trio like Janet (`janet.h:540-577`) |
| `shell` | Interactive CLI | — | Optional computed goto; threshold GC (QuickJS, `quickjs.c:1687-1696`) |
| `full` | wasmtime allowed | — | Aggressive optimizations isolated by feature |

## 6. P3 — Wasm substrate

| Candidate | State in this study |
|---|---|
| `wasmtime` | Full-only (~25 MB); never in nano/shell |
| `WAMR` | **Experiment candidate**: WASI + `RISCV32` + flags (`FAST_INTERP`, `MINI_LOADER`) — `reference-runtimes.md §4` |
| `wasm3` | Smaller (~64 KB / 10 KB RAM) + deterministic profile as a replay model |
| `wasmi` | Rust-native alternative with `wasmi_wasi` (`wasip1`); decide in the spike |

P3 spike measures all three with WASI enabled before any ADP (criteria: KB, `RISCV32`,
determinism, `no_std`, metering).

## 7. P4 — anti-fragmentation rules

1. **One semantics / N emitters:** studies never create a dialect; only an ADP changes
   semantics.
2. **Tier 1/2/3 with friendly errors:** Tier 1 = core; Tier 2/3 = refusals with an actionable
   message (Luau `Error.h` model), never silent.
3. **Conformance corpus as referee:** divergence between profiles = bug in the corpus, not
   "profile behavior".
4. **Profiles via `features` + `[profile.*]`, never new crates:** nano/shell/full are flag
   combinations, not forks.
5. **CI matrix `full/shell/nano`:** every PR runs all three profiles; failure in one profile
   blocks merge.

## 8. Continuity order

1. Run, in a future authorized round, conformance and the gates of this reconstruction.
2. Complete Reg verifier, structural versioning and host/output policies.
3. Parity of methods, closures, async, hooks and structural contracts.
4. Windows/slots and caches with paired benchmark, without transporting upstream results.
5. Nano/shell profile and alternative-engine spike with license and portability decision.

The [usage and reimplementation guide](runtime-hardening-guide.md) contains APIs, compatibility
changes and steps for each fix. The [master guide](llm-study-guide.md) keeps the methodology and
acceptance criteria that are still uncertified.
