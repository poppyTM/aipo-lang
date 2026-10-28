# Aipo — Testing Strategy

**Status:** normative
**Scope:** validation levels, gates, fixtures, evidence expectations for the Aipo implementation
**Blocking question:** which checks block completion?

## Test levels

1. **Unit** — per-crate domain logic (lexer classification, scope rules, bytecode verifier).
2. **Snapshot/golden** — tokens, syntax, HIR, IR, bytecode disassembly, formatter output,
   diagnostics. Committed under each crate's `tests/snapshots/` or `docs/conformance/`.
3. **Property** — lossless round-trip, formatter idempotency, lexer span slicing,
   parser never-panic. Deterministic (`aipo-testkit::Rng` + printed seeds).
4. **Integration** — end-to-end: `.aipo` program → `aipo run` → expected stdout/exit;
   `aipo check` → expected diagnostics.
5. **Fuzz** — pipeline smoke fuzz over arbitrary bytes; parser/lexer fuzz targets.
   Every crash finding becomes a regression fixture.
6. **Differential (Wave 2)** — VM ↔ JavaScript on the proven subset, extended to
   generated programs (AipoSmith) and metamorphic rewrites.
7. **UI/diagnostic** — golden human + JSONL renderings, schema invariants,
   no-color discipline, cascade control (`crates/aipo-cli/tests/diagnostic_ui.rs`).
8. **Resource/security** — hostile bytecode, resource exhaustion with termination
   contract, portable subprocess watchdog (no platform-only `timeout`).
9. **Benchmarks** — `aipo-bench` baselines (median/MAD), scaling analysis and cross-language samples/checksums; compilation-gated in PRs, executed on a dedicated runner.
10. **Coverage-guided fuzz (nightly tier)** — `fuzz/` libFuzzer targets with
    committed seed corpora; see `docs/testing/ci-tiers.md`.

## Quality gates (blocking)

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --locked
prumo validate . && prumo doctor .
```

On Linux, the full workspace gate requires `libasound2-dev` for `aipo-game-host`.
Without it, use the default-members gate (`cargo test --locked`) and run Poppy/eGUI
separately. `prumo` gates require the Prumo CLI to be installed.

Risk-gated (when relevant code changes): `cargo miri test` (no Wave 1 unsafe expected),
fuzz targets, `cargo audit`/`cargo deny`.

## Cross-cutting rules

- **Sink discipline:** program output flows through the process-global stdlib
  sink. Any test capturing it must hold a serializing lock for the whole test
  (see `docs/testing/concurrency-audit.md`); locks are per test file and never
  re-entered.
- **Determinism:** fixed seeds everywhere; a failure prints its seed and the
  exact commands to reproduce.
- **Tiers:** `docs/testing/ci-tiers.md` decides what runs per-PR vs scheduled
  vs dedicated-runner. Wall-clock timing never gates shared runners.

## Fixtures

- `docs/conformance/programs/NN_name.aipo` + `NN_name.stdout` — must run and match stdout.
- `docs/conformance/diagnostics/NN_name.aipo` + `NN_name.code` — must fail with the listed code(s).
- `docs/conformance/formatting/NN_name.input.aipo` + `NN_name.expected.aipo` — formatter golden corpus.
- `docs/conformance/modules/<case>/entry.aipo` — multi-file module cases with optional `.stdout`/`.code`.
- `docs/conformance/ahs/` and `docs/conformance/host/` — host schema and headless capability fixtures.
- The corpus is the referee; expected diagnostics live next to their fixture, not in a separate tree.

## Evidence expectations

| Claim | Evidence |
|---|---|
| "MVP runs" | integration test + fixture program output |
| "diagnostic correct" | fixture with expected code/span |
| "formatter stable" | idempotency test |
| "no panics leak" | fuzz smoke + panic-free test suite |
| "wave complete" | gate checklist in `docs/waves/` with links to evidence |
