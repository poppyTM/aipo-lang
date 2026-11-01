# P07-G01 — Runtime reconstruction, studies and guide

**Date:** 2026-10-09. **Goal:** `P07-G01`, kept in `DRAFT`.
**Base studied:** `488905ffb0b83ebbff458939542350d7280ad79b`.
**Base integrated:** `bcbc4c991dee1d9f853c597aaed2790e8981b8b9`.
**Branch:** `audit/runtime-hardening-and-studies`.

This delivery rebuilds fixes after the temporary files of the previous round were lost. It is not a
byte-for-byte recovery of that patch. The old test and benchmark results do not certify this
reconstruction. The user authorized commit, push and PR and asked: "no need to run the tests". No
test, JS selftest, conformance program or benchmark was executed in this round.

## Delivered implementation

| Area | Problem in base | Change and rationale |
| --- | --- | --- |
| Reg emitter | Unknown IR was ignored; unknown operators became equality | APIs return `Result`, preflight rejects unrepresentable resources and operands |
| Reg flow | Temporary height followed textual order of branches | CFG computes heights, checks underflow/joins and emits using each instruction's entry |
| Reg encoding | Compressed iteration index in C; counts and destinations could truncate | New opcodes use full C; register, pool, offset and 256-item-list limits are checked |
| Reg values | Incomplete duplicated arithmetic | Reuse of `Value` helpers for promotion, finiteness, safe limit and equality |
| Reg execution | Invalid operand B, pools, arguments and destinations could fail incorrectly | Checks by operand role and explicit errors; strict lookup does not become `none` |
| Reg activations | Call state and handlers were vulnerable to incorrect propagation | Metadata/arity checked, caller restored, local handlers and immediate failure return without handler |
| Reg limits | No instruction limit | Optional cumulative API with explicit reset; callbacks remain outside the limit |
| Reg iteration/contracts | Guards and contracts without lowering | Guards per activation and fundamental/nominal contracts; interfaces with operations rejected |
| Dict in VM/Reg/JS | Primary returned value instead of key | Keys in insertion order, per canon; JS expectation corrected manually |
| Strings VM/Reg | Indexing built a temporary `Vec<char>` | Shared ASCII/Unicode helper, negative index and `i64::MIN` without negation causing overflow |
| Wasm | Missing entrypoint and output errors could look like success | Errors propagated, output delivered after start/entrypoint traps and optional fuel before instantiation |
| Dead Wasm code | Private indices never read and unused span helper | Removal of dead metadata, preserving exported helpers |
| Features and regressions | JIT/formatter suites did not respect reduced builds | Feature guards, Reg cases and two new fixtures; code compiled, not executed |
| Studies | Assumptions and references without sufficient reproducible linkage | Manifest of nine SHAs, external/offline verify and notes with directed reading and bounded proposals |
| Documentation | No consolidated guide to reproduce the changes | [Complete guide](../development/runtime-hardening-guide.md), CLI reference, diagnostics, contracts and roadmap synchronized |

The `wasm-encoder` update to `0.261.0` came from a merge on `main` during the work. It was incorporated by fast-forward before the final checks; this PR does not attribute that update to the audit.

## Verification actually performed

Results and hashes of implementation files are in [P07-G01-checks.json](P07-G01-checks.json). Compiling test targets means only type checking: `cargo check --all-targets` does not run their tests.

| Check | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Recorded in JSON after the final review |
| `cargo check` of CLI/VM/bytecode/Wasm, `--all-targets --locked`, default features | Recorded in JSON; includes regression targets and examples without running them |
| CLI with Wasm compiler and without Wasmtime, `--all-targets --locked` | Recorded in JSON |
| CLI without default features, `--all-targets --locked` | Recorded in JSON |
| `node --check` on runtime and JS selftest | Syntax accepted; selftest not executed |
| AST of Python scripts and reading of changed JSONs | Syntactic inspection, without running benchmarks |
| Local links of changed pages | Destination existence checked; not a VitePress build or anchor verification |
| `fetch-refs.sh --verify` | The nine HEADs, origins, clean trees and lock match the manifest |
| `git diff --check` | Recorded in JSON |

Environment: Rust `1.99.0` (`b940084d7`, host `x86_64-unknown-linux-gnu`), Node `24.19.0`, Python `3.12.14`. Builds use `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=2` and `CARGO_PROFILE_DEV_DEBUG=0` due to resource constraints. These are not performance measurements or release builds. The declared MSRV `1.85` was not certified.

## Pending gates

| Gate | State and reason |
| --- | --- |
| Rust, JS, CLI and conformance tests | Not executed by explicit user instruction |
| Benchmarks/layout probes | Not executed; protocol and harness delivered for another round |
| Clippy, rustdoc and MSRV | Not executed in this reconstruction |
| Prumo validate/doctor/docs verify | Not executed; no approval or formal transition was fabricated |
| VitePress documentation build/render | Not executed; documentation can be read directly on GitHub |
| Full upstream audit | Not completed; reading limited to the regions identified in the notes |
| Human review and merge | Pending; PR published as draft |

The intelligence record was updated with this delivery in review state, with no fictitious test count. The historical state of `prumo.json` was preserved. The user's authorization to publish does not equal certification of all gates.

## Compatibility and continuity

The experimental Reg compilation APIs now require `Result` handling; new private fields require `RegVm::new`/`Default`. New opcodes need this revision's runtime. Consumers that relied on the Dict Primary bug (value instead of key) must migrate to the canonical `key, value` binding.

RegVM still lacks full parity of methods, closures/upvalues, async, hooks, invariants, mutation journal and host. Length guards do not detect every same-size removal/insertion. Wasm fuel does not limit memory, output or native work. The [guide, sections 14–16](../development/runtime-hardening-guide.md#14-o-que-ainda-pode-melhorar) orders these pending items and describes how to reimplement and validate each layer.

To reproduce this delivery, use the PR commit SHA, the dependency lock and the SHAs in `studies/refs.json`. The PR diff and published files replace the temporary links that stopped working.
