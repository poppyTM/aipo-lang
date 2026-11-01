# P08-G01 — Performance Matrix 2 baseline

**Date:** 2026-10-28 · **State:** DONE · **Measured base:** `c036164` (main after v0.1.0) ·
**Host:** AMD Ryzen 5 5600G, 12 CPUs, Linux x86_64 · **Pinning:** `taskset -c 0` ·
**Rust:** 1.99.1 · **Build:** release.

> **Central limit:** this is the best host available with an isolated CPU, not a dedicated
> runner. The numbers are a reproducible baseline with stated scope, not a regression gate.

## PERF-01 — Environment and protocol

- `aipo-bench --json` ran 58 samples in 7 rounds with CPU 0 pinned.
- `aipo-bench --compare --compare-quick --compare-runs 3` ran the cross-language workloads.
- Aipo binary: `target/release/aipo`; SHA-256 hash recorded in `compare.json`.
- Load average at start: about 1.16; CPU affinity was applied, but the host stayed shared.
- The runner itself classified scaling as `noisy`; no threshold was applied.

Artifacts: [matrix2.json](matrix2.json) and [compare.json](compare.json).

## PERF-02 — VM vs. RegVM end-to-end

Median times in milliseconds; processes include startup, parsing, compilation/planning and
execution. `aipo-reg` includes plan selection and canonical fallback when applicable.

| Workload | Aipo CLI/VM | Aipo VM in-process | Aipo RegVM | Reg/VM |
| --- | ---: | ---: | ---: | ---: |
| arithmetic | 102.68 | 99.46 | 97.62 | 0.95x |
| collections | 82.44 | 77.42 | 57.26 | 0.69x |
| fields | 254.42 | 254.45 | 259.41 | 1.02x |
| recursion | 36.96 | 33.76 | 57.19 | 1.55x |
| strings | 6.58 | 2.23 | 6.59 | 1.00x |

Reading: Reg won on `collections` and tied on `strings`/`arithmetic`; it lost on `recursion` and
`fields`. This result does not justify claiming a universal gain. Native Reg plan coverage stays
at 20/36 conformance programs.

## PERF-11 — Wasm

| Workload | Wasm JIT (ms) | VM CLI (ms) | Wasm/VM |
| --- | ---: | ---: | ---: |
| arithmetic | 16.74 | 102.68 | 0.16x |
| collections | 16.73 | 82.44 | 0.20x |
| fields | 16.68 | 254.42 | 0.07x |
| recursion | 16.75 | 36.96 | 0.45x |
| strings | **failed** | 6.58 | — |

`strings` fails with `AIPO_COMPILE_WASM_UNSUPPORTED` because `len` is outside the subset. Wasm
times sit near the startup/JIT floor (~16.7 ms); do not extrapolate to larger workloads.

## PERF-12 — Available cross-language

| Workload | Aipo VM | RegVM | Wasm | Aipo→JS | Python 3.12 | Node 18 | Native Rust |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| arithmetic | 102.68 | 97.62 | 16.74 | 358.05 | 31.87 | 108.99 | 6.24 |
| collections | 82.44 | 57.26 | 16.73 | 759.99 | 21.78 | 116.00 | 6.57 |
| fields | 254.42 | 259.41 | 16.68 | 619.01 | 62.23 | 110.20 | 6.60 |
| recursion | 36.96 | 57.19 | 16.75 | 236.00 | 16.71 | 110.06 | 6.40 |
| strings | 6.58 | 6.59 | — | 161.00 | 11.67 | 106.94 | 6.41 |

Runtimes unavailable on this host: Lua, LuaJIT, Luau, Wren, Ruby, PyPy and omniLua. They were
not installed in this round; the existing comparison with them remains historical.

## Reproduction

```sh
cargo build --release -p aipo-cli -p aipo-bench
taskset -c 0 target/release/aipo-bench --json docs/evidence/P08-G01/matrix2.json
taskset -c 0 target/release/aipo-bench --compare --compare-quick \\
  --aipo-bin target/release/aipo --compare-runs 3 \\
  --compare-json docs/evidence/P08-G01/compare.json \\
  --compare-languages aipo,aipo-vm,aipo-reg,aipo-wasm,aipo-js,python,javascript,rust
```

## What this baseline does not prove

- It is not a dedicated runner; it does not measure full RSS, allocation per runtime or
  multi-platform cold start.
- It is not a regression gate and does not replace the full performance matrix of Matrix 2.
- It does not certify Wasm/Reg parity; it only measures current behavior.
