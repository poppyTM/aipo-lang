# Tooling decisions (P01-G02)

**Status:** record (process level)
**Scope:** every tool evaluated under §36: adopted / rejected / deferred, with why, toolchain
compatibility, how to run, CI tier and maintenance cost

## Adopted

| Tool | Why | Run | CI tier | Maintenance |
|---|---|---|---|---|
| `aipo-bench` (in-house runner, no benchmark/statistics dependency) | Statistically honest baselines (median/MAD), raw samples, cross-language checksums and sampled peak RSS | `cargo run --release -p aipo-bench -- [--quick] [--json out]`; `--compare` for Aipo/Lua/LuaJIT/Wren/Luau/Python/PyPy/Ruby/Node/Rust | PR compiles it; dedicated runner executes | Low: workloads, comparison manifest and fixtures |
| LLVM coverage via `llvm-tools` component + `docs/testing/coverage.sh` | Real line/region/function data with zero new dependencies | `bash docs/testing/coverage.sh [--report PATH]` | Scheduled (slow: full instrumented suite) | Low: script + component install |
| `cargo-fuzz` 0.13.2 + libFuzzer targets (`fuzz/`) | Found a real P0 (IR builder panic) within ~2k executions | `cargo +nightly fuzz run <target> -- -max_total_time=N` (needs `~/.cargo/bin` first on PATH — see `fuzz/README.md`) | Nightly scheduled | Medium: targets, seeds, artifact triage |
| `cargo-audit` 0.22.2 | Zero advisories over 31 deps, verified | `cargo audit` | Scheduled | Low |
| `cargo-deny` 0.20.2 + `deny.toml` | Licenses/bans/sources green after policy commit | `cargo deny check` | Scheduled (advisory DB needs network) | Low: `deny.toml` |
| Miri (nightly component) | Full `aipo-vm` suite green incl. `RefCell` sharing | `cargo +nightly miri test -p aipo-vm` | Scheduled (slow: ~2 min for the crate) | None unless FFI lands |
| `aipo-testkit` (in-house) | Shared deterministic harnesses (Rng, AipoSmith, differential, watchdog) with no new deps | `cargo test -p aipo-testkit` | PR (fast) | Low |

## Rejected (with reason, not by default)

| Tool | Why not |
|---|---|
| Criterion | Needs a new dependency + MSRV analysis for marginally better statistics than median/MAD at this stage; re-evaluate when benchmarks gate releases |
| proptest | Same tradeoff: deterministic in-house properties (`Rng` + seed printing) give reproducibility without the dependency; re-evaluate if property count explodes |
| AFL++ | libFuzzer covers the need; second engine adds maintenance without new bug classes |
| External profilers | No bottleneck justifies them yet; the one real bottleneck (f-string padding) was found with wall-clock ratios |

## Deferred (condition named)

| Tool | Condition to adopt |
|---|---|
| `cargo-mutants` | Manual 3-mutant spot-check done (3/3 caught, one requiring a new test); adopt when mutation runs fit a nightly schedule without blocking PRs |
| `cargo-nextest` | No test-isolation bug found (see `concurrency-audit.md`); adopt for `--rerun`/sharding when real CI exists |
| Sanitizers (ASan/TSan) | Relevant at native/FFI boundaries; none exist (`forbid(unsafe)`) |
| Iai/cachegrind | Dedicated performance runner prerequisite; wall-clock baselines come first |

## Environment notes

- Cross-language comparison discovers `lua`, `luajit`, `wren_cli`/`wren`, `luau`, `python3`, `pypy3`/`pypy`, `ruby` and `node`; missing optional runtimes are skipped, while a selected Aipo CLI/VM or Aipo→JavaScript path is a required failure when its binary/runtime is absent.
- Rust native is a control path, not a like-for-like dynamic-language comparison; its startup includes the `aipo-bench` harness.
- Reference source checkouts live outside the repository and are pinned in `/home/raillen/Documentos/Projetos/aipo-reference-runtime/SOURCES.json`; the benchmark still records the runtime resolved from `PATH`.
- `peak_rss_bytes` is sampled from Linux `/proc` during a separate metrics pass when `--compare-resources` is enabled. Allocation counters stay `null` until a runtime-specific adapter can report comparable semantics.
- MSRV 1.85 verified: `cargo check --workspace --all-targets` green on `1.85-x86_64-unknown-linux-gnu` (isolated target dir).
- `cargo +nightly` must resolve through the rustup shim (`~/.cargo/bin` first on PATH); the distro `/usr/bin/cargo` does not understand `+toolchain` and cargo-fuzz's inner `cargo` inherits PATH resolution.
