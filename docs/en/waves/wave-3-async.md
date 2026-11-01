# Wave 3 — async: Task, scheduler, await

**Status:** closed — exit gate met, all three slices DONE (`P02-G01`, `P02-G02`,
`P02-G03`); the plan below is the certified record, not open work
**Authority:** canon Waves model (Wave 3), Fechamento §6, Stdlib canon (Task/Sequence/Time),
Poppy Pivot (async direction + await-do)
**Scope:** value layer, stdlib async surface, scheduler + `async`/`await`/`await do`
**Slices:** P02-G01 (types/values) → P02-G02 (stdlib) → P02-G03 (async infra)
**Evidence:** `docs/evidence/P02-G01-wave3-types-and-values.md`,
`docs/evidence/P02-G02-wave3-stdlib-async.md`,
`docs/evidence/P02-G03-wave3-async-syntax-and-diagnostics.md`

## Objective

Ship cooperative, deterministic async: `Task` values, a single-threaded scheduler with virtual
time, `async fn`/`await`/`await do`, structured combinators
(`all`/`race`/`timeout`/`cancel`/`group`/`spawn`), cancellation as fault, with VM↔JS
differential parity on ordering and cancellation.

## Non-negotiable rules (from canon)

- No implicit parallelism; FIFO run queue; depth-first await driving.
- `await do` is sugar for sequential awaits (never in subexpressions, never into nested
  fn/lambda/callback bodies, nested block form warns).
- `attempt` captures recoverable Failure only — never cancellation.
- Forgotten tasks are a static diagnostic.
- Divergence VM↔JS is a bug.

## Exit gate (evidence required)

| Criterion | Certified by |
|---|---|
| Corpus programs exercising tasks/sleep/all/race/timeout/cancel/group/await-do green on VM and Node with identical stdout, codes and exit status | `programs/24_async_functions_and_await`, `programs/25_await_do`, `programs/26_async_combinators`, `programs/27_numeric_literal_bases`; `crates/aipo-js/tests/differential.rs` (`test_vm_and_js_agree_with_committed_stdout` plus the five Wave 3 async cases) |
| Cancellation and await-cycle fixtures fault with committed codes | `diagnostics/26_runtime_uncaught_failure_through_await`, `diagnostics/27_runtime_await_cycle`, `diagnostics/28_runtime_await_cancelled`; `AIPO_RT_AWAIT_IN_CALLBACK` covered by the `P02-G02` suite |
| `cargo fmt/clippy/test/doc` green | `cargo test --workspace --all-targets`: 301 passed, 0 failed; fmt/clippy `-D warnings`/doc all clean — see the three evidence records |
| No new `unsafe` | workspace `unsafe_code = "forbid"` compiles, so the property is enforced by the compiler |
| MSRV unchanged | `rust-version = "1.85"` in the workspace manifest, untouched |

## Follow-ups recorded, not hidden

- `Task[T]` parametric contracts stay out of V1 with a dedicated diagnostic (ADP-006 §G);
  `Date`/`TimeOfDay`/`DateTime` wait for the Wave 6 `timezone` package (ADP-006 §D).
