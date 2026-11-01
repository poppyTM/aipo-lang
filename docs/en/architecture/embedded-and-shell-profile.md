# Native, shell, web and embedded profiles

**Status:** implementation and limits · **Update:** P07-G02, 2026-10-09.
**Sources:** `Cargo.toml`, `crates/aipo-cli/Cargo.toml`, `aipo-bytecode` and `aipo-vm`.

This document replaces the previous specification that mixed MCU goals, unmeasured memory/startup numbers and hypothetical APIs with the delivered runtime. See the [usage/reimplementation guide](../development/runtime-and-tooling-guide.md) and the [evidence](../evidence/P07-G02/README.md).

## Effective build profiles

| Selection in `aipo-cli` | Optional components | Wasmtime | Rust standard library |
|---|---|---|---|
| Default features | Formatter, JS, Wasm emitter, regex, extra Unicode and runner | Included | `std` |
| `--no-default-features --features profile-shell` | Formatter, regex and extra Unicode | Absent | `std` |
| `--no-default-features --features profile-web` | JS and Wasm emitter | Absent; add `wasmtime-runner` for the runner | `std` |
| `--no-default-features --features profile-embedded` | No optional emitter/JIT | Absent | `std` |
| Same embedded selection with `--profile nano` | LTO, size optimization, strip and Cargo panic abort | Absent | `std` |

Aliases do not remove defaults on their own. No per-profile crates were created. `embedded` means a smaller native configuration, not a `no_std` port. The minimal CLI still registers the stdlib's OS services; hosts may use the libraries and register only their own services. MSRV is Rust 1.96, aligned with the Wasmtime/Cranelift graph of the lockfile.

## One frontend, explicit execution plans

The stack VM is the semantic reference: values, contracts, methods, captures, scheduler, invariants and failures. The RegEmitter compiles its strict subset; `ExecutionPlan` tries native Reg when the IR is representable and verified, or uses canonical bytecode with an explicit reason. Higher-order methods and failure handlers still force the canonical plan; on 2026-10-27, 20/36 conformance programs ran natively in Reg.

This lets `aipo run --engine=reg` use the canonical surface without pretending the Reg interpreter has every capability. `aipo plan` shows the decision without running. The `analyze_to_reg_module` API stays strict. Host contexts that require the VM also select the canonical plan.

JS and Wasm remain their own emitters. Wasm does not represent all of `Value`'s dynamics; `is` with an unknown semantic type is rejected with a diagnostic. No WAMR/wasmi runtime is embedded in this delivery.

## Representation, memory and registers

`Value` remains a Rust enum with variants managed by Rc/RefCell. Its size depends on the target; `layout_probe` must measure the concrete build. There is no current promise of 16 bytes, NaN boxing or a fixed minimum RAM.

`List`, `Set` and `Bytes` use `Collection<T>` with structural revision; Dict uses its own revision. Guards check the revision independently of length. Existing elements may be replaced without changing structure. A guest snapshot restores allocations in place to preserve aliases.

The RegVM keeps 256 registers, a depth limit and a reserved arena. The current managed values still live in Rc; the arena is not the allocator for all values and provides no global O(1) deallocation. Compiled calls move/restore only the caller/callee's live window. Raw entry preserves all host registers.

The real encoding is defined in `crates/aipo-bytecode/src/instruction.rs`:

| Field | Width | Role |
|---|---|---|
| Opcode | 7 bits | Up to 128 codes |
| A | 8 bits | Register 0–255 |
| B | 9 bits | Operand per opcode; registers still limited to 255 |
| C | 8 bits | Operand per opcode |
| Bx | 17 bits | Unsigned pool/index per opcode |
| sBx | 17 bits with 65536 bias | Immediate/relative jump |

The verifier distinguishes register from index/immediate. It is not correct to interpret every B as a register or to advertise C as 9 bits.

## Session, shell and reload

`aipo-sh` uses a persistent Session, multiline input, JSONL history and completion/load/reload/reset commands. It compiles and runs only new statements. The interactive Reg profile explicitly uses the canonical session.

The linker preserves published indices and relocates new units. Reload removes declarations from the file, initializes the candidate, preserves compatible globals, offers migration and validates retained layouts. Snapshot restores definitions/heap if the candidate fails. History and watcher have size/depth limits.

Rollback does not undo host files, network, processes or external resources. The shell offers the existing `sh` services; it does not implement full POSIX job control, universal descriptor redirection or process pipelines as a consequence of `|>`. The operator still follows the canon.

## Embedding and limits

The C ABI lives in `aipo-c-abi` and exposes the names in `include/aipo.h`, not the hypothetical APIs of the previous document. There is synchronous load/call, cooperative begin/pump/abort, handles, capabilities and budgets. Raw Reg has its own verification, budget and diagnostics. Strings/Bytes cross C as snapshots with explicit release.

Rust uses `Vm`, `RegVm`, `Session` and `ExecutionPlan`. `Value` is `!Send`; each runtime belongs to one thread. C reentry during a callback is refused. Pump bounds how much guest runs, not the duration of a native callback.

Wasm allows fuel, a linear-memory limit and a buffered output limit, with reusable engine/cache and a separate Store per execution. There is no global limit over the whole process JIT or all Rust allocators.

## Measurement and future ports

[P07-G02](../evidence/P07-G02/README.md) contains a Reg call benchmark against the pinned baseline, raw data and reproduction. It does not measure cold start, Flash, RSS of every profile or external runtimes. Numbers from another layout, Lua version or machine must not be reused as current Aipo numbers.

A port to an MCU requires separating OS-dependent services, designing a limited allocator/heap, handling cycles, defining a HAL, validating layout/alignment, allowing execution from Flash and measuring RAM/Flash/startup on a concrete target. `nano` with `panic=abort` provides no panic recovery by unwinding.

The cross-platform build matrix and artifact workflow are implemented; their success must be observed before certifying platforms. The tests of this round were explicitly waived by the user. The implementation does not declare the Prumo gates complete.
