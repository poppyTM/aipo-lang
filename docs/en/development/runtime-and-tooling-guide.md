# Runtime, embedding and tooling: usage and reimplementation

**Round:** P07-G02 · **Date:** 2026-10-09 · **Base:** `f0a0d70d176be031cd4b600fde1df6179429fb9a`.

This guide describes the implementation of the five fronts requested after the P07-G01 review. The
implementation is integrated into `main`; the Prumo goal remains `DRAFT`. The 2026-10-27
reconciliation ran the full workspace (1,130 tests), conformance VM/Reg, Poppy, eGUI, game host,
portal validators and the VitePress build; fuzz, Miri and ASan remain pending. The examples
below are usage instructions, not a new universal certification.

Also read the [evidence of this round](../evidence/P07-G02/README.md), the
[source study](../studies/runtime-and-tooling-follow-up.md), the
[real profiles](../architecture/embedded-and-shell-profile.md) and the
[previous guide](runtime-hardening-guide.md). The documentation is Markdown inside the
repository, available on GitHub.

## 1. Delivery of the five fronts

| Front | Implementation | Relevant limit |
|---|---|---|
| 1. Correction and security | Complete Reg verifier for existing operands, defined initialization and scopes; structural revisions of collections; conservative `is` in Wasm | Does not constitute fuzzing/Miri certification or conformance execution |
| 2. Coverage and embedding | Explicit `ExecutionPlan`; canonical linker; Reg/C budgets; cooperative C execution; Wasm limits and cache | Closures, async, hooks and interfaces still use canonical services when native Reg does not support them |
| 3. Session, shell and profiles | Persistent REPL, multiline input, history, per-command completion, reload with migration and rollback; feature aliases | Shell uses the OS; no complete POSIX job control; `embedded` uses `std` |
| 4. Performance | Live register windows, shared functions, per-module prepared constants; reproducible paired benchmark | Gain measured only on the workloads described in the evidence |
| 5. Tooling and distribution | `new`, `watch`, `profile`, `debug`, `plan`, stdio LSP, offline vendor, packager, installer and workflows | Basic LSP; workflows do not certify platforms before running; no public release published |

## 2. Installation and profiles

MSRV moves from **Rust 1.85 to 1.96**. The existing lockfile includes Wasmtime 49.0.4 and
Cranelift 0.136.4, which declare Rust 1.96. The previous declaration did not represent that
dependency graph. The change also required style adjustments flagged by Clippy, mainly conditional
simplification; the lints were not weakened.

```sh
# Minimum workspace toolchain
rustup toolchain install 1.96.0 --profile minimal

# Full CLI: formatter, JS, Wasm emitter, Wasmtime runner, regex and extra Unicode
cargo +1.96.0 build --locked -p aipo-cli --release

# Shell without JIT, with formatter, regex and extra Unicode
cargo +1.96.0 build --locked -p aipo-cli --release \
    --no-default-features --features profile-shell

# JS/Wasm emitters, without Wasmtime
cargo +1.96.0 build --locked -p aipo-cli --release \
    --no-default-features --features profile-web

# Minimal native CLI, still with std
cargo +1.96.0 build --locked -p aipo-cli --profile nano \
    --no-default-features --features profile-embedded
```

Use `--no-default-features` to remove the full profile. Adding `profile-shell` to the defaults
does not remove the JIT. `nano` is a Cargo optimization profile for the binary; it is not a
backend or a microcontroller port. Fundamental normalizations still follow the canon; the
`unicode` feature enables additional operations.

There is no promise of `Value` size, minimum RAM or millisecond startup. Use
`crates/aipo-vm/examples/layout_probe.rs` and measurements on the real target to obtain those
numbers.

## 3. Execution choice: VM and Reg

```sh
aipo run src/main.aipo
aipo run src/main.aipo --engine=reg
aipo plan src/main.aipo
```

`aipo plan` compiles without running and writes JSON with `engine` and `reason`. The plan tries
native Reg when the emitter and verifier accept the IR; it uses the canonical VM when the emitter
rejects the construct or when the program depends on higher-order methods or failure handlers,
which still lack native parity. The absence of a Reg capability produces an explicit reason; no
instruction is discarded to simulate support. On 2026-10-27, 20/36 conformance programs selected
native Reg and all 36 produced the expected output with `--engine=reg`.

`analyze_to_reg_module` remains the strict API: it returns an error when native support is
missing. `analyze_to_execution_plan` returns the explicitly typed alternative. Hosts such as
`--host=headless-test` use the canonical context even in the Reg profile, because their callbacks
depend on that context.

This delivers coverage to the **execution profile**, preserving language semantics. It does not
mean async, closures/upvalues, operational interfaces, invariants and the journal were
reimplemented in the native Reg interpreter. The canonical VM remains the reference for those
capabilities.

### Reg verification

`crates/aipo-bytecode/src/reg_verifier.rs` performs two analyses:

1. Validates every operand, including unreachable instructions: register indices, argument
   windows, pools, required String constants, function references, jumps and iteration
   projections. It also rejects constants outside the language's safe numbers.
2. Walks the CFG with a per-register initialization state and handler/iteration scopes. Parameters
   start initialized; joins use intersection. A read register must be initialized on all paths.
   Exceptional entries are conservative and recover the scope installed by the handler.
   Scope underflow and reachable fallthrough without return are errors.

`RegEmitter::compile_module` checks the result. `RegVm::run_module`, `run_function`,
`invoke_function` and raw entry check their respective boundaries before guest effects. In raw
entry, host registers start initialized, including those holding `none`. A module proof is
reused only during its execution; an RAII guard restores that state also during unwind.

To add an opcode: update the format, emitter, verifier operands, CFG effects, execution and
conformance cases. Do not confuse the stack bytecode's byte position with the Reg instruction
index. Each Reg instruction occupies a `u32`; the format uses a 7-bit opcode, 8-bit A, 9-bit B
and 8-bit C, or 17-bit Bx/sBx as the instruction requires.

## 4. Collections and mutation during iteration

Previously, comparing only length allowed a removal followed by an insertion to slip through a
callback boundary. Now `Collection<T>` tracks structural revision. `DictMap` has an equivalent
revision.

| Operation | Revision |
|---|---|
| Insert/remove element or key, clear collection, reorganize structure | Increments |
| Replace value at an existing position or value of an already-present key | Preserves |
| Compare two collections with the same elements and order | Ignores revision |

Guards store identity and revision. The canonical VM preserves state per task and removes guards
on returns and unwinds; higher-order methods also check the return of native callbacks. Reg
preserves guard depth in handlers. Existing elements can still be replaced per canon.

**Rust API migration:** `List`, `Set` and `Bytes` went from `Rc<RefCell<Vec<_>>>` to
`Rc<RefCell<Collection<_>>>`. Use the constructors or convert a `Vec` with `.into()`:

```rust
use aipo_vm::Value;
let items = Value::list(vec![Value::Int(1), Value::Int(2)]);
let bytes = Value::bytes(vec![1, 2, 3]);
let text = Value::string("Aipo");
```

`Value::set` receives already-deduplicated elements. Mutable access exposes elements of a slice
without allowing resize through `DerefMut<Vec<_>>`. Host extensions must use `Collection`'s
structural methods. The public Reg function table uses `Vec<Rc<RegCompiledFunction>>`; adapt
clients that built it manually.

## 5. Persistent session and REPL

```sh
aipo-sh
```

```text
var count = 0
fn next() {
    count += 1
    return count
}
next()
next()
```

The session compiles the new unit, keeps a declaration catalog, relocates its bytecode and runs
only the new entry. Old bodies remain available for `Function`/`Closure` values that still
reference them. Old declarations may be compiled into the current catalog, but old statements
are not repeated; the REPL does not replay the whole history to rebuild state.

`run_persistent_at` preserves Tasks, results, queues, waiters, joins, groups, IDs and virtual
time across units. Thus a task created in one unit can be awaited in another. `run_at` remains
the independent entry point that reinitializes the scheduler. Persistent entry requires
append-only linked bytecode, keeping old offsets valid.

The definitions snapshot includes the scheduler. The heap snapshot also follows task results,
host-task arguments, journals and upvalue cells retained by tasks, preserving aliases on
rollback. Results/tasks and old code can still grow in long sessions; Rc cycle/retention
collection remains future work.

| REPL command | Use |
|---|---|
| `:help` | Show commands |
| `:history` | Show previous units |
| `:complete PREFIX` | List globals with that prefix |
| `:load PATH` | Evaluate a unit |
| `:reload PATH` | Replace declarations from that file and preserve compatible state |
| `:reset` | Clear the session |
| `:quit` | Exit |

The lexer controls delimiter balancing, ignoring strings and comments, for multiline input. On
redirected input there are no prompts; failures result in a non-zero exit code. `aipo-sh
--engine=reg` uses the canonical persistent session and identifies that choice in the banner.

History uses JSONL, up to 1,000 units and an 8 MiB read limit. Larger files are ignored. The
default path uses `XDG_STATE_HOME`, or `HOME/.local/state`, followed by `aipo/history.jsonl`.
`AIPO_HISTORY_FILE` changes the path; an empty value disables persistence. Writing uses a
temporary file, sync and rename; on Unix the file is created with mode `0600`. The interface
still lacks a line editor with arrow navigation and Tab completion.

### Linker

`append_unit`, in `aipo-bytecode/src/linker.rs`, verifies the new module, works on a candidate
copy, deduplicates pools and relocates name, constant, function, closure and contract indices.
It updates function entries and spans. Internal relative jumps do not change. It checks format
limits and the linked module before replacing the original image. `function_by_name` searches
the most recent definition.

To reimplement, preserve published code indices: compacting/removing old units invalidates
still-live references. The linked image grows during the session; collecting old units requires
a future reachability/identity policy. Current deduplication walks pools linearly and could be
improved with interning indices after measurement.

## 6. Hot reload and migration

```sh
aipo watch src/main.aipo
```

`watch` follows `.aipo`, `.toml`, `.lock` and `.ahs` files in the package tree containing the
entry, or in the entry's directory when there is no manifest. It ignores symlinks, `.git`,
`target` and `node_modules`; depth is limited to 64 and count to 10,000 files. It uses content
hashes and 250 ms polling. Path dependencies outside that tree require another watcher or an
explicit reload API call.

`Session::reload_with` executes these steps:

1. Captures definitions, scheduler and reachable guest heap before removing declarations
   from the file.
2. Compiles, links and initializes the candidate. Removing a function/type stops exposing the
   old declaration.
3. Keeps existing non-callable globals that are still declared.
4. Calls the host migration function and verifies layouts of retained instances, including
   names, order and `fixed` fields.
5. Updates caches and confirms the generation, or restores code, catalog, globals, scheduler
   and heap.

```rust
use aipo_cli::Session;
use aipo_vm::Value;
use std::path::Path;

let mut session = Session::new();
session.set_instruction_budget(Some(100_000));
let status = session.reload_with(
    "var count = 0\n", Path::new("counter.aipo"),
    &mut std::io::stdout(), &mut std::io::stderr(),
    |globals| {
        globals.entry("count".into()).or_insert(Value::Int(0));
        Ok(())
    },
);
assert_eq!(status, 0);
```

`HeapSnapshot` walks the graph once per allocation: collections, Dict, Bytes, structs, upvalue
cells, method receivers, Failure payloads and Sequence data/caches. Rollback restores **the Rc
allocations themselves**, preserving aliases held by the host. Cloning only the globals map was
not enough.

An incompatible migration must replace or adapt reachable instances, including their `fixed`
fields. A removed type with a still-live instance is rejected. Initializers may mutate old
state; that is reverted if the candidate fails. On a successful reload those mutations
participate in the confirmed state.

**Transaction limit:** files, processes, network, native callbacks and opaque host resources are
not undone by `HeapSnapshot`. Initializers should avoid irreversible external effects, or the
host must provide its own transaction. There is no Rc cycle collection; snapshot is not a
garbage collector.

## 7. Embedding and budgets

A runtime belongs to one thread. `Value` uses Rc/RefCell and is not `Send`; do not share a
mutable instance across threads. The host registers capabilities explicitly. The C boundary
refuses reentry during callbacks.

### Rust

- `Session` offers persistent evaluation, per-unit limit, global reads, completion and
  migration.
- `Vm` offers `run_at`, `start_at`, `debug_step`, `take_completion`, metrics and budget.
- `RegVm::set_instruction_budget(Some(n))` configures a cumulative budget and resets its
  counter; `None` disables counting. `reset_instruction_count` resets explicitly.
- In the canonical VM, budgeted executions preserve consumption across entries; `reset_instruction_count`
  resets. `Session` resets before each evaluation, giving a per-unit budget.

The canonical C counter tracks execution when budget or metrics are enabled. Execution without
budget may reset the counter and does not count opcodes with metrics disabled; it is not a
universal counter for the whole runtime lifetime.

An instruction budget measures guest work, not total memory, native callback duration or
wall-clock. The host must limit external operations separately.

### C: synchronous and cooperative execution

The header is `crates/aipo-c-abi/include/aipo.h`. The canonical ABI keeps
`aipo_runtime_load_module` and `aipo_runtime_call`; the runtime links units and uses relocated
offsets to avoid calling the wrong module's code. A recoverable load error restores definitions
and heap. Incompatible retained layouts are rejected.

Additional APIs split execution:

```c
bool completed = false;
aipo_value_t result;
aipo_status_t status = aipo_runtime_begin(rt, "demo", "main", NULL, 0);
while (status == AIPO_OK && !completed) {
    status = aipo_runtime_pump(rt, 1000, &completed, &result);
    /* Return control to the host event loop between pumps. */
}
if (status == AIPO_OK && completed) {
    aipo_value_release(rt, result);
}
```

`begin` prepares the function as the main cooperative computation, requires compatible arguments
and no captures at the selected entry. `pump` executes at most the requested quanta; callbacks
may still block. The value is only valid when `completed` is true. Load/call/begin refuse a busy
runtime. `aipo_runtime_abort` cancels that computation and keeps mutations and I/O already
completed; it does not roll back. Failures and cancellation keep their canonical distinction.
Unwinding panics are contained at the boundary; builds with `panic=abort` cannot be recovered by
`catch_unwind`.

For raw Reg, the header now covers lifecycle, int set/get, run, budget, counter/reset and
`aipo_reg_vm_copy_last_error`. The latter returns the required size including NUL and allows
querying with a null buffer or zero capacity. A smaller buffer receives a truncated copy with
NUL. An Int outside the safe range or a non-Int return when `out_int` is requested produces an
error; there is no silent coercion to zero.

Strings/Bytes returned by the canonical ABI are runtime snapshots. Release them with
`aipo_value_release`. Do not `free` them, do not retain them after destroying the runtime, and
do not pass expired pointers, short buffers or misaligned data. Null and size checks do not
prove arbitrary C-pointer validity.

## 8. Wasm: semantics and resources

The `is` test uses a known semantic type, not the Wasm storage type. `i32` may store Bool or a
pointer; `i64` does not prove Int. None matches in `is T?`; other values use
fundamental/nominal comparison. Expressions, parameters without proof, nullable, ambiguous calls
or element bindings with unknown type produce a diagnostic guiding to use the VM.

When `compile_hir` rejects a construct outside the subset, the CLI emits
`AIPO_COMPILE_WASM_UNSUPPORTED` with the error span and the note `use --engine=vm`. This code
is a compile-time code, not a runtime fault.

A legacy early branch that used physical types and made the later semantic check unreachable
was removed. There is now a single path: prove the value, resolve a non-shadowed static
destination and evaluate the left side once, preserving its effects. The function catalog tracks
global bindings and declared types to avoid confusing names with the prelude.

Aliases such as `let T = Int`, parameters/globals/functions that shadow `Int`, unknown
destinations and dynamic expressions on the right side are rejected in Wasm with guidance to use
`--engine=vm`. `Function` was not invented as a TypeTag: the diagnostic name of a callable
value is not a fundamental type available in the prelude. `String`/`task.spawn` call proofs
also require non-shadowed names. Numeric operations with unknown operands and string
concatenation receive no false type facts.

To expand support, propagate type facts through HIR and its joins or implement real dynamic tags.
Do not revert to the fallback that assumes Int for any expression. Rejecting an unproven case is
an explicit limitation, not an invented `true` result.

With `wasmtime`, `WasmExecutionOptions` offers `fuel`, `memory_bytes` per linear memory and
`output_bytes` for buffered output. Excess output is rejected before extending the buffer;
excessive memory grow traps. Execution delivers output with writer checking. These limits do not
cover external host resources or all memory of the JIT process.

`WasmRunner` reuses Engine and compiled modules in an LRU cache bounded by count. Each execution
creates an independent Store for memory, globals, fuel and output. `set_cache_capacity(0)`
disables the cache; `execute_wasm_with_options` remains the convenience entry point. The
optional runner is Wasmtime in this delivery; there was no switch to WAMR/wasmi and no
certification on MCU.

## 9. Development tools

| Command | What it does |
|---|---|
| `aipo new demo` | Creates a manifest, `src/main.aipo` and README; does not overwrite an existing directory |
| `aipo profile src/main.aipo --budget 100000 --json` | Runs on the VM and reports time, instructions, calls, native calls and field cache |
| `aipo debug src/main.aipo` | Terminal debugger: `step`, `continue`, `break BYTE_OFFSET`, `globals`, `quit` |
| `aipo plan src/main.aipo` | Shows engine and plan reason, without executing |
| `aipo watch src/main.aipo` | Recompiles/reloads candidate while keeping the last valid generation |
| `aipo lsp` | LSP server on stdin/stdout |

The debugger uses bytecode offsets and one scheduler quantum per step, and may switch tasks. It
shows IP, span and stack. An execution failure or uncaught Failure yields exit code 1. It does
not yet implement DAP, lexical inspection of all locals, line stepping or expression evaluation
in a frame.

The LSP offers `initialize`, shutdown/exit, full document sync, UTF-16 diagnostic positions,
completion of words/prelude/declarations and formatting when the feature exists. It analyzes the
open text, not only the saved file, and respects document versions. Imports use the project
resolver. Diagnostics from other files are contextualized by path/line, without pretending
offsets in the open document.

Transport verifies Content-Length, limits headers to 8 KiB and messages to 8 MiB. Invalid JSON
and invalid requests have distinct responses. Unsupported methods return an explicit error. Hover,
rename, go-to-definition, semantic tokens, incremental changes and cancellation of in-progress
work remain future evolution. Configure the editor to start `aipo lsp`; never send logs through
the protocol's stdout.

## 10. Offline vendor and distribution

```sh
aipo package lock caminho/do/pacote
aipo package vendor caminho/do/pacote --out vendor-demo
# GitHub dependencies already verified in the cache:
aipo package vendor caminho/do/pacote --cache caminho/do/cache --out vendor-demo
aipo run vendor-demo/root/src/main.aipo
```

Vendor requires an existing lock and a verifiable graph. It does not fetch the network. It copies
root/dependencies into a new directory, refuses symlinks, rewrites dependencies to local paths
and produces a local lock. `vendor-manifest.json` keeps the original lock and a SHA-256
inventory. It ignores hidden files, `target` and `node_modules`; projects that depend on those
assets must package them through another explicit process. The stage becomes the destination by
rename; an error removes the stage created by the operation.

The existing resolver continues to use exact versions and pinned sources. Vendor adds no
registry, package publication or SemVer range solving.

```sh
# Run in a clean checkout, after the commit
python3 scripts/release/package.py --profile shell

# Verify the SHA-256 and install the local file into an explicit prefix
python3 scripts/release/install.py target/dist/ARQUIVO.tar.gz --prefix /caminho/aipo
```

The packager builds the CLI, includes the license and `build.json` with commit, target, compiler
and profile; `full` includes the C header and libraries. It produces `.tar.gz` and a checksum.
Timestamp and member order are controlled by `SOURCE_DATE_EPOCH`; this makes file assembly
deterministic for the same payloads, without promising bit-identical binaries across machines.

The installer verifies SHA-256 before extracting, accepts only regular files in one root, rejects
traversal, duplicate entries, symlinks and symlinked destinations. It prepares files, preserves
backups and uses per-file substitution with rollback on failure. `--force` allows replacing
existing files. It does not download or run scripts from the package. This is not a transaction
against an OS crash across multiple substitutions.

`profiles.yml` defines builds on Linux/macOS/Windows, minimum and stable Rust, for shell/web/
embedded. `package-artifacts.yml` is manual, generates artifacts per OS/profile and does not
publish a release. The execution of these workflows was not observed in this delivery; the local
result does not certify the other platforms.

## 11. Performance: what changed and how to reproduce

The Reg bottleneck was copying all 256 registers and repeatedly cloning function bodies/pools. The
activation now moves and restores only the window needed by caller and callee. Bodies use Rc and
guest constants are prepared when the module loads. Internal calls reuse the module proof;
external boundaries are still verified.

Raw entry keeps the full window to preserve host registers. The depth limit remains to protect
stack/RAM. The change does not implement a compact arena, NaN boxing or threaded dispatch.

Use [the raw data and procedure](../evidence/P07-G02/README.md). The `reg_calls_bench` example
runs 20,000 calls with a nine-instruction caller and one register in the callee; Int and String
cases are actually used. The driver alternates baseline/candidate, pins the CPU when requested,
removes warmup and records at least two batches, compiler and hashes. Do not extrapolate the
result to the whole language, cold start, memory or struct/async workloads.

## 12. What can still improve and suggested order

| Priority | Work | Why and completion criterion |
|---|---|---|
| P0, before certifying | Run conformance, regressions, fuzz, Miri/ASan and the cross-platform matrix when authorized | Compilation does not prove effects, unwind, aliases, callbacks or protocol; record real results per scenario |
| P1 | Native Reg: captures, scheduler and hooks via shared contracts/services | Reduce fallback without duplicating semantics; each capability requires a differential against the VM and a closed error while incomplete |
| P1 | Rc cycles and retention of old units | Long sessions can still accumulate heap/code; define identity, scope and lifecycle before compacting |
| P1 | Hot reload of external resources and structurally typed ergonomic migrations | Guest snapshot does not undo I/O; coordinate host transaction hooks and imports outside the tree |
| P2 | Complete LSP/DAP and line editor | Navigation, scopes, locations, locals and real cancellation make the tools adequate for large IDEs |
| P2 | Total heap/compilation limits and callback deadline | Opcode budget is not a memory or external-CPU sandbox; instrument the allocator/host per profile |
| P2 | Registry, publication and SemVer resolution | The current package is exact/pinned/offline; ranges and public distribution need their own policy |
| P3 | `no_std`, real allocator/arena, HAL and XIP | Current embedded is native with std; measure RAM/Flash and adapt services on a concrete target before promising MCU |
| P3 | Additional optimizations and alternative Wasm backend | Require idle baseline, functional criteria and evidence; do not choose an engine by name or by another project's benchmark |

## 13. Reimplementation and review

Start from observable boundaries: result/diagnostic, order of effects, identity and lifespan of
values, budget and failure behavior. Reuse the `Value` semantics and canonical services.
Implement in order: verifier/collections → linker/snapshot → Session/reload → embedding →
tooling → distribution.

Check the cases added in `reg_verifier.rs`, `guest_transactions.rs`, `persistent_session.rs`,
fixtures 33–36 and the Wasm proofs. The 2026-10-27 reconciliation ran the VM/Reg corpus, Poppy/
eGUI tests, game host and full workspace; fuzz, Miri and ASan remain pending. Do not rewrite
snapshots to hide divergence. The goal must not move to `DONE` without real gates and approvals.
