# Aipo — Conformance Corpus (slice S11)

**Status:** normative (process level)
**Authority:** implements `docs/waves/wave-1-mvp.md` (S11) and `docs/reference/cli.md`
**Scope:** corpus layout, snapshot matrix, regeneration, gate rubric
**Update Triggers:** new fixture kind, snapshot format change, gate criteria change

## Delta de reconstrução — P07-G01

As fixtures `programs/33_dict_primary_iteration` e `programs/34_conditional_values` registram, respectivamente, chaves de Dict em iteração primária e resultados condicionais em ramos que convergem. Seus `.stdout` são expectativas baseadas no canon e revisão manual; não foram regenerados nem executados nesta reconstrução.

`crates/aipo-cli/tests/reg_conformance.rs` compara essas fixtures nos engines vm/reg. `crates/aipo-vm/tests/reg_vm_robustness.rs` cobre aritmética compartilhada, operandos inválidos, budget, Unicode/extremos e índice de iteração em registrador alto. `disabled_features.rs` cobre ausência de compiler/runner. As suites JIT Wasm agora respeitam features. Execução dispensada explicitamente pelo usuário nesta rodada; paridade ainda precisa ser validada.

Consulte [guia e limites](../development/runtime-hardening-guide.md) e [evidência](../evidence/P07-G01-runtime-hardening.md). Este delta não reclassifica como certificados os cenários pendentes.

## Purpose

The corpus is the shared, executable definition of the delivered language surface — Wave 1
MVP subset plus the Wave 2 JS-parity and Wave 3 async/collection deliveries certified below.
It is the artifact that the CLI, the formatter and the language agree on: everything the
language claims is either run here with a committed output, or rejected here with a committed
diagnostic code.

Nothing in this directory is generated at build time except the `.stdout` snapshots, and the
snapshot for a program is only ever rewritten on request (see *Regeneration*).

## Layout

| Directory | Kind | Pairing | Consumed by |
|---|---|---|---|
| `programs/` | runnable programs | `NN_name.aipo` + `NN_name.stdout` | `aipo-cli` conformance suite |
| `diagnostics/` | programs that must fail | `NN_name.aipo` + `NN_name.code` | `aipo-cli` conformance suite |
| `formatting/` | formatter golden corpus | `NN_name.input.aipo` + `NN_name.expected.aipo` | `aipo-formatter` golden suite and `aipo fmt --check` |
| `modules/` | multi-file module cases | `modules/<case>/{entry.aipo, *.aipo}` + `entry.stdout` / `entry.code` | `aipo-cli` conformance suite |

### Fixture invariants

- **`programs/*.stdout`** is the exact standard output of `aipo run`. A program whose output
  changes must have its snapshot reviewed, never silently rewritten.
- **`diagnostics/*.code`** lists one diagnostic code per line. Every listed code must appear
  in stderr, so a fixture that starts failing for the *wrong* reason fails the suite too.
- **`formatting/*.expected.aipo`** is both the expected output of the formatter and a
  canonical source: `fmt --check` on it must be a no-op, and the formatter must be idempotent
  on it.
- **`modules/<case>/`** holds one `entry.aipo` plus the modules it imports. An entry is a
  failure fixture when a sibling `<entry>.code` exists, and a passing fixture otherwise; the
  other `.aipo` files in the case are import targets, not entry points.

## Snapshot matrix

| Program fixture | Covers |
|---|---|
| `01_hello` | literals, `String()` conversion, `io.println` |
| `02_recursion` | recursion, `return`, base cases |
| `03_control_flow` | `if`/`elif`/`else`, `match`/`when`, `loop`, `while`, `repeat`, `each`, `break`, `continue` |
| `04_collections` | `List`/`Dict` literals, core collection method APIs, indexing |
| `05_structs_and_impl` | `struct` with defaults, construction, `impl` methods, associated functions |
| `06_closures` | anonymous `fn`, capture, per-iteration capture in loops |
| `07_strings_and_math` | `string` and `math` modules, `find`/`replace`/`split`/`join`, rounding modes |
| `08_failures` | `fail`, `or_else`, `attempt`/`failed`, `err.message` |
| `09_slicing` | ranges `a..b`, list/string slicing with negative bounds |
| `10_integrated` | several features combined in one program |
| `11_defaults_and_named_args` | default parameters evaluated per call, defaults referencing earlier parameters, named arguments in any order, pipeline into a call |
| `12_bytes` | `Bytes(count)` construction, byte indexing, `len`, slicing |
| `13_init_and_invariant` | `Type{...}` runs `init` when declared (with defaults and `fixed` assignment), `invariant()` verified at the end of construction |
| `14_signature_contracts` | `name: Type`, `name!: Type`, `-> T`, `T?`, checked default, the `Function` contract, a `struct` contract and a mutable receiver |
| `15_invariant_on_mutation` | `invariant()` at stable mutable boundaries: commit, rollback with the entry value preserved (method and direct assignment) and rollback of two instances in one operation |
| `16_interface_contracts` | `satisfy` plus an interface used as a written contract: structural conformance at runtime, `T?` accepting `none`, an interface operation with an argument |
| `17_unicode_nfc` | NFC at the construction boundaries: escaped literal, concatenation, interpolation, `join`, `replace`, `format`, case mapping, `reverse`, and a mark with no precomposed form surviving normalization |
| `18_tolerant_slices_and_clamp` | tolerant slices (above the end, before the start, inverted) for `List`, `String` and `Bytes`; `math.clamp` with inverted bounds as a recoverable `Failure`; mixed numeric promotion in `min`/`max` |
| `19_local_functions` | local `fn` declarations: shared `var` capture (canon's `create_counter`), self-recursion through the local name, and a local `fn` reading an enclosing parameter |
| `20_module_scope` | module-scope visibility: a top-level `var` read and mutated by a module function (canon's `var counter` + `fn bump` pattern) and a top-level `let` read from one |
| `21_attempt_recovery_and_journal` | `return fail(…)` propagating to the caller's `attempt` boundary, and recovery releasing pre-handler journal entries (only post-handler mutations roll back) |
| `22_signed_zero` | IEEE signed zero: `-0.0` observable in `Float` display, `Int` arithmetic never producing negative zero (normalized on the JS backend) |
| `23_init_in_parameterized_fn` | construction with `init` inside a parameterized function (fuzz-found IR builder panic regression) |
| `24_async_functions_and_await` | `async fn` as a call protocol (calling spawns eagerly, `await` explicit everywhere), task values bound/passed/joined, `task.spawn`, `await` as statement/initializer/return, anonymous `async fn` closure and an `async fn` `impl` method with the receiver as argument 0 |
| `25_await_do` | `await do … end` sequential sugar over several tasks and a local `fn` that awaits inside the block |
| `26_async_combinators` | `task.all`/`race`/`timeout`/`cancel`/`sleep`/`group` composed in one program with deterministic ordering |
| `27_numeric_literal_bases` | decimal/hex/binary/octal integer literals, separators, float literal forms and `i64`-range boundaries |

| Diagnostic fixture | Code asserted |
|---|---|
| `01_parse_missing_brace` | `AIPO_PARSE_UNEXPECTED_TOKEN` |
| `02_parse_unexpected_token` | `AIPO_PARSE_UNEXPECTED_TOKEN` |
| `03_lexer_unterminated_string` | `AIPO_LEX_UNTERMINATED_STRING`, `AIPO_PARSE_UNEXPECTED_TOKEN` |
| `04_sem_unknown_name` | `AIPO_SEM_UNKNOWN_NAME` |
| `05_sem_redeclared_in_scope` | `AIPO_SEM_REDECLARED_IN_SCOPE` |
| `06_runtime_type_mismatch` | `AIPO_RT_TYPE_MISMATCH` |
| `07_runtime_index_out_of_range` | `AIPO_RT_INDEX_OUT_OF_RANGE` |
| `08_runtime_mutation_during_iteration` | `AIPO_RT_MUTATION_DURING_ITERATION` |
| `09_runtime_div_zero` | `AIPO_RT_DIV_ZERO` |
| `10_runtime_failure_uncaught` | `AIPO_RT_FAILURE_UNCAUGHT` |
| `11_runtime_invariant_violation` | `AIPO_RT_TYPE_MISMATCH` |
| `12_runtime_contract_violation` | `AIPO_RT_TYPE_MISMATCH` (parameter contract fault) |
| `13_runtime_return_contract` | `AIPO_RT_TYPE_MISMATCH` (return contract fault) |
| `14_runtime_invariant_mutation_uncaught` | `AIPO_RT_FAILURE_UNCAUGHT` |
| `15_sem_contract_violation` | `AIPO_SEM_CONTRACT_VIOLATION_STATIC` (literal argument against a written parameter contract) |
| `16_sem_return_contract` | `AIPO_SEM_CONTRACT_VIOLATION_STATIC` (literal returned against `-> T`) |
| `17_runtime_interface_contract` | `AIPO_RT_TYPE_MISMATCH` (a value without the operation an interface declares) |
| `18_runtime_interface_arity` | `AIPO_RT_TYPE_MISMATCH` (a same-named operation of the wrong caller-visible arity) |
| `19_runtime_clamp_inverted_bounds` | `AIPO_RT_FAILURE_UNCAUGHT` (unhandled inverted `clamp` bounds) |
| `20_lex_invalid_number` | `AIPO_LEX_INVALID_NUMBER` (malformed numeric literal) |
| `21_runtime_overflow_literal` | `AIPO_RT_OVERFLOW` (literal above the safe-integer range) |
| `22_runtime_overflow_unrepresentable` | `AIPO_RT_OVERFLOW` (numeric value with no representable `Int`) |
| `23_sem_await_in_subexpression` | `AIPO_SEM_AWAIT_IN_SUBEXPRESSION` |
| `24_sem_forgotten_task` | `AIPO_SEM_FORGOTTEN_TASK` |
| `25_sem_nested_await_do` | `AIPO_SEM_NESTED_AWAIT_DO` |
| `26_runtime_uncaught_failure_through_await` | `AIPO_RT_FAILURE_UNCAUGHT` (failure surfacing out of an awaited task) |
| `27_runtime_await_cycle` | `AIPO_RT_AWAIT_CYCLE` |
| `28_runtime_await_cancelled` | `AIPO_RT_CANCELLED` |
| `29_sem_await_literal` | `AIPO_SEM_CONTRACT_VIOLATION_STATIC` (await operand provably not a `Task`) |

| Module case | Covers |
|---|---|
| `modules/basic` | selective import (`entry`), namespace alias (`alias`), init-once on repeated import (`init_once`), private-name rejection both by name (`private_violation`) and through the namespace (`alias_private_violation`) |
| `modules/cycle` | cyclic imports are rejected (`AIPO_SEM_IMPORT_CYCLE`) |
| `modules/missing` | importing a module with no backing file (`AIPO_SEM_UNKNOWN_MODULE`) |

| Formatting fixture | Covers |
|---|---|
| `01_missing_indent` | indentation is canonicalized to four spaces |
| `02_operator_spacing` | binary operator spacing |
| `03_nested_blocks` | nested block indentation and `end` alignment |
| `04_match_branches` | `when`/`else` arm indentation |
| `05_comments_and_blanks` | comment and blank-line preservation |
| `06_struct_and_impl` | `struct`/`impl` bodies |
| `07_slices_and_calls` | call and slice argument layout |
| `08_trailing_block` | trailing `do … end` blocks |

### Host surface fixtures (`ahs/`)

`ahs/headless_test_host.json` is an Aipo Host Schema describing a `demo` module the language and
the standard library know nothing about. The `.aipo` files beside it are exercised by
`cargo test -p aipo-cli --test host_surface`, with and without `--ahs=<that file>`;
the expected code is in each file's header comment. The suite also exercises `run`,
Wasm semantic checking, JSONL diagnostics and the explicit Rust embedding surface.

| Fixture | Asserts |
|---|---|
| `host_surface_probe` | resolves with the surface; `AIPO_SEM_UNKNOWN_NAME` without it |
| `host_unknown_member` | `AIPO_SEM_UNKNOWN_NAME` for `demo.nonexistent` |
| `host_arity_mismatch` | `AIPO_SEM_ARITY_MISMATCH` for `demo.ping()` |
| `host_optional_param` | an `optional` parameter may be omitted, positional or named |
| `host_contract_violation` | `AIPO_SEM_CONTRACT_VIOLATION_STATIC` for a literal against `Int` |

The suite includes regressions for unknown/duplicate named arguments, required arguments,
nullable contracts, dynamic values left to runtime, local module shadowing, preserved struct
method checks, rejected flag combinations and isolation between CLI/embedding compilations.

These are not part of the `programs/`/`diagnostics/` suite because they need a flag.

### Runtime host fixtures (`host/`)

`headless_test_host.json` is embedded by the opt-in `--host=headless-test` profile.
Unlike `--ahs`, the profile registers a VM-local implementation without granting capabilities.
`cargo test -p aipo-cli --test host_runtime` drives the real executable in human and JSONL
modes; `check` accepts each runtime-fault fixture without executing it.

| Fixture | Pair | Asserts |
|---|---|---|
| `capability_denied.aipo` | `.code` | Clock denied (`AIPO_RT_CAPABILITY_DENIED`). |
| `stale_handle.aipo` | `.code` | Old generation remains stale after slot reuse (`AIPO_RT_STALE_HANDLE`). |
| `scope_escape.aipo` | `.code` | A closed native scope cannot publish its handle (`AIPO_RT_SCOPE_ESCAPE`). |
| `live_handle.aipo` | `.stdout` | Live handles read copied integers and release safely; repeated runs agree. |

Additional runtime tests cover both clocks, all six publication sites, uncatchable faults,
dynamic boundary contracts, strict flag combinations, and isolation from ordinary CLI and
embedding compilations. These fixtures are deliberately separate from the unprofiled corpus.

## Regeneration

Snapshot files are written by the suite when the environment asks for it:

```bash
AIPO_UPDATE_SNAPSHOTS=1 cargo test -p aipo-cli --test conformance
```

This rewrites `programs/*.stdout` and `modules/**/*.stdout`. Everything else — diagnostic
codes, formatter expectations, module fixtures — is authored by hand, because those files are
specifications rather than observations. A diff in a regenerated snapshot is a language change
and must be reviewed as one.

## Gauntlet rubric

The S11 rubric scores the MVP on five gates. Each gate has one owner command and one piece of
evidence, so a score cannot be claimed without a reproducible command.

| Gate | Weight | Command | Pass condition |
|---|---|---|---|
| Executable MVP | 30% | `cargo test -p aipo-cli --test conformance` | all program and module fixtures run green against committed snapshots |
| Diagnostic discipline | 20% | same suite, failure tests | every fixture fails with its committed code, and no fixture fails for an unrelated reason |
| Determinism | 20% | `cargo test -p aipo-formatter` + `fmt --check` corpus tests | formatter is idempotent; canonical sources report no drift; `fmt --check` never rewrites |
| Robustness | 15% | `cargo test -p aipo-cli --test fuzz_smoke` | no input makes the pipeline panic; malformed input yields a diagnostic and exit code 1 or 2 |
| Repository gates | 15% | `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test --workspace`, `cargo doc` | all four green |

### Gauntlet score — `P00-G16` hand-off (100%)

Scored 2026-09-19 from the repository root (rustc/cargo 1.98.1). Full hand-off in
`docs/evidence/P00-G16-wave-1-exit-review.md`.

| Gate | Weight | Exact command | Score |
|---|---|---|---|
| Executable MVP | 30% | `cargo test -p aipo-cli --test conformance` | **pass** — 13/13 |
| Diagnostic discipline | 20% | same suite, failure tests (`test_failing_fixtures_report_expected_diagnostic_codes`, `test_module_failures_report_expected_diagnostic_codes`) | **pass** |
| Determinism | 20% | `cargo test -p aipo-formatter` | **pass** — 6 + 4 + 1 = 11/11 |
| Robustness | 15% | `cargo test -p aipo-cli --test fuzz_smoke` | **pass** — 3/3 |
| Repository gates | 15% | `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (146/146), `cargo doc --workspace --no-deps` | **pass** |

Inventory verified on disk: `programs/` 23 `.aipo` + 23 `.stdout`, `diagnostics/` 19 `.aipo` +
19 `.code`, `formatting/` 8 `.input.aipo` + 8 `.expected.aipo`, `modules/` 3 entry points
(`basic`, `cycle`, `missing`).

## Verified gaps

The corpus records what works. What the MVP subset lists but the implementation does not yet
deliver is tracked in `docs/adp/` (no-invention policy).

The five gaps the S11 corpus found are recorded in
`docs/adp/ADP-002-construction-hooks-and-runtime-contracts.md`, and all of them are closed and
certified here:

| Gap | Closed by | Certified by |
|---|---|---|
| G1 — `init` on construction | `P00-G13` | `programs/13_init_and_invariant` |
| G2 — `invariant()` at the end of construction | `P00-G13` | `programs/13_init_and_invariant`, `diagnostics/11_runtime_invariant_violation` |
| G4 — `Bytes(count)` construction | `P00-G13` | `programs/12_bytes` |
| G2b — `invariant()` at mutation boundaries | `P00-G14` | `programs/15_invariant_on_mutation`, `diagnostics/14_runtime_invariant_mutation_uncaught` |
| G3 — signature contracts at runtime | `P00-G14` | `programs/14_signature_contracts`, `diagnostics/12_runtime_contract_violation`, `diagnostics/13_runtime_return_contract` |

The three residual limits `P00-G14` left open were closed by `P00-G15` and are certified here:

| Residual | Closed by | Certified by |
|---|---|---|
| `aipo-sema` did not use the written annotations for a pre-execution report | `P00-G15` | `diagnostics/15_sem_contract_violation`, `diagnostics/16_sem_return_contract` |
| Interface contracts were accepted instead of checked by structural conformance | `P00-G15` | `programs/16_interface_contracts`, `diagnostics/17_runtime_interface_contract`, `diagnostics/18_runtime_interface_arity` |
| The ADP-001 questions on inverted `clamp` bounds, slice saturation and NFC construction boundaries were open | `P00-G15` | `programs/17_unicode_nfc`, `programs/18_tolerant_slices_and_clamp`, `diagnostics/19_runtime_clamp_inverted_bounds` |
| Module bindings were invisible inside `fn`/`impl` bodies and local `fn` declarations collapsed into anonymous closures | `P00-G16` | `programs/19_local_functions`, `programs/20_module_scope` |

What the MVP subset declares but the implementation still does not deliver is explicit
non-delivery, not an open gap (see `docs/evidence/P00-G16-wave-1-exit-review.md` and
`docs/stdlib/mvp-subset.md` Deferred): LSP/REPL, host ABI/Poppy, packages/registry,
regex/json/fs/http, `graphemes` and hot reload. The Wave 1 deferrals the later waves closed are
recorded below.

### Wave 2 and Wave 3 deliveries certified here

| Delivery | Wave | Certified by |
|---|---|---|
| Wave 2 backend `aipo-js` | `P01-G01` | `crates/aipo-js/tests/differential.rs` (VM = Node on stdout, codes and status) |
| `Set`, `Sequence`, `Bytes` packing, `Duration`, `Task`/`Group` handles | `P02-G01` | `docs/evidence/P02-G01-wave3-types-and-values.md` |
| Cooperative scheduler and async combinators (`task.spawn`/`sleep`/`all`/`race`/`timeout`/`cancel`/`group`) | `P02-G02` | `docs/evidence/P02-G02-wave3-stdlib-async.md` |
| `async fn` call protocol, anonymous/local `async fn`, `await do`, static async diagnostics, await-cycle/cancellation faults, numeric-literal hardening | `P02-G03` | `programs/24`–`programs/27`, `diagnostics/20`–`diagnostics/28`, `docs/evidence/P02-G03-wave3-async-syntax-and-diagnostics.md` |

### Inventory after `P02-G03` (verified on disk)

`programs/` 27 `.aipo` + 27 `.stdout`, `diagnostics/` 29 `.aipo` + 29 `.code`,
`formatting/` 8 `.input.aipo` + 8 `.expected.aipo`, `modules/` 3 entry points
(`basic`, `cycle`, `missing`).

## Delta P07-G02 — implementação sem execução de testes

Fixtures `35_register_profile_shared_services` e `36_semantic_type_tests` acrescentam captures/scheduler no perfil Reg e testes fundamentais/nullable de tipo. Os stdout são expectativas do canon revisadas, não resultados regenerados. `reg_conformance.rs` inclui ambos os engines; `persistent_session.rs`, `guest_transactions.rs`, `reg_verifier.rs` e `type_proofs.rs` registram regressões para futura execução. Nenhuma suite foi executada nesta rodada por pedido do usuário. Consulte [guia](../development/runtime-and-tooling-guide.md) e [evidência](../evidence/P07-G02/README.md).
