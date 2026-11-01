# Reference VMs — Tier A

**Round:** 2026-10-09. Sources: [manifest](../../studies/refs.json). Checkouts obtained at full
commits with HEAD and origin verified, read outside the repo via `./studies/fetch-refs.sh`. This
note records directed reading of source, without compiling or turning third-party numbers into
measurements of our implementation. Every factual statement cites file + line read.

> Layout deviations recorded:
> - The `lua/lua` repo keeps sources at the **root** (`lobject.h`, not `src/lobject.h`).
> - `quickjs.c` + `quickjs-opcode.h` are at the **root**; `docs/` is a docusaurus site, with no
>   markdown internals guide.

---

## 1. Wren (`wren-lang/wren`, `main`)

**What it is:** language with a compact VM and C embedding. The fiber, frame and upvalue
architecture provides a concrete reference for execution lifetimes.

**How it works:**
- In [`wren_vm.c`, `captureUpvalue`/`closeUpvalues`](https://github.com/wren-lang/wren/blob/99d2f0b8fc2686134b32b18166e037639f7e9f2c/src/vm/wren_vm.c#L244),
  open upvalues live in a list ordered by slot. Closing an upvalue copies the value into its
  own storage and repoints it. Return closes upvalues of the frame before discarding slots.
- [`CallFrame`](https://github.com/wren-lang/wren/blob/99d2f0b8fc2686134b32b18166e037639f7e9f2c/src/vm/wren_value.h#L284)
  holds IP, closure and stack base. The VM updates local frame caches when the activation changes.
- [`WREN_COMPUTED_GOTO`](https://github.com/wren-lang/wren/blob/99d2f0b8fc2686134b32b18166e037639f7e9f2c/src/vm/wren_vm.c#L890)
  selects dispatch by labels on a compatible compiler; a `switch` fallback exists.

### Verified technical details

- **Core size:** `src/vm/wren_vm.c` has ~1994 lines and 691 `;`; the `src/vm/* + src/include/*`
  set totals ~13k lines with 3721 `;`. The interpreter fits in a focused read.
- **Dispatch in 2 modes:** with `WREN_COMPUTED_GOTO` (`wren_vm.c:890-918`), table
  `dispatchTable[]` (`wren_vm.c:892-896`) + `goto *dispatchTable[...]` (`wren_vm.c:905`); without
  it, classic `switch` (`wren_vm.c:910-916`). This is the intermediate step between pure `switch`
  and generated assembly: same loop, swapped by `#define`.
- **Single-pass compiler:** `emitJump` (`wren_compiler.c:1362-1367`) emits opcode + 16-bit
  placeholder; `patchJump` (`wren_compiler.c:1756-1766`) computes the delta and validates
  `jump <= MAX_JUMP (1 << 16)`. Forward jumps without IR, with a 64 Ki instruction limit per jump.
- **Fibers as a primitive:** `ObjFiber` (`wren_value.h:316-355`) keeps a heap stack, dynamic
  frames, its own open-upvalue list, and a `caller` pointer to resume after `yield`
  (`wren_value.h:348`). `ObjUpvalue` (`wren_value.h:178-197`) hoists the value into `closed` on
  close.
- **Method dispatch:** by class + signature, no inline caches in the central interpreter.

**Closed questions:** capture requires an explicit relation between slot and lifetime; return must
close captures before invalidating slots; computed goto is a build-conditioned choice, not a
feature to be copied literally into safe Rust.

**Lesson → action in Aipo:**
- RegVM isolates handlers and restores caller state; next step: model frames with base/slots and
  capture cells, keeping canon semantics.
- **Dispatch in 2 modes:** canonical `switch`, with computed `goto` behind a feature/flag as a
  future paired benchmark. Crates: `aipo-vm`.
- **Fibers vs `Task`:** scheduler may mirror `ObjFiber` (stack + frames + `caller`) for
  lightweight cooperative suspension. Crates: `aipo-vm`.
- **Single-pass:** `emitJump`/`patchJump` is the reference if the frontend emits jumps without
  IR. Crates: `aipo-compiler`.

---

## 2. QuickJS-ng (`quickjs-ng/quickjs`, `master`)

**What it is:** embeddable JavaScript engine. Reference for value organization, property names,
ownership and cycle collection — not a proposal to adopt JavaScript semantics.

**How it works:**
- [`quickjs.h`](https://github.com/quickjs-ng/quickjs/blob/dad13e33cee39b910de2e45ea940c08baf42dbf9/quickjs.h#L208)
  distinguishes ownership of `JSValue`/`JSValueConst`; value representations are conditioned by
  macros, including union+tag.
- [`JSShapeProperty`/`JSShape`](https://github.com/quickjs-ng/quickjs/blob/dad13e33cee39b910de2e45ea940c08baf42dbf9/quickjs.c#L1102)
  associate atoms, flags and a property table.
- [`JS_RunGC`](https://github.com/quickjs-ng/quickjs/blob/dad13e33cee39b910de2e45ea940c08baf42dbf9/quickjs.c#L7484)
  is the point to study the collector; its full policy was not audited in this reconstruction.

### Verified technical details

- **`JSValue` and width per mode:** under `JS_NAN_BOXING` (`quickjs.h:251-280`), `uint64_t`
  (8 bytes) with tag in the high 32 bits and floats via offset; without NaN-boxing, a
  pointer+tag struct (16 bytes). Negative tags for managed objects (`JS_TAG_OBJECT = -1`,
  `JS_TAG_STRING = -7`).
- **GC by refcount + cycle remover:** each object carries `ref_count` and `mark`
  (`quickjs.c:284-296`). `JS_RunGC` runs `gc_decref` (isolate zeroed counters) → `gc_scan`
  (re-sum reachable) → `gc_free_cycles` (free cycles under
  `JS_GC_PHASE_REMOVE_CYCLES`). Triggered by allocation pressure via the malloc threshold
  (`malloc_gc_threshold = 256 KiB`, `quickjs.c:1687-1696,2334`), with no separate root set.
- **Shapes and atoms:** `JSShape` (`quickjs.c:1108-1140`) shares prototype and the name/flag
  layout among instances with a global `shape_hash`. Names are interned in the `atom_hash`
  hash table (`quickjs.c:343-347`).
- **Bytecode without IR:** formats declared in `quickjs-opcode.h` (`FMT(u8)`, `FMT(const)`,
  `FMT(atom)`) consumed directly at emission.

**Closed questions:** interned names and shared layouts are mechanisms different from property
values; compact representation requires configuration and invariants. Shapes are not equivalent
to a `HashMap` added to each instance.

**Lesson → action in Aipo:**
- Keep `Value` semantics centralized and measure field-lookup cost before introducing
  shapes/slots in Aipo.
- **Shapes vs `struct_fixed_fields`:** a shared `shape` is the model for sealing structs
  (`SealStruct`) — invalid transitions become a friendly diagnostic error instead of a dynamic
  dictionary. Crates: `aipo-vm`, `aipo-compiler`.
- **Atoms:** intern names once and compare by id (`atom_hash`). Crates: `aipo-vm`.
- **Threshold GC:** if a cycle collector exists, triggering by `malloc_size` is more
  predictable than timers. Crates: `aipo-vm`.

---

## 3. Lua (`lua/lua`, `master`)

**What it is:** embeddable runtime with tagged values, slot stack and closures with upvalues.

**How it works:**
- [`lobject.h`](https://github.com/lua/lua/blob/0b29f408433e92953cc72b1d3e06c7ac8139e439/lobject.h#L49)
  defines the `TValue` payload and tag.
- [`luaF_findupval`](https://github.com/lua/lua/blob/0b29f408433e92953cc72b1d3e06c7ac8139e439/lfunc.c#L85)
  walks open upvalues by level, reusing the existing one or creating another. An ordered list
  may still have linear cost in the number of open captures traversed.
- [`luaV_execute`](https://github.com/lua/lua/blob/0b29f408433e92953cc72b1d3e06c7ac8139e439/lvm.c#L1210)
  receives the dispatch mechanism conditioned on `LUA_USE_JUMPTABLE`. The studied source is the
  manifest-pinned commit.

### Verified technical details

- **`TValue`: size and layout on 64-bit:** `union Value` (8 bytes: GC, pointer, cfunc, int, num)
  + `lu_byte tt_` in `struct TValue` (`lobject.h:49-69`), totaling 16 bytes on LP64 with
  padding. Tags in bits 0–3 + variants 4–5 + collectable flag bit 6. Copy via `memcpy`
  (`setobj`).
- **Open upvalues and ordered list:** `luaF_findupval` (`lfunc.c:87-99`) walks
  `L->openupval` ordered by stack level (`uplevel(p) >= level`). Worst case O(n) in the
  number of open upvalues. `luaF_closeupval` (`lfunc.c:197-210`) unlinks from the list, copies
  the value into the upvalue object's slot (`setobj(L, slot, uv->v.p)`) and repoints
  (`uv->v.p = slot`). `MAXUPVAL 255` (`lfunc.h:29`) fits in 8 bits.
- **Dispatch and register windows:** `vmdispatch` = `switch`, `vmcase` = `case`, `vmbreak` =
  `break` (`lvm.c:1199-1201`). Register window per frame base: `RA(i)` = `base + GETARG_A(i)`
  with `updatebase(ci)` = `base = ci->func.p + 1` (`lvm.c:1108,1122`), without copying the
  whole register bank on calls. 32-bit instruction format with opcode in the first 7 bits
  (`lopcodes.h:15-53`).

**Closed questions:** layout and capture cost depend on representation; the ordered search does
not become O(1); the dispatch mechanism must be analyzed on the real target and build.

**Lesson → action in Aipo:**
- Study slots and captures before removing the 256-register copy in RegVM. In this round,
  operands and caller restoration were fixed; the migration to base+slots windows remains a
  proposal.
- **Upvalues:** implement `MakeClosure`/`GetUpvalue`/`SetUpvalue` using a list ordered by stack
  level instead of loose copies; close an upvalue by hoisting the value into the cell. Crates:
  `aipo-vm`, `aipo-compiler`.
- **u32 format:** 32-bit instruction with limited opcodes and operands; upvalue indices fit in
  8 bits (`MAXUPVAL=255`). Crates: `aipo-compiler`, `aipo-vm`.

---

## 4. LuaJIT (`LuaJIT/LuaJIT`, `v2.1`)

**What it is:** implementation with interpreter, JIT and compact value representation. Reference
for encoding and frames, not a new dependency for Aipo.

**How it works:**
- [`lj_obj.h`](https://github.com/LuaJIT/LuaJIT/blob/c6ffc141a8762b41703f9287d63d93622a13dd8f/src/lj_obj.h#L151)
  defines `BCIns` as `uint32_t`, plus `TValue` and representations conditioned on
  `LJ_GC64`/`LJ_FR2`.
- [`lj_bc.h`](https://github.com/LuaJIT/LuaJIT/blob/c6ffc141a8762b41703f9287d63d93622a13dd8f/src/lj_bc.h#L25)
  fixes the ABC/AD/AJ ranges and roles.
- [`func_finduv`](https://github.com/LuaJIT/LuaJIT/blob/c6ffc141a8762b41703f9287d63d93622a13dd8f/src/lj_func.c#L37)
  reuses cells in an ordered upvalue list.

### Verified technical details

- **`TValue`: 8 bytes with NaN-tagging:** `union TValue` (`lj_obj.h:174-249`) overlays `u64`,
  `lua_Number` and `GCRef`+tag. In 32-bit/non-GC64, tag in the MSW over a quiet-NaN double;
  with GC64, 4-bit tag inside the quiet NaN and 47-bit payload. Price: strict coupling to the
  IEEE-754 standard and two build-dependent layouts.
- **Upvalues and frames:** `func_finduv` (`lj_func.c:36-69`) and `lj_func_closeuv`
  (`lj_func.c:82-100`) close cells `>= level`. Frame on the stack managed by base + slots,
  without a flat copy of the whole register bank.
- **32-bit bytecode:** formats `ABC`, `AD`, `AJ` (`lj_bc.h:14-27`). Limits
  `BCMAX_A/B/C = 0xff`, `BCMAX_D = 0xffff`, `BCBIAS_J = 0x8000`.
- **Dispatch in assembly (DynASM):** the main interpreter is generated from `.dasc` files
  (`vm_x64.dasc`, `vm_arm64.dasc`), using replicated or common dispatch with continuation via
  pointers (`vm_arm64.dasc:241-245,581-586`). High-performance threaded code, but dependent on
  specific hardware architecture.

**Closed questions:** word width does not by itself define the range of every operand; formats and
roles must be described per opcode. `GCRef` must not be advertised as always 32 bits, since
distinct configurations exist.

**Lesson → action in Aipo:**
- Document operand roles, check constants and offsets and use iteration opcodes with full C.
  NaN boxing and assembly dispatch remain future experiments; there is no current justification
  to replace Aipo's safe, portable `Value` model.
- **Dispatch ladder:** keep canonical `switch` in RegVm, evaluate computed `goto` (Wren model)
  behind a conditional feature, and defer DynASM generators until a production profile demands
  them. Crates: `aipo-vm`.
- **Upvalues:** reuse the ordered-search and explicit-close invariants of
  `func_finduv`/`lj_func_closeuv`. Crates: `aipo-vm`.

---

## 5. Common limits and continuity

The inspection covers the cited regions of the four Tier A runtimes. A full audit of GC,
compilation, FFI, reentrancy and portability requires dedicated rounds. References serve to
formulate hypotheses and structural invariants, not to announce performance, memory or maturity
gains for Aipo in advance.

See [lessons for Aipo](lessons-for-aipo.md) and the
[reimplementation guide](runtime-hardening-guide.md) for the mapping between the studied sources
and the changes actually delivered in the repository.
