# Toolchain observability

There is no service, no endpoints and no distributed traces: Aipo's observability is the
diagnostics contract itself plus versioned measurement artifacts.

1. **Structured diagnostics:** every error goes to stderr as `file:line:column` text or as
   `--message-format=jsonl` (stable schema in `docs/diagnostics/catalog.md`); the program never
   mixes stdout with errors.
2. **Contractual exit codes:** `0` success, `1` language failure, `2` usage error (see
   `docs/reference/cli.md`).
3. **Performance baselines:** `docs/performance/baseline.md` (median/MAD per workload) +
   `docs/performance/cross-language.md` (samples, checksums and runtimes) + the `aipo-bench`
   JSON for comparison.
4. **Coverage:** LLVM report via `docs/testing/coverage.sh` (navigation, not a meta).
5. **Evidence per goal:** `docs/evidence/` records what was proven, with commands.
