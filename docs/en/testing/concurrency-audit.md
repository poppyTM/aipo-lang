# Test concurrency audit (P01-G02)

**Status:** record (process level)
**Scope:** process-global mutable state in test infrastructure

## Inventory

| Global | Owner | Guard |
|---|---|---|
| `aipo_stdlib::io::OUTPUT_SINK` (program output) | `aipo-stdlib` (`Arc<Mutex<Option<…>>>`) | Per-file serializing lock (`OnceLock<Mutex<()>>`) in every test file that captures program output |
| `aipo_testkit::pipeline` sink use | testkit consumers | Caller-held lock (documented on `run_capture`) |
| Contract: `check` never touches the sink; only `run` does | CLI + stdlib | Verified by inspection |

## Findings

- **No race exists in the current design.** Each integration test file compiles to its own
  process; inside a process, exactly one test holds the sink lock at a time. Cross-test
  contamination (the historical `07_strings_and_math` snapshot pollution) was fixed by
  serializing *every* CLI invocation, and the pattern is now replicated in each new harness
  (`examples`, `diagnostic_ui`, `fmt_equiv`, `determinism`, `unicode_security`).
- **Cost:** serialization lengthens wall time for sink-capturing suites (conformance,
  examples). Accepted: correctness over speed.
- **No refactor undertaken.** A shared fixture (e.g. a `testkit::sink_lock`) would be
  aesthetic only; per-file locks are explicit and grep-able.
- **cargo-nextest evaluated → deferred:** nextest parallelizes test binaries across processes,
  which is already what `cargo test` does here, and it cannot remove the in-process sink lock
  (the actual constraint). Revisit when real CI needs flaky-test detection (`--rerun`) or
  sharding.

## Residual risk

A future test that captures program output without taking a lock will contaminate silently.
Mitigation: the CONTRIBUTING quality section now names the lock discipline (see
testing-strategy update in this goal).
