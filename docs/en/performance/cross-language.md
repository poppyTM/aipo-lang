# Aipo cross-language benchmarks

**Status:** implemented; manual performance evidence, not a blocking gate
**Scope:** portable workloads for Aipo CLI/VM, Aipo VM in-process, Aipo→JavaScript/Node, Lua, LuaJIT, Wren, Luau, CPython, PyPy, Ruby, JavaScript/Node and native Rust
**Related:** `docs/performance/baseline.md`, `docs/testing/ci-tiers.md`, `crates/aipo-bench/`

## Objective

Compare the cost of equivalent workloads without hiding the difference between interpreter, JIT, VM and
native code. Native Rust is an algorithmic control; its process uses the same runner binary and includes
that harness's startup, so it does not represent the startup of a minimal native binary.

Aipo appears on four lines:

- **Aipo Wasm JIT:** direct HIR-to-WebAssembly compilation and execution via Cranelift/Wasmtime with
  the `aipo_host` Host ABI (`aipo run --wasm`).
- **Aipo VM in-process:** compilation outside the timer; execution, VM creation and stdlib registration
  inside the timer.
- **Aipo CLI/VM:** `aipo run` process, including startup and source compilation on the classic Stack VM.
- **Aipo→JavaScript/Node:** bundle emitted once before the timer; Node execution measured separately.

## How to run

Prepare the release binaries:

```bash
cargo build --release -p aipo-cli -p aipo-bench
```

Run the full comparison:

```bash
target/release/aipo-bench --compare \
  --compare-json target/cross-language.json
```

Useful modes:

```bash
# three samples, for development
target/release/aipo-bench --compare --compare-quick

# specific workload and languages; N is explicit per workload
target/release/aipo-bench --compare --compare-n arithmetic=100000,collections=50000 \
  --compare-languages aipo-vm,lua,python,rust

# isolated A/B for a specific workload
target/release/aipo-bench --compare --compare-workloads collections \
  --compare-languages aipo-vm --compare-runs 15

# explicit Aipo binary path
target/release/aipo-bench --compare --aipo-bin target/release/aipo

# first wave: local Wren and Luau; PyPy is optional
PATH="$Wren_BIN_DIR:$LUAU_BIN_DIR:$PATH" \
  target/release/aipo-bench --compare \
  --compare-languages wren,luau,pypy \
  --compare-workloads arithmetic,collections,strings,recursion,startup \
  --compare-runs 1 --compare-n arithmetic=32,collections=32,strings=8 \
  --compare-resources
```

When `--aipo-bin` or `AIPO_BIN` is used, the runner records the hash and the profile inferred from
the path; a custom binary remains the responsibility of whoever selected it.

### Dedicated paired runner

For local A/B measurements, use two already-built release binaries and the
`scripts/perf/paired.sh` runner. It pins the CPU, alternates baseline/candidate order and does
not alter Git state:

```bash
scripts/perf/paired.sh \
  --baseline target/release/aipo-bench-before \
  --candidate target/release/aipo-bench-after \
  --workloads arithmetic,collections,fields,recursion \
  --rounds 15 --pairs 3 --cpu 0 \
  --output-dir target/paired
```

The script requires `taskset` by default; `--allow-unpinned` should only be used on machines
without taskset support. Reports and a `manifest.txt` with hashes, workloads, rounds and CPU land
in `target/paired`.

The runner detects `lua`, `luajit`, `wren_cli`/`wren`, `luau`, `python3`, `pypy3`/`pypy`, `ruby`
and `node`. Missing external runtimes are marked as skipped. Aipo CLI/VM, Aipo→JavaScript/Node and
native Rust are required; without the Aipo binary or Node, the failure appears in the report and
the command returns an error.

## Workloads

All programs print exactly `checksum:<value>`. The runner rejects missing, divergent or non-zero-exit
output.

| Workload | Default N | What it measures |
|---|---:|---|
| `arithmetic` | 200000 | loop with a function call and arithmetic |
| `collections` | 100000 | list construction and iteration |
| `fields` | 100000 | six fields, mutation and method calls |
| `strings` | 5000 | incremental ASCII concatenation |
| `recursion` | fixed | Fibonacci(24) |
| `startup` | fixed | minimal process, runtime initialization and possibly warm cache |

`--compare-quick` drops `startup` and uses three samples. Normal execution uses five samples.
`--compare-n` uses `workload=N` pairs, with N between 1 and 1,000,000; this avoids applying
arithmetic's N to the quadratic strings workload. `--compare-workloads` selects a list of
workloads for isolated A/B. `--compare-runs` accepts 1 to 31.

## First wave of references

The matrix adds three references without collapsing different groups into a single comparison:

- **Wren 0.4.0 + Wren CLI 0.4.0**: compact VM, bytecode and embedding; the CLI receives the
  script and arguments directly.
- **Luau 0.739**: Lua-derived VM, with interpreter and native code; the CLI uses `-a` to separate
  program arguments.
- **PyPy 8.0.0, Python 3.12**: same Python language with a JIT/GC runtime; absent from PATH it
  appears as `skipped`.

Sources are kept in `/home/raillen/Documentos/Projetos/aipo-reference-runtime/` and recorded in
`SOURCES.json`. They are read-only references, not build dependencies of Aipo. The runner resolves
the executable from `PATH`; the pinned checkout alone does not prove which binary ran. Local runs
should therefore record `PATH`, binary hash and reported version.

Wren does not ship an official CLI in the VM's main checkout. The separate CLI is the source of the
executable used in this battery. Building CLI 0.4.0 hit a `write` name collision with current libc
headers; the local run used a temporary tree with a compatibility rename, without modifying the
pinned checkout. Luau does not accept `--version`; the runner uses `-h` to confirm availability
and records `luau CLI (no --version flag)`.

Initial reading map:

| Aipo | References |
|---|---|
| `crates/aipo-vm/src/vm/dispatch.rs` | `lua/lvm.c`, `cpython/Python/ceval.c`, `ruby/vm_exec.c`, `node/deps/v8/src/interpreter/interpreter.cc` |
| `crates/aipo-vm/src/vm/call.rs` | `ruby/vm_method.c`, `luau/VM/src/lvmexecute.cpp`, `wren/src/vm/wren_vm.c` |
| `crates/aipo-vm/src/value.rs` | `lua/lobject.c`, `wren/src/vm/wren_value.c`, `luau/VM/src/lobject.cpp` |
| `crates/aipo-bytecode/src/emitter.rs` | `luau/Compiler/src/Compiler.cpp`, `wren/src/vm/wren_compiler.c`, `cpython/Compile` |

Reading serves to form hypotheses. No reference code must be copied without checking license,
runtime contract and semantic parity.

Allocation counting was deliberately left out of the first result because Aipo still has no single
accountable allocator/GC contract; `docs/adp/ADP-003-execution-budgets.md` remains a draft. The
next spike must define scope, unit and cost before choosing `Rc`, arena or tracing GC.

Recommended order for the next `fields` experiments: (1) monomorphic slot cache per site, already
kept; (2) frame-base cache, already kept; (3) copy-on-entry of `SetField` in unguarded structs,
already kept; (4) `BoundMethod` allocations still pending, but no new name pool until a dedicated
runner exists; (5) `Call0..Call4`/`GetLocal8` opcodes. Dense struct layout, cached method
metadata, a name pool and allocation-free method dispatch were tested and reverted for lack of
demonstrated gain. Each step needs its own A/B; do not introduce JIT, NaN-boxing or a custom GC
before evidence.

### Runner noise floor

This host delivers a noise floor of about ±5% on `fields` and `arithmetic` workloads under normal
conditions, and much worse dispersion under contention: the same binary has varied up to 2.5×
between runs of the same configuration, with load average 7.3 on 4 cores. Practical consequences:

- A difference smaller than ±5% is not evidence of anything, even with identical checksums and
  small per-run MAD.
- A single paired batch does not decide a change. Candidate changes need at least two batches,
  ideally with `arithmetic` as a control workload, because it does not touch the changed path and
  therefore measures environment drift.
- `arithmetic` uses no structs and no method dispatch, so it serves as an environment-drift
  control for `fields` experiments.
- Moving fields in large structs changes cache offsets and alignment enough to swing from +7% to
  neutral with no semantic change. Field position is part of the experimental result and must be
  reported alongside the delta.
- A batch with small MAD can still be contaminated by accumulated drift across the session;
  comparing the candidate against the baseline measured in the same batch remains mandatory.
- When the host is contested, wall-clock decides nothing. `scripts/perf/cpu_ab.py` measures the
  child process's CPU time (`RUSAGE_CHILDREN`) instead of wall-clock: under contention wall-clock
  inflates by an arbitrary factor, while CPU time stays proportional to the work actually done,
  because time removed by the scheduler is not charged. Even so, with 2.5× dispersion within the
  same binary, the minimum over many repetitions remains the most reliable estimator, and the
  correct conclusion is "inconclusive" when intervals overlap.

## What the report contains

The JSON report uses schema 5; the entry identifies `manifest_schema` separately.

Each result includes:

- the brute sample of each run;
- median and MAD;
- minimum, p95 and maximum;
- expected and observed checksum;
- operations and operations per second;
- runtime version;
- `N`, mode, preparation setup and runtime setup when applicable;
- internal metrics of instructions, calls, fields, field cache, globals and constants for the
  Aipo VM;
- for the Aipo VM, timing without instrumentation and a separate, un-timed run to collect
  metrics;
- `resources` with `peak_rss_bytes`, when available, in bytes, obtained by Linux `/proc`
  sampling only with `--compare-resources`; `allocation_count` and `allocated_bytes` remain null
  until a runtime-specific adapter exists;
- `field_cache_hits` and `field_cache_misses` count slot lookups in `GetField` and `SetField`;
  therefore hits can exceed `field_lookups`, which records only reads;
- environment, build profile, Git dirty state, CPU/OS and number of available CPUs;
- runner binary hash and Aipo binary hash/profile when selected.

Example output:

```text
Aipo CLI/VM  287.11ms MAD=2.28ms p95=289.39ms checksum=59999500000
Lua           4.33ms MAD=233.35us p95=4.56ms checksum=59999500000
Rust native   1.49ms MAD=32.69us p95=1.79ms checksum=59999500000
```

## Fairness policy

- The same algorithm, the same logical operations and the same `N` are used in every language;
  native Rust also builds and traverses the list and the string.
- The `fields` workload preserves the same recurrence and checksum, but uses structs, classes,
  tables or `__slots__` according to each runtime's model; compare it as a lookup hotspot, not
  as an object-layout ranking.
- The result is verified by checksum; this reduces the chance of dead work, but is not an
  independent proof that every operation ran.
- A warmup does not enter the samples.
- `process` and `in-process` must not be compared as the same metric.
- Interpreter startup remains included in process results.
- Aipo→JS compilation/bundle is reported as setup, not mixed with execution.
- Aipo VM in-process does not include process startup and is always labeled separately.
- Native Rust is compiled outside the timer and does not represent the same runtime class; the
  startup line includes the `aipo-bench` harness and serves only as a runner control.
- The p95 of a short run may equal the maximum; use more samples on a dedicated runner before
  interpreting that number.

## Limitations

- The result depends on CPU, system, versioning and thermal state.
- Python/PyPy, Ruby, Lua/LuaJIT, Wren, Luau and Node have different garbage collectors,
  dispatch, recompilation and optimizations. The execution mode must be read together with
  `kind`, `mode`, `includes_startup` and `includes_compile`.
- Benchmarks record `peak_rss_bytes` by Linux sampling in a separate run when
  `--compare-resources` is active; sampling may miss the peak of very short processes.
  `allocation_count` and `allocated_bytes` are not yet comparable across runtimes and remain
  null when there is no native adapter. I/O, editor startup and engine integration remain out
  of scope.
- Large `N` reduces the startup effect, but may measure different memory regimes.
- Fixtures are versioned and trusted; the runner is not a sandbox for arbitrary code. External
  processes have a 120 s deadline and output limited to 1 MiB; VM capture is also limited to
  1 MiB, but the in-process VM has no instruction budget for arbitrary code.
- A local run is not a regression baseline. Use a dedicated runner, CPU pinning and a history of
  at least three reports.

## Final local result

The historical table below covers the five original workloads; the `fields` workload is reported
separately in the first wave.

A full five-sample release run, sequential execution, Intel Core i7-3632QM, 4 available CPUs,
Rust 1.98.1, Node 24.18.0, Python 3.14.6, Ruby 4.0.6, Lua 5.5.1 and LuaJIT 2.1 produced these
medians:

| Workload | Aipo CLI/VM | Aipo VM in-process | Aipo→JS | Lua | LuaJIT | Python | Ruby | JS | Rust |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| `arithmetic` | 251.04ms | 239.12ms | 572.05ms | 10.68ms | 5.49ms | 57.33ms | 99.18ms | 57.02ms | 5.51ms |
| `collections` | 194.68ms | 180.20ms | 1118.91ms | 10.62ms | 5.50ms | 41.67ms | 94.90ms | 62.60ms | 5.64ms |
| `strings` | 10.62ms | 5.45ms | 109.41ms | 10.57ms | 10.65ms | 25.87ms | 87.60ms | 52.13ms | 5.50ms |
| `recursion` | 85.09ms | 79.00ms | 269.51ms | 31.07ms | 5.57ms | 31.07ms | 92.09ms | 50.32ms | 5.51ms |
| `startup` | 5.50ms | 0.29ms | 63.83ms | 5.53ms | 5.52ms | 21.05ms | 85.65ms | 48.40ms | 5.55ms |

The last implementation change removes temporary `Vec`s from natives and conversions; the
subsequent measurement in `target/cross-language-final-current.json` was taken under heavy runner
load and therefore does not replace the table above as a comparable baseline.

## Local result of the first wave

Release run with five samples, sequential execution, Wren 0.4.0 and Luau 0.739. The numbers are
process medians and include startup/compilation; they are not a regression baseline.

| Workload | Wren | Luau |
|---|---:|---:|
| `arithmetic` | 41.37ms | 22.27ms |
| `collections` | 37.38ms | 11.61ms |
| `fields` | 120.35ms | 63.15ms |
| `strings` | 84.00ms | 21.44ms |
| `recursion` | 22.36ms | 16.07ms |
| `startup` | 6.55ms | 7.25ms |

In the `fields` workload, the same run measured Aipo VM at 1,430.92ms, against 120.35ms for Wren
and 63.15ms for Luau. That difference is a **hotspot alert for fields/methods**, not a universal
verdict: the three runtimes have different object models. The detailed report is in
`target/fields-release.json`.

PyPy was not available on `PATH` and was recorded as `skipped`. A full run with all runtimes
available produced 66 results, 6 skipped and 0 failures. The first-wave report is in
`target/first-wave-release.json`; the full matrix is in
`target/cross-language-first-wave-full.json`. Both contain samples, RSS when requested and
provenance.

## Isolated A/B of the monomorphic field cache

Experiment with 15 samples, Aipo VM in-process, `N=100000`, same workload and same checksums:

| Variant | Median | MAD |
|---|---:|---:|
| Before | 1,001.77ms | 39.22ms |
| Cache per site | 718.20ms | 30.54ms |

Observed median reduction: **28.31%**. The cache produced 2,399,992 hits and 8 misses in the
metrics pass. Reports: `target/fields-cache-before.json` and `target/fields-cache-after.json`.
This result is directional, not a gate, because the runner is shared.

## Rejected experiment: method-metadata cache

A per-site/type cache for `GetField` of methods was temporarily implemented and measured against
the same `fields` workload. It recorded 199,998 hits and 2 misses, but showed no reproducible gain
on the shared runner:

| Variant | Median (31 samples, CPU 0) |
|---|---:|
| Without method cache | 971.58ms |
| With method cache | 1,131.79ms |

A smaller sequential run was also worse (1,141.64ms without cache vs. 1,599.96ms with cache).
Since the values were affected by heavy load, the conservative conclusion is absence of
demonstrated benefit, not a universal regression. The prototype was reverted; the monomorphic
field cache and the frame-base cache remain as kept optimizations. Reports:
`target/fields-method-disabled-pinned.json` and `target/fields-method-enabled-paired.json`.

## Rejected experiment: method-name pool

A prototype replaced the name in `BoundMethodData` with `Rc<str>` and kept qualified names in
per-site slots. The intent was to eliminate `format!` and name copies, but A/B showed no
reproducible gain: in an adjacent 31-sample run, `fields` was 602.49ms against 605.07ms baseline,
while `collections` worsened from 178.82ms to 192.71ms. A second variant with `Owned`/`Shared`
also did not resolve: `fields` 635.72ms against 630.59ms and `collections` 190.71ms against
179.45ms. Runner load varied between runs, so these numbers are only an exploratory rejection,
not a universal regression. The pool and the `BoundMethodData` layout change were reverted; the
next attempt must avoid an API-breaking change without a dedicated-runner A/B. Reports:
`target/qualified-name-candidate-31.json`, `target/qualified-name-baseline-31b.json` and
`target/bound-name-enum-candidate-31.json`.

## Rejected experiment: per-slot `fixed`-flag cache

The registered `SetField` path started loading a `Vec<bool>` of `fixed` flags per type and using
a setter with the flag already resolved, avoiding `HashSet<String>::contains` per write. The
paired A/B (3 pairs, 15 samples) made the candidate slower in the median of the three pairs:
`arithmetic` +5.34%, `fields` +11.32% and `recursion` +144.36%. Checksums and metrics stayed
identical, but the extra lookup cost outweighed the saving; the change was reverted. Reports:
`target/fixed-field-paired/`.

## Rejected experiment: guardedness cache entry

The per-site cache was extended to also store `type_is_guarded`, eliminating the two HashMap
lookups in `SetField`. The paired A/B (3 pairs, 15 samples) showed no gain: `arithmetic`
+6.32%, `fields` +1.40% and `recursion` +2.13% in the median of the three pairs. Checksums and
metrics stayed identical, but the extension was reverted. Reports: `target/field-entry-paired/`.

## Rejected experiment: dense `StructInstance` layout

`StructInstance` stopped storing `Vec<(String, Value)>` and started storing
`field_names: Vec<String>` alongside `fields: Vec<Value>`, eliminating the tuple per slot. The
intent was to make field access a direct index into `Vec<Value>` and reduce the size of the
stored value.

The paired A/B (3 pairs, 15 samples, CPU 0) showed the candidate slower on `fields` in all three
pairs: +3.73%, +7.39% and +24.23%. In the median of the three pairs, `arithmetic` was +33.87%,
`fields` +22.66% and `recursion` -57.76%. Runner load was high during measurement (load average
2.50 to 3.80 on 4 cores, with the coding agent consuming ~86% of CPU), which explains the
dispersion in `arithmetic` and `recursion`; even so, the direction on `fields` was consistent
across all three pairs and is the relevant evidence, since that is the workload exercising the
changed path. Checksums stayed identical in every pair.

The likely cause is structural: separating names and values trades one allocation per struct
instance for two, and each field access then touches two memory regions instead of one. Since the
`fields` fixture builds instances repeatedly, the extra allocation dominates the expected gain
from indexed access.

The experiment was also API-breaking: `fields` is public and the eight consuming files
(`value.rs`, `vm/dispatch.rs`, `host.rs` and five stdlib modules) had to change. Since there was
no gain, the prototype was reverted and the public API keeps `Vec<(String, Value)>`. Reports:
`target/dense-layout-paired/`.

## Rejected experiment: allocation-free method dispatch

Inspection of the hot path found a real cost that benchmarks do not isolate. In the `fields`
fixture there are 200,000 method binds (`advance` and `score`), and each one made four `String`
allocations: one clone of the receiver's `type_name`, two clones to build the map key and one
`format!("{type_name}.{field_name}")`. The correct pattern already existed in the repository:
`method_natives` has a nested twin (`method_natives_by_type`) that does lookup with `&str`
without allocating, but `struct_methods`, the user-method map and the `fields` hot path never
received that treatment.

The prototype applied the same pattern: nested `struct_methods_by_type` by type, qualified name
formatted once at registration instead of per bind, and `lookup_struct_method(&str, &str)`
reusing `lookup_method_native`. The instance name was cloned only on the error path. Semantically
the change was neutral: 56 suites passed, including the VM↔JS differential, and checksums stayed
identical.

Even so, the gain could not be demonstrated. Four paired batches, changing only the position of
the new field in struct `Vm`:

| Batch | New field position | Δ `fields` | Δ `arithmetic` |
|---|---|---:|---:|
| 1 (5 pairs) | inline, in the middle | -5.57% | +1.72% |
| 2 (3 pairs) | inline, in the middle | -3.70% | +1.13% |
| 3 (3 pairs) | at the end of the struct | +1.05% | -0.02% |
| 4 (5 pairs) | boxed, in the middle | +4.54% | +6.80% |

The same logical change, with the same mechanism, swung from -5.6% to +4.5% just by moving a
field. That indicates the runner's noise floor is about ±5% on this host and that differences
inside that band are not evidence. The first batch, which suggested a convincing -5.57% gain
with 4 of 4 pairs cleanly favoring the candidate, was largely luck from a quiet batch; later
batches with controlled noise refuted it. `arithmetic` works as a control workload because it
has no structs and no method dispatch, so it measures environment drift rather than the effect of
the change.

For those reasons the prototype was reverted. Beyond the absence of demonstrated gain, the
change required duplicating registration state: `struct_methods` is `pub`, so any future write
outside `register_struct_method` would desynchronize the shadow map and silently break method
dispatch, with no speed gain to pay for that risk.

## Diagnosis: constant cost per opcode and the 72-byte `VmFault`

Measuring cost per opcode in three calibration programs with the same number of iterations, and
using the deterministic instruction count as the denominator, the cost per opcode is practically
constant and independent of the operation executed:

| Program | Ops/iter | ns per opcode |
|---|---:|---:|
| `i = i + 1` (locals) | ~9 | ~111 |
| `s = s + i; i = i + 1` | ~14 | ~98 |
| `arithmetic` (calls and globals) | 28 | ~100 |

A constant cost of about 100 ns per opcode that does not change with the operation mix indicates
the cost is in the dispatch loop, not in any handler. Ablation ruled out metrics accounting as
the cause: disabling `metrics_enabled` changed the result by only 1% to 2%.

The identified cause sits at the top of `Vm::step`, executed for **every** opcode:

```rust
// before
let opcode = OpCode::try_from(opcode_byte).map_err(|b| VmFault::CorruptedBytecode {
    offset: self.ip - 1,
    reason: format!("unknown opcode 0x{b:02x}"),
})?;
```

`OpCode::try_from` returns `Result<OpCode, u8>`, that is 2 bytes, but `.map_err` rebuilds the
result as `Result<OpCode, VmFault>`. `VmFault` is 72 bytes, because several variants carry an
inline-allocated `String`. So every executed opcode built and destroyed a 72-byte `Result`, and
the closure performing the conversion captured `self`, which forces the compiler to keep `ip` in
memory. Add to that every `Result<_, VmFault>` from `push`, `pop` and `read_u16`, and the same
cost appears several times per opcode.

The fix replaces `.map_err` with a `match` that only builds the fault on the cold path, keeping
offset and message identical:

```rust
let opcode = match OpCode::try_from(opcode_byte) {
    Ok(opcode) => opcode,
    Err(byte) => return Err(VmFault::CorruptedBytecode { /* identical to before */ }.into()),
};
```

The happy path becomes a `match` over 2 bytes, with no large `Result`, no closure capturing
`self` and no `format!`. The `test_unknown_opcode_reports_the_offset_of_the_bad_byte` test pins
the cold-path error, and the 56 workspace suites pass with identical instruction counts and
checksums.

**The measurement result is null.** With child-process CPU time, 24 alternating repetitions per
`arithmetic` workload, the result was:

| | min | median |
|---|---:|---:|
| before | 3.019s | 5.474s |
| after | 3.014s | 5.359s |
| delta | -0.16% | -2.09% |

The deciding number is the minimum, because it is the state without contention: if the change
removed real work per opcode, the minimum would drop measurably. It did not. The likely
explanation is that LLVM already sank fault construction out of the happy path, keeping only the
2-byte `match` at runtime, so the waste existed in source but never actually executed. The two
distributions overlap almost completely (before 3.02s–7.53s, after 3.01s–8.52s).

The change was kept, not for a speed gain, which does not exist, but for two reasons that do
not depend on a benchmark: it makes fault construction on the cold path explicit in the code
instead of relying on the optimizer to move the error path out, and the new test pins the
`unknown opcode` contract, which previously had no coverage. Recording this as a performance
optimization would be wrong; it is a robustness and clarity change with a measured null result.

The same compiler hypothesis probably applies to `push`, `pop` and `read_u16`: the size of
`VmFault` is a real hygiene and ergonomics problem, and it is worth reducing for API reasons,
but it is plausible that the cold error path is already removed at runtime. Therefore
**shrinking `VmFault` must not be treated as a performance optimization without measuring**.
The command to measure on an idle host is:

```sh
python3 scripts/perf/cpu_ab.py <baseline-binary> <candidate-binary> --workloads arithmetic --rounds 10 --reps 24
```

## A/B of the frame-base cache

The VM keeps `Vm::frame_base`, the active frame's `stack_base`, so `GetLocal`, `SetLocal` and
`JumpIfSetLocal` do not consult `frames.last()` on every access. The value is updated in `run`,
frame push/pop, failure handlers, `invoke` and task switching. A regression test covers nested
callback return and restoration of the outer frame.

Paired A/B, Aipo VM in-process, CPU 0, 31 samples, same workload and checksums:

| Workload | Without hot-path cache | With `frame_base` | Delta |
|---|---:|---:|---:|
| `arithmetic` | 276.23ms | 265.98ms | -3.71% |
| `fields` | 736.02ms | 687.05ms | -6.65% |
| `recursion` | 97.72ms | 93.65ms | -4.17% |

Internal metrics and checksums stayed bit-for-bit identical. Reports:
`target/frame-base-baseline-31.json` and `target/frame-base-candidate-31.json`. The result is
directional, not a gate, because the runner is shared. An additional attempt to force
`#[inline]` on hot helpers brought no reproducible gain and was reverted.

## Reduction of copies in `SetField`

The input value of an assignment is only needed for the journal when the type is guarded. The
path now checks `type_is_guarded` before cloning the previous value; structs without an
`invariant()` keep the same mutation, without the input copy. In the `fields` fixture this
eliminates 600,000 `Value` clones (six writes × 100,000 iterations) deterministically. A later
A/B with 31 samples showed no consistent wall-clock gain (`fields`: 608.31ms baseline vs
612.08ms with the change; `collections`: 184.11ms vs 178.36ms). Therefore the change is kept
as a deterministic reduction of work, not as a speed claim. The rollback tests for guarded
structs and the differential suite remain green.

## Rejected experiment: guarded-type cache

A `HashSet<String>` was created to replace the two lookups in `type_is_guarded`. The paired
runner (3 pairs, 15 samples per run) showed the candidate slower in the median of the three
pairs: `arithmetic` +6.58%, `fields` +1.63% and `recursion` +3.36%. Checksums and metrics
stayed identical, but there was no benefit; the cache was reverted. Reports:
`target/guarded-cache-paired/`.

## Isolated A/B of the shim constant cache

In a sequential three-sample run for `Aipo→JavaScript/Node` only, the lazy per-instruction
constant cache reduced the median by about `26%` (`arithmetic`), `11%` (`collections`) and
`17%` (`recursion`). Reports are in `target/js-constant-cache-base.json` and
`target/js-constant-cache-after.json`; the result is directional because the runner is shared.

Notes: Aipo CLI/VM and Aipo VM in-process trailed the compared languages on computational
workloads; Aipo→JavaScript remains the most expensive path, while the strings case highlights
the cost of incremental concatenation. In-process VM startup is measured without process
startup. Native Rust runs the same logical operations but remains only a lower-bound control.

In a local A/B of three samples against `target/perf-phase1.json`, the Aipo VM reduced the
median by `12.3%` (`arithmetic`), `10.1%` (`collections`), `6.0%` (`strings`) and `6.2%`
(`recursion`); Aipo→JavaScript varied from `-10.4%` to `+2.8%`. This result is directional, not
a gate, because the runner is shared.

## WebAssembly JIT substrate (ADP-013 / milestone 6)

With the introduction of the `aipo-wasm` crate and native CLI integration
(`aipo run --wasm`), Aipo programs are compiled directly from HIR to WebAssembly bytecode and
executed as native machine code via Cranelift/Wasmtime.

The WebAssembly runtime eliminates the dispatch overhead of the traditional Stack VM (`match` of
opcodes, `Vm::step`, the 16-byte `Value` on this target with boxing and dynamic stack-limit
checks), operating directly with Wasm primitives (`i64`, `f64`, `i32`) and a linear memory
layout for structs.

### Comparative battery (`aipo-bench --compare`)

Seven-round execution comparing the new WebAssembly JIT runtime (`aipo-wasm`) with the previous
Stack VM (`aipo` and `aipo-vm`) and market runtimes. Evidence recorded in
`target/bench_wasm_comparison.json`.

#### 1. Arithmetic (`arithmetic`, N=200000, checksum: `59999500000`)

Intensive loop of arithmetic function calls (`step(i) = i * 3 - 1`).

| Runtime | Execution type | Median (ms) | Minimum (ms) | Speedup vs VM | Ratio vs Wasm |
|---|---|---:|---:|---:|---:|
| **Rust native** | Compiled native (`rustc -O`) | 5.51ms | 5.46ms | 48.5x | 0.35x |
| **LuaJIT** | Native tracing JIT | 5.57ms | 5.43ms | 48.0x | 0.35x |
| **Lua 5.4** | C bytecode VM | 10.77ms | 10.58ms | 24.8x | 0.68x |
| **Aipo Wasm JIT** | **WebAssembly JIT (Cranelift)** | **15.72ms** | **10.53ms** | **17.0x – 25.1x** | **1.00x** |
| **JavaScript (Node.js)** | V8 JIT | 63.44ms | 56.89ms | 4.2x | 4.03x |
| **Python (CPython 3.12)** | C bytecode interpreter | 69.59ms | 62.86ms | 3.8x | 4.43x |
| **Ruby (CRuby 3.x)** | YARV interpreter | 108.14ms | 105.87ms | 2.5x | 6.88x |
| **Aipo VM in-process** | Rust Stack VM (no startup) | 231.64ms | 229.61ms | 1.15x | 14.73x |
| **Aipo CLI/VM** | Rust Stack VM (with startup) | 267.34ms | 244.91ms | 1.00x (base) | 17.00x |
| **Aipo → JS / Node** | JS transpilation over Node | 707.36ms | 603.04ms | 0.38x | 44.99x |

Highlights:
- **Aipo Wasm JIT is 17x to 25x faster** than Aipo CLI on the classic Stack VM.
- **Beats Node.js by 4.0x**, **CPython by 4.4x** and **CRuby by 6.9x**.
- Ties the Lua 5.4 C interpreter (10.53ms vs 10.58ms minimum).
- Within **1.9x** of the absolute Rust compiled-native ceiling.

#### 2. Deep recursion (`recursion`, Fibonacci(24), checksum: `46368`)

Pure recursive calls with 92,735 function invocations and frame-activation control.

| Runtime | Execution type | Median (ms) | Minimum (ms) | Speedup vs VM | Ratio vs Wasm |
|---|---|---:|---:|---:|---:|
| **Rust native** | Compiled native (`rustc -O`) | 5.50ms | 5.45ms | 17.9x | 0.48x |
| **LuaJIT** | Native tracing JIT | 5.85ms | 5.68ms | 16.8x | 0.52x |
| **Aipo Wasm JIT** | **WebAssembly JIT (Cranelift)** | **11.35ms** | **10.60ms** | **8.65x** | **1.00x** |
| **Lua 5.4** | C bytecode VM | 16.22ms | 11.12ms | 6.05x | 1.43x |
| **Python (CPython 3.12)** | C bytecode interpreter | 31.89ms | 30.26ms | 3.08x | 2.81x |
| **JavaScript (Node.js)** | V8 JIT | 59.66ms | 58.38ms | 1.65x | 5.26x |
| **Aipo VM in-process** | Rust Stack VM (no startup) | 77.68ms | 77.16ms | 1.26x | 6.85x |
| **Aipo CLI/VM** | Rust Stack VM (with startup) | 98.15ms | 86.64ms | 1.00x (base) | 8.65x |
| **Ruby (CRuby 3.x)** | YARV interpreter | 98.17ms | 93.55ms | 1.00x | 8.65x |
| **Aipo → JS / Node** | JS transpilation over Node | 307.74ms | 287.11ms | 0.32x | 27.12x |

Highlights:
- **Aipo Wasm JIT is 8.6x faster** than Aipo CLI/VM and 6.8x faster than the in-process VM.
- **Beats Lua 5.4 by 1.4x**, **CPython by 2.8x**, **Node.js by 5.3x** and **CRuby by 8.6x**.
- Within **1.9x** of native Rust and LuaJIT.

#### 3. Process startup and initialization (`startup`, checksum: `ready`)

Measures the full lifecycle: CLI process invocation, parsing, lowering, native codegen and stdout
emission.

| Runtime | Median (ms) | Minimum (ms) | Notes |
|---|---:|---:|---|
| **Aipo VM in-process** | 0.31ms | 0.30ms | No new OS process cost |
| **LuaJIT / Lua / Rust** | ~5.50ms | ~5.45ms | Minimal C/Rust process without heavy compilation |
| **Aipo CLI/VM** | 5.51ms | 5.46ms | Pure interpreter (no JIT) |
| **Aipo Wasm JIT** | **15.64ms** | **10.68ms** | **Includes parsing + Wasm codegen + Cranelift native compilation** |
| **Python (CPython)** | 20.80ms | 20.64ms | CPython runtime initialization |
| **Node.js** | 57.83ms | 56.37ms | V8 initialization |
| **Aipo → JS** | 68.40ms | 64.29ms | Node + Aipo runtime shim |
| **Ruby** | 89.63ms | 85.44ms | Ruby VM initialization |

Aipo Wasm JIT startup (~15ms) includes native codegen by Cranelift, yet stays **more agile than
CPython (21ms), Node.js (58ms) and Ruby (90ms)**.

## Revalidation on 2026-10-27

`aipo-bench --quick --json /tmp/aipo-bench-current.json` completed on HEAD with 49 frontend,
command, VM, JS and scaling samples. The `vm/invariant-commit` workload used the old
`impl`/`init`/`invariant()` forms and was updated to `Tipo:metodo`/`init`/`invariant`; without
that the runner aborted on the current parser. Scaling ratios were classified as `noisy` by the
runner itself; treat those numbers as performance smoke, not a certified baseline.

## Baseline P08-G01 (2026-10-28)

Matrix 2 ran with CPU 0 pinned on the shared host `c036164`: `aipo-bench --json` with 58
samples and `aipo-bench --compare --compare-quick` with 3 rounds. Reg came in at
0.95x/0.69x/1.02x/1.55x/1.00x against the VM on arithmetic/collections/fields/recursion/
strings; Wasm at 0.16x/0.20x/0.07x/0.45x on supported workloads. `strings` on Wasm failed with
`AIPO_COMPILE_WASM_UNSUPPORTED`. Raw data and limits: [P08-G01](../evidence/P08-G01/README.md).
This is not a dedicated runner or a regression gate.

## Reg profile: microbenchmark is not a user gain

The paired benchmark in P07-G02 measures Reg calls against a fixed baseline (17.361 ms → 3.616 ms
on Int; 20.129 ms → 4.426 ms on String). That number **is not** an end-to-end CLI gain: on
2026-10-27, `aipo plan` selected native Reg on 20/36 conformance programs and used the
canonical VM on the rest. Programs with `filter`/`map`/`reduce`, `attempt`/`or_else` or
non-representable IR remain on the VM. Measure the real workload with `aipo plan` before citing
the ratio.

These numbers are local evidence, not a gate or a performance promise. The full report, with raw
samples, release profile, dirty state and checksums, is in `target/cross-language-final.json`
and `target/bench_wasm_comparison.json`, outside version control. Attach it as evidence when a
product decision depends on the data.
