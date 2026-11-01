# Aipo — Wave 2 plan: JavaScript parity

**Status:** normative (process level) — slice W2-1 DONE, see
`docs/evidence/P01-G01-js-parity-mvp-subset.md`; remaining slices open
**Authority:** implements `docs/canon/Aipo — Waves, Vertical Slices, Gauntlet Loops …md` (Wave 2) and
`docs/canon/Aipo — Fechamento Arquitetural 10 10…` (§10)
**Scope:** Wave 2 objective, slices, exit gate
**Update triggers:** slice decomposition change, gate criteria change

## Objective

Ship a second backend that preserves Aipo semantics: `.aipo` programs compile to ESM JavaScript
(`app.js + aipo-runtime.js + app.js.map`) via `aipo build`, with a VM↔JS differential suite
proving parity over the frozen Wave 1 corpus. JavaScript is a target; its internal differences
(UTF-16, `Number`, exceptions) must not leak into Aipo programs.

## Architecture (canon)

```
Aipo Source
    ↓
Semantic / Core IR (aipo-ir, target-neutral)
    ↓
JavaScript Emitter (aipo-js, NEW — deps: aipo-ir, aipo-diagnostics only)
    ↓
app.js + aipo-runtime.js + app.js.map (ESM, ECMA-426 source maps)
```

Rules:

- `aipo-js` depends on `aipo-ir` + `aipo-diagnostics` only. Bytecode coupling is forbidden.
- Output is ESM. Source maps are mandatory.
- Runtime shim is small, versioned and tested; it contains only unavoidable semantic deltas
  (code-point strings, `Int`/`Byte` range checks, `Failure`/`or_else` lowering, `Bytes` via
  `Uint8Array`/`DataView`, identity/equality helpers).
- `Int` range ±(2^53−1), `String` NFC, ordered `Dict`, `copy`/`same` identity and
  `Failure`/fault split follow Aipo, never JS accidentals.
- Recoverable `Failure` lowers to a distinguishable runtime representation; `attempt` captures
  only Aipo `Failure`, never arbitrary JS exceptions. Runtime faults stay faults.
- Divergence between VM and JS backends is a bug until proven otherwise.

## Parity subset (frozen oracle)

The Wave 1 corpus is the referee, frozen by `docs/evidence/P00-G16-wave-1-exit-review.md`:

- `docs/conformance/programs/01`–`20` + `.stdout` (including `19_local_functions`,
  `20_module_scope`);
- `docs/conformance/diagnostics/01`–`19` (diagnostics stay frontend: `build` must fail with the
  same codes, never emit JS);
- `docs/conformance/modules/{basic,cycle,missing}` (resolution happens before IR, so the JS
  bundle is single-module; cycle/missing must fail at build time).

`Bytes` packing APIs, `Set`/`Sequence`, LSP/REPL, async, host ABI/Poppy and packages remain
explicit non-delivery (see P00-G16 record). No language surface change in Wave 2.

## Wave 2 slices

| # | Slice | Deliverable |
|---|---|---|
| W2-1 (P01-G01) | JS parity for the full MVP subset | `aipo-js` emitter + versioned shim + source maps + `aipo build` + differential suite green over `programs/01`–`20` |
| W2-2 | Diagnostics parity at build time | every `diagnostics/*` fixture fails `aipo build` with its committed code; no JS emitted on failure |
| W2-3 | Module bundling + stdlib closure | `modules/*` cases bundled to single-file JS; stdlib version pinned and tested |

This plan ships W2-1 first; W2-2/W2-3 are acceptance gates of the same goal, not separate goals,
unless the differential suite forces a split.

## MVP exit gate for W2-1 (evidence required)

- `aipo build <entry.aipo> --out <dir>` emits `app.js`, `aipo-runtime.js`, `app.js.map` (ESM,
  valid ECMA-426 map, `sources: [<entry>.aipo]` minimum).
- Differential suite: for each `programs/NN_*`, VM stdout == JS (`node`) stdout == committed
  `.stdout`. Command: `cargo test -p aipo-js --test differential` (or the harness that owns it).
- `node --version` recorded in the evidence record (suite requires Node ≥ 20; CI uses the
  pinned LTS).
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, `cargo doc --workspace --no-deps` all green.
- `prumo validate` and `prumo doctor` green; docs delta resolved; evidence recorded here.
