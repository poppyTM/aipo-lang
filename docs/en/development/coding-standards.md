# Coding and engineering standards (Rust)

Subordinate to the canon `Aipo — Rust Engineering Standard …` and to the workspace lints
(`Cargo.toml`: `unsafe_code = "forbid"`, clippy `all/correctness/suspicious/complexity/perf/style`
as `deny`).

1. **Formatting and style**:
   - All Rust code passes through `rustfmt` (`cargo fmt --all --check` in the gate).
   - Compiler and clippy warnings are errors (`-D warnings` in the gate).
2. **Typing and errors**:
   - Strict typing in every public signature (`Option`/`Result` explicit, no `unwrap` on
     user paths).
   - Aipo user errors are `Result`/`Diagnostic`/`VmError` — no `panic` escapes as a user error
     (guaranteed by fuzz + `catch_unwind` in hostile harnesses).
   - `unsafe` forbidden in the workspace; dependencies with `unsafe` require an approved
     exception line in `docs/security/security-contract.md`.
3. **Code documentation**:
   - Comments explain the *why*, never the *what*.
   - Public items have rustdoc (`missing_docs = warn`); crates have a `description`.
4. **Tests**:
   - Every behavior change ships with a fixture/regression (corpus, goldens or unit test);
     see `docs/development/testing-strategy.md`.
   - Deterministic tests (fixed seeds); nothing dependent on wall-clock as a gate; program
     output captured under a serialized lock (see `docs/testing/concurrency-audit.md`).
