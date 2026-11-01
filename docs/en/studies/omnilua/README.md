# Aipo-lang × omniLua — studies and technical audit

This directory holds the operational and technical documentation of the audit, reuse and evolution of
**Aipo-lang** guided by the **omniLua** reference (per the technical directive
`AIPO_OMNILUA_DIRETRIZ_TECNICA_CODE_AGENTS.md`).

## 1. Fundamental principle

> **Aipo is not a Lua dialect.** No semantics, grammar or philosophy of Aipo may be changed to
> accommodate Lua conveniences. omniLua is used strictly as a **source of runtime-implementation,
> sandboxing, allocation and oracle techniques in Pure Rust (`unsafe_code = "forbid"`)**, subject
> to rigorous evidence and parity tests.

## 2. Pinned reference commits

- **Aipo-lang:** `58f8bc15de13a5eea5834a18f62a6e74bae73349` (branch: `feat/host-ahs-headless-conformance`)
- **omniLua:** `90dcf85a8cfb95305b77056af00a114fd0df30a1` (local: `~/aipo-refs/omnilua`,
  origin: [ianm199/omnilua](https://github.com/ianm199/omnilua))
- **Licenses:**
  - Aipo: MIT License, Copyright (c) 2026 Aipo Contributors.
  - omniLua: MIT License, Copyright (c) 2026 lua-rs-port contributors (derivative work of
    Lua 5.4.7 PUC-Rio, keeping the corresponding NOTICE).

## 3. Phase 0 artifact structure

| File | Description |
|---|---|
| [`AIPO_CURRENT_STATE.md`](AIPO_CURRENT_STATE.md) | Proven inventory of Aipo's subsystems (what exists, what is partial, gaps). |
| [`OMNILUA_SOURCE_MAP.md`](OMNILUA_SOURCE_MAP.md) | Mapping of omniLua crates, files and mechanisms relevant to Aipo. |
| [`BASELINE.md`](BASELINE.md) | Frozen baseline of tests, compilation and metrics of Aipo. |
| [`SEMANTIC_COMPATIBILITY.md`](SEMANTIC_COMPATIBILITY.md) | Strict compatibility matrix between the Aipo model and the Lua/omniLua model. |
| [`OPPORTUNITIES.md`](OPPORTUNITIES.md) | Opportunity matrix categorized as `COPY / ADAPT / INSPIRE / DEFER / REJECT`. |
| [`RISKS.md`](RISKS.md) | Threat, regression-risk and architectural-safeguard mapping. |

## 4. Next phases

- **Phase 1:** validation training and oracle/canary infrastructure.
- **Phase 2:** instrumentation and boundary hardening (leak canaries and allocation counters).
- **Phase 3:** sandboxing pilot (shared uncatchable instruction budget + sampled memory ceiling).
- **Phase 4:** scoped embedding pilot (host loans with explicit lifetime).
