# P07-G02: implementation, validation and performance

**Date:** 2026-10-09 · **State:** DRAFT · **Tests:** not executed by explicit user instruction.
**Validated and measured source:** `33d524b59caecd8b0d33f48f385a64010e14a3c1` · **Tree:** `42bfc0d63f9b23484e342991713814372339042c`.

The environment was recovered and the code was published on `feat/runtime-and-tooling-completion`. The remote checkpoint tree was compared with the local one, followed by the final Wasm correction and integration of `main` `23bab1d2`. The [complete guide](../../development/runtime-and-tooling-guide.md) describes what changed, why, how to use it, compatibility and reimplementation.

> **Reconciliation of 2026-10-27 (working tree over `ab5fbc0`, not committed):** the full workspace passed with 1,130 tests (including Poppy, eGUI and game host), conformance VM/Reg with 36/36, portal validators with 13 tests and the VitePress build; Clippy, rustfmt, strict rustdoc and tests passed on Rust 1.96.0; the shell, web and embedded profiles also compile on MSRV. The cross-platform matrix is still pending. The backend matrix measures Reg/Wasm and the README was corrected. The goal remains DRAFT.

## Check results

| Check | Result | Scope |
| --- | --- | --- |
| `cargo +1.96.0 clippy --workspace --all-targets --locked -- -D warnings` | exit 0 | Whole workspace; compiles test targets without running them |
| `cargo +1.96.0 fmt --all -- --check` | exit 0 | Code formatting |
| `cargo +1.96.0 doc --workspace --no-deps --locked`, `RUSTDOCFLAGS=-D warnings` | exit 0 | Current rustdoc, regenerated after clearing the retained output path |
| CLI `check --all-targets --locked --no-default-features`, with shell/web/embedded and without alias | exit 0 in all four cases | Reduced builds on MSRV 1.96 |
| C11 and C++17 header `-fsyntax-only`, warnings treated as errors | exit 0 | Syntax; does not link or run a C consumer; 29 exports found/declared |
| Python `ast.parse` on release/perf scripts | OK | Syntax; does not run installer/packager |
| Links/guides, `git diff --check` | OK | Document structure and whitespace |
| Portal: check-docs, check-usage-docs and verify-progress | exit 0 | 141 pages, 26 exact examples, 33 processes/49 proofs; does not run Aipo |
| Portal: `npm run build` | exit 0 | VitePress; Aipo fence language falls back to plain text |
| Rust/Aipo/differential tests, fuzz, Miri, ASan and portal unit tests | Not executed | User instruction; no invented pass counts |

Commands, environment and limits are in [checks.json](checks.json). `npm run check` was not invoked because it includes unit tests; its three static validators were called separately. Cross-platform workflows were not observed; no release was published.

Clippy 1.96 pointed out two boolean simplifications in parser and cache-test source; they were fixed without weakening the lint. Initial rustdoc failed when removing a retained output directory: the generated directory was moved, and the rerun in a fresh output path with one job passed. Earlier failures are not treated as success.

## Final Reg call benchmark

Baseline `f0a0d70d176be031cd4b600fde1df6179429fb9a` versus source
`33d524b59caecd8b0d33f48f385a64010e14a3c1`, **Rust 1.96.0/LLVM 22.1.2**, default
release, `CARGO_INCREMENTAL=0`, two jobs per build. The same `reg_calls_bench.rs` source copied
byte-for-byte to the baseline. Builds completed before collection; no compilation/npm of this
session ran during measurement. This does not guarantee the absence of all external host load.

Each run contains 20,000 calls, a nine-instruction caller, five registers in the caller and
one in the callee. Int and String cases return values consumed by `black_box`. The measurement
includes loading and verifying `run_module`, not just the Call instruction. CPU 0, three
batches × five pairs per case, alternating order; nine samples per process, first removed as
warmup. **480 observations retained**.

| Case | Baseline median | Candidate median | Baseline/candidate ratio |
| --- | ---: | ---: | ---: |
| int | 17.361 ms | 3.616 ms | 4.80× |
| string | 20.129 ms | 4.426 ms | 4.55× |

Per-batch ratios ranged from 4.40× to 4.90×. Consult the raw data rather than treating a single
ratio as a guarantee: [reg-calls-paired.csv](reg-calls-paired.csv) and [performance.json](performance.json)
include compiler, binary hashes, workload, source inventory and CSV.

After collection, the CSV was normalized from CRLF to LF for Git storage. The 480 observations
remain identical; the report preserves the previous hash and reports the published file hash.
The driver now writes LF in future collections. This serialization change does not alter the
Rust snapshot or require repeating the measurement.

Earlier measurements made before recovery were replaced by this collection. They used a different
revision/compiler and included concurrent builds; they did not certify the final snapshot. Do not
extrapolate to overall language speed, startup, allocations/RSS, structs, async, callbacks, MCU
or JIT.

### Reproduction

```sh
git worktree add --detach /tmp/aipo-base f0a0d70d176be031cd4b600fde1df6179429fb9a
cp crates/aipo-vm/examples/reg_calls_bench.rs /tmp/aipo-base/crates/aipo-vm/examples/
# In candidate and baseline, with separate target dirs:
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=2 cargo +1.96.0 build --locked --release -p aipo-vm --example reg_calls_bench
# With no builds running, choose an allowed CPU:
python3 scripts/perf/reg_calls.py --baseline /PATH/BASE/reg_calls_bench --candidate target/release/examples/reg_calls_bench --rustc /PATH/rustc --cpu 0 --batches 3 --pairs 5 --out /PATH/evidence
```

The supplied `rustc` must be the same 1.96 used in both builds. The driver does not create
baselines or update tests.

## Recovery and continuity

[recovery.json](recovery.json) preserves **the historical snapshot of the blockage**, when the
code had not yet been pushed. It does not describe the current state. The initial published
checkpoint was `4ad98242a8867da0ab62653d7843ffed83e164b9`; the `409 environment_offline`
disconnection was resolved. There was no automatically rejected approval and no need to renew
authorization for commit/push/PR.

[completion.json](completion.json) records the current state, revision and limits. P07-G02
remains DRAFT because certification tests/gates were not executed. Native Reg captures/async/
hooks, cycle/retention GC, transactional external resources, complete LSP/DAP/line editor,
registry/ranges and no_std/HAL/MCU remain future evolution, with criteria described in the guide.
