# Aipo performance baseline

**Status:** record (generated artifact, not a gate)
**Scope:** wall-clock baselines for frontend stages, commands, VM workloads, the JS backend and the
separate cross-language suite
**Environment:** Linux x86_64 (4 CPU, 5 GiB RAM), rustc 1.98.1, Node v24.18.0, `--release`,
shared runner (noisy — see the MAD column)
**Recorded on:** 2026-09-20, `cargo run --release -p aipo-bench` (7 samples, median/MAD), git a8709881
**Re-measured on:** 2026-10-31, same machine and method, git ad7aaaa — see "2026-10-31 round" below

These numbers are baselines for future comparison on a dedicated runner, **not** pass/fail
thresholds. For the multi-runtime protocol, raw samples, checksums and fairness rules, see
`docs/performance/cross-language.md`.

## Frontend stages (median)

| stage | 858 B | 9 KiB | 99 KiB | 1023 KiB |
|---|---|---|---|---|
| bytecode/compile+verify | 34.12µs | 347.30µs | 3.91ms | 45.94ms |
| formatter/format | 50.16µs | 561.38µs | 9.55ms | 78.44ms |
| hir/lower | 24.52µs | 224.03µs | 4.18ms | 41.71ms |
| ir/lower | 19.97µs | 239.95µs | 4.27ms | 39.82ms |
| lexer/tokenize | 24.13µs | 275.36µs | 5.36ms | 58.56ms |
| sema/check | 14.65µs | 105.30µs | 2.43ms | 24.32ms |
| source/new | 1.90µs | 15.45µs | 179.48µs | 2.23ms |
| syntax/parse | 77.95µs | 861.32µs | 11.42ms | 102.74ms |

## VM commands and workloads (median)

| workload | input | median | MAD |
|---|---|---|---|
| cmd/check | 858 B | 188.04µs | 7.59µs |
| cmd/run | 858 B | 349.31µs | 6.43µs |
| cmd/build-emit | 858 B | 483.31µs | 4.75µs |
| cmd/fmt-check | 858 B | 94.61µs | 2.92µs |
| vm/int-arith | 43 B | 217.06µs | 1.62µs |
| vm/float-arith | 46 B | 206.48µs | 1.16µs |
| vm/call-overhead | 61 B | 223.84µs | 625ns |
| vm/recursion | 75 B | 13.79ms | 315.07µs |
| vm/closure-shared-var | 94 B | 274.68µs | 12.55µs |
| vm/global-lookup | 45 B | 218.07µs | 7.66µs |
| vm/list-build-iter | 74 B | 503.85µs | 33.07µs |
| vm/dict-insert-lookup | 78 B | 226.62µs | 2.02µs |
| vm/string-concat | 39 B | 477.75µs | 2.80µs |
| vm/interpolation | 43 B | 222.32µs | 2.16µs |
| vm/bytes-alloc | 49 B | 77.50µs | 611ns |
| vm/pipeline | 40 B | 142.71µs | 389ns |
| vm/failure-propagate | 112 B | 175.88µs | 180ns |
| vm/attempt | 68 B | 106.75µs | 262ns |
| vm/contracts | 71 B | 179.25µs | 1.07µs |
| vm/invariant-commit | 167 B | 212.84µs | 1.34µs |

## JS backend (frontend + emit + node spawn + execution)

| workload | input | median | MAD |
|---|---|---|---|
| js/emit | 858 B | 288.64µs | 13.93µs |
| js/emit | 9 KiB | 4.00ms | 178.16µs |
| js/emit | 99 KiB | 57.60ms | 4.30ms |
| js/emit | 1023 KiB | 576.11ms | 16.64ms |
| js/hello | 17 B | 57.64ms | 5.16ms |
| js/integrated | 858 B | 72.06ms | 6.68ms |

Node process spawn dominates the small workloads (~50–70 ms total). The separate
cross-language measurement distinguishes Aipo VM in-process, Aipo CLI/VM and
Aipo→JavaScript; steady-state comparisons still require a dedicated runner.

## Scaling (list construction/iteration, dict insertion, N=200..1600)

All ratios stay close to 2× per doubling on this runner — linear behavior; the
verdicts report the noise honestly. Raw ratios:

- `list-build: N=200..1600 ratios [1.73x, 2.17x, 1.66x] verdict=noisy — rerun on dedicated runner`
- `list-iterate: N=200..1600 ratios [1.88x, 2.07x, 1.86x] verdict=noisy — rerun on dedicated runner`
- `dict-insert: N=200..1600 ratios [1.75x, 1.87x, 2.02x] verdict=noisy — rerun on dedicated runner`

## Notable finding during this gauntlet

Parsing format-strings scaled 68× per 10× of input (measured 23.4 ms for 400 f-strings in
8.4 KiB). Root cause: `" ".repeat(offset)` padding per placeholder plus full rescans.
Fixed by making slice parsing padding-free and shifting spans back
(`crates/aipo-syntax/src/shift.rs`, ADP-005 addendum): 1.77 ms afterwards, cost proportional
to the offset eliminated, corpus snapshots byte-identical.

## 2026-10-31 round

Two hot-path fixes moved the hottest stages. Median at 99 KiB (7 samples), same noisy
machine:

| stage | before (2026-09-20) | now | factor |
|---|---:|---:|---:|
| lexer/tokenize | 5.36ms | 1.07ms | 5.0× |
| syntax/parse | 11.42ms | 4.12ms | 2.8× |
| js/emit | 42.9ms | 1.58ms | 27× |

What changed:

- **Parser** stopped cloning the whole `TokenKind` to read a payload and again to consume it;
  identifiers and literals now cost one allocation, not two. Sixty-one sites that only discarded
  the token now move just the cursor. At 1023 KiB parse dropped from 102.74ms to 47.95ms.
- **Lexer** decodes the cursor's code point once and keeps it beside the offset; `peek` became
  a field read instead of re-slicing and re-decoding UTF-8. At 1023 KiB tokenize dropped from
  58.56ms to 11.09ms.
- **JS emitter** writes the module JSON straight into the buffer instead of building a
  `serde_json::Value` tree per instruction (48 `json!` macros, one map and one `String` per
  field). At 1023 KiB it dropped from 78.44ms to 15.41ms; output is semantically identical
  (key order follows the wire, not the `BTreeMap`).

`ir/lower` (118ms at 1023 KiB) is work proportional to program size, not quadratic behavior:
measured at 21 KiB / 43 KiB / 88 KiB the cost is linear (0.00s / 0.01s / 0.02s in
`aipo check`).

### Where cost remains, and why

- **`syntax/parse` is the largest absolute stage** (47.95ms at 1 MiB, 21.9 MB/s). The loop now
  does real work: every AST node is allocated and every expression passes through the Pratt
  parser. Reducing further would require an arena of nodes or buffer reuse — a representation
  change, not a hot-path fix.
- **`vm/recursion`** (fib(20), 4.63ms) is the per-instruction cost of the bytecode interpreter:
  `step` does an iteration check, a metrics counter and an opcode `match` per instruction. `Value`
  is already 16 bytes and the frame call allocates nothing; the remaining limit is interpreted
  dispatch itself. Switching to computed dispatch or threading is a VM architecture decision that
  the performance doctrine asks to justify with data, not intuition.
