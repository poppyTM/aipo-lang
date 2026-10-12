# Prumo — .

This is the intent router for humans and agents. Add links as stable documentation is created; do not create empty documentation solely to populate this map.

## Current state

- [Project state](../PROJECT_STATE.md)
- `prumo.json` — canonical project configuration

## I want to use the product

Add user tutorials, how-to guides, reference and explanations under `docs/user/` as needed.

## Revisão adicional de runtime e estudos

- [Uso, alterações e reimplementação](development/runtime-hardening-guide.md)
- [Estudos e fontes pinadas](studies/index.md)
- [Evidência e gates não executados da reconstrução](evidence/P07-G01-runtime-hardening.md)

P07-G01 permanece DRAFT; testes dispensados nesta rodada por pedido explícito do usuário. Publicação em PR não equivale à aprovação dos gates ou transição formal de fase.

## I want to develop/contribute

Add onboarding, codebase tour, build/test/debug and task-oriented development guides under `docs/developer/`.

## I want to operate/support it

Add deployment, configuration, observability, runbooks, backup/recovery, troubleshooting and release guidance under `docs/operations/` / `docs/support/` as needed.

## I am an AI agent

1. Read the active Goal.
2. Use the smallest sufficient context.
3. Prefer structural/symbol/document-section pointers.
4. Expand only when evidence is insufficient.
5. Keep output bounded.
6. Update only impacted canonical docs.
7. Record evidence/intelligence and garbage-collect temporary context.

## Architecture / decisions / specs

- [Crate contracts](crates/crate-contracts.md) — per-crate responsibility, dependency and invariants
- [Wave 1 plan](waves/wave-1-mvp.md) — MVP subset, vertical slices, exit gate (closed, see P00-G16)
- [Wave 2 plan](waves/wave-2-js-parity.md) — JS parity objective, slices, exit gate (W2-1 done, see P01-G01)
- [Language authority map](language/authority-map.md) — which document wins, and the no-invention/evidence policies
- [CLI reference](reference/cli.md) — stable `aipo run/check/build/fmt` surface, machine output and exit codes
- [MVP stdlib subset](stdlib/mvp-subset.md) — implemented Prelude V1, `List`/`Dict` methods and `math`/`string`/`io` surface
- [Conformance corpus](conformance/README.md) — fixture layout, snapshot matrix, regeneration and the gauntlet rubric
- [ADP-001 — Byte and core types as values](adp/ADP-001-byte-and-core-types-as-values.md) — resolved: Q1/Q2 by `P00-G10`, Q3/Q4/Q5 by `P00-G15`
- [ADP-002 — Construction hooks and runtime contracts](adp/ADP-002-construction-hooks-and-runtime-contracts.md) — resolved: G1/G2/G4 by `P00-G13`, G2b/G3 by `P00-G14` (interface structural conformance corrected and certified by `P00-G15`)
- [ADP-003 — Execution budgets](adp/ADP-003-execution-budgets.md) — draft: fuel/memory/interruption undecided
- [ADP-004 — Unicode identifier policy](adp/ADP-004-unicode-identifier-policy.md) — draft: confusables/NFC/NBSP undecided
- [ADP-005 — Parser recursion bounds](adp/ADP-005-parser-recursion-bounds.md) — accepted: 128/64/128 robustness bounds
- [ADP-006 — Wave 3 and Wave 4 open decisions](adp/ADP-006-wave3-wave4-open-decisions.md) — accepted
- [ADP-007 — Package identity and distribution](adp/ADP-007-package-identity-and-distribution.md) — accepted
- [ADP-008 — Release aipo v0.1.0 boundary](adp/ADP-008-v0.1.0-language-release.md) — accepted
- [ADP-009 — Synchronous C ABI](adp/ADP-009-synchronous-c-abi.md) — accepted
- [ADP-010 — Interoperability thin proofs](adp/ADP-010-interoperability-thin-proofs.md) — accepted
- [ADP-011 — Performance and ergonomics roadmap](adp/ADP-011-performance-and-ergonomics-roadmap.md) — accepted

Add further architecture, ADR/RFC and specifications as the project grows.

## Quality and testing

- [Testing strategy](development/testing-strategy.md) — validation levels, gates, fixtures
- [CI tiers](testing/ci-tiers.md) — what runs per-PR, scheduled, or on a dedicated runner
- [Gap matrix](testing/gauntlet-gap-matrix.md) — per-crate coverage inventory of the gauntlet
- [Tooling decisions](testing/tooling-decisions.md) — adopted/rejected/deferred tools
- [Concurrency audit](testing/concurrency-audit.md) — global test state discipline
- [Diagnostic accessibility rubric](testing/diagnostic-accessibility-rubric.md) — text-interface requirements
- [Performance baseline](performance/baseline.md) — wall-clock medians per workload
- [Cross-language benchmarks](performance/cross-language.md) — checksums, raw samples and runtime comparison protocol

## Evidence

Gate evidence records live under `docs/evidence/`, one per completed goal:

- [P00-G09 — Runtime & minimum stdlib](evidence/P00-G09-slice-s9.md)
- [P00-G10 — Backend completion](evidence/P00-G10-backend-completion.md)
- [P00-G11 — CLI and formatter](evidence/P00-G11-cli-and-formatter.md)
- [P00-G12 — Conformance and MVP gate](evidence/P00-G12-conformance-and-mvp-gate.md)
- [P00-G13 — Construction hooks and Bytes](evidence/P00-G13-construction-hooks-and-bytes.md)
- [P00-G14 — Runtime contract enforcement](evidence/P00-G14-runtime-contract-enforcement.md)
- [P00-G15 — Static contracts and interface conformance](evidence/P00-G15-static-contracts-and-interface-conformance.md)
- [P00-G16 — Wave 1 exit review](evidence/P00-G16-wave-1-exit-review.md)
- [P01-G01 — JS parity for the full MVP subset](evidence/P01-G01-js-parity-mvp-subset.md)
- [P01-G02 — Deep quality gauntlet](evidence/P01-G02-deep-quality-gauntlet.md)
- [P02-G01 — Wave 3 tipos e valores](evidence/P02-G01-wave3-types-and-values.md)
- [P02-G02 — Wave 3 combinadores assíncronos e scheduler](evidence/P02-G02-wave3-stdlib-async.md)
- [P02-G03 — Wave 3 infra assíncrona](evidence/P02-G03-wave3-async-syntax-and-diagnostics.md)
- [P03-G01 — Host ABI: capability model, host values and generational handles](evidence/P03-G01-host-abi.md)
- [P03-G02 — Poppy adapter and deterministic headless demo](evidence/P03-G02-poppy-adapter-and-headless-demo.md)

## Goals

Goals live under `.ai/goals/<phase>/` and define measurable completion.

## Durable intelligence

Compact project/task intelligence lives in `.prumo/history/project-intelligence.json`.
