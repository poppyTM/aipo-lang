# Aipo — Wave 1 plan: executable MVP

**Status:** closed — exit gate met, see `docs/evidence/P00-G16-wave-1-exit-review.md`
(historical plan; the exit criteria below are the certified record, not open work)
**Authority:** implements `docs/canon/Aipo — Waves, Vertical Slices, Gauntlet Loops …md`
**Scope:** Wave 1 objective, slices, exit gate
**Update triggers:** slice decomposition change, gate criteria change

## Objective

Reach an executable, verified, documented, reproducible Aipo MVP: `.aipo` programs run
end-to-end (source → lexer → parser → HIR → sema → Core IR → bytecode → VM) via `aipo run`,
with `aipo check`, `aipo fmt` baseline, structured diagnostics, fixtures/snapshots, and no Rust
panic reaching the user.

## MVP language subset (formal slice)

Everything below is drawn from the closed V1 surface (Language Reference + Canonical Syntax,
Lotes 1–5 + ergonomics 2026-09-10). Anything not listed is out of MVP scope and follows the
no-invention policy.

- Literals: `none`, `true`/`false`, Int (range ±(2^53−1), `_` separators), Float (finite),
  Byte, String (`"..."`, `f"..."`, `r"..."`, `fr"..."`, multiline `"""`).
- Bindings: `let`/`var`; mutability paths (`var`, parameter `!`, `self!`, `fixed` fields);
  destructive binding `let [a, b] = ...`, `let {name} = ...` (surface only).
- Operators: postfix `.` `?.` call/index; unary `+ - not`? (no: `not` sits below comparisons
  per precedence); `* / div %`, `+ -`, `..`, comparisons `== != < <= > >=`, `is`, `not`,
  `and`, `or`, `or_else`, `|>`; compound assignments `+= -= *= /= div= %=`.
- Control flow: `if/elif/else/end`, inline `if c then s [else s]`, `match/when/else/end`,
  `loop`, `while`, `repeat n [as i]`, `each v in c` / `each i, v in c`, `break`, `continue`.
- Functions: `fn name(params) [-> T] ... end`, default params, named args, local `fn`,
  anonymous `fn`, closures capturing bindings (per-iteration capture in loops),
  value/no-result distinction, `return`/`return expr`.
- Calls: positional before named, trailing blocks `do ... end`, `callee do ... end` sugar,
  pipeline `|>`, dot-call for `impl` associated functions only.
- Data: `struct` (fields, defaults, `fixed`), construction `Type{...}` positional+named,
  `impl Type` (init, invariant, associated fns), `List`/`Dict` literals + core ops,
  indexing/slicing with negative indices, ranges `a..b`.
- Errors: `fail("msg")` / `fail(err)`, `or_else`, `attempt ... failed err ... end`,
  `err.message`; runtime faults (numeric, index, key, mutation-during-iteration,
  contract violations at runtime).
- Contracts: parameter/return `name: Type`, `name!: Type`, `-> T`, `T?`; runtime check at
  boundaries; `is` narrowing; interfaces + `satisfy` (structural check).
- Modules: one file = one module; `import m`, `import m: names`, `import m as alias`,
  `export`, structural resolution, acyclic, eager init once.
- Prelude/builtins: `none true false len copy same some fail` + core types; minimal stdlib:
  `io.print`, `String` core ops (`len byte_len contains starts_with ends_with find replace
  split join trim lower upper capitalize reverse format`), `List` core ops (`add insert
  remove remove_at remove_last clear contains find first last is_empty count reverse sort
  sort_by filter transform`), `Dict` core ops (`has get keys values remove clear is_empty`),
  numeric utilities (`abs min max clamp sqrt pow floor ceil round truncate`),
  `Int() Float() Byte() String()` conversions.
- CLI: `aipo run`, `aipo check`, `aipo fmt`, `--message-format=jsonl`.

**Explicit MVP exclusions** (post-MVP waves): JS backend, async/await, host ABI/Poppy,
packages/registry, LSP, REPL, regex/json/fs/http, lazy `Sequence`, `Set`, `Bytes` packing
APIs beyond construction/indexing, `graphemes`, hot reload.

## Wave 1 vertical slices

Each slice is observable end-to-end where applicable, with its own fixtures and tests.

| # | Slice | Deliverable |
|---|---|---|
| S1 | Workspace + diagnostics + source | cargo workspace, `aipo-source`, `aipo-diagnostics`, JSONL emitter, CI baseline |
| S2 |Lexer core | tokens for the MVP surface incl. f/r/fr strings, `TokenKind` snapshot tests |
| S3 | Parser core | lossless syntax + typed AST for statements/expressions, recovery, snapshots |
| S4 | HIR lowering | trailing blocks, pipelines, ellipsis chains, conditional-value normalization |
| S5 | Sema baseline | scopes, mutability paths, arity/contracts, module structure, fail-fast diagnostics |
| S6 | Core IR + bytecode | IR build, instruction set v1, emitter, verifier, disassembler snapshots |
| S7 | VM core | frames, value model, arithmetic w/ Int range + finite Float, control flow, calls/closures |
| S8 | Data + errors | structs (fixed/invariant), List/Dict ops, Failure/fault model, attempt/or_else |
| S9 | Modules + stdlib | import/export/init-once, prelude, stdlib MVP subset |
| S10 | CLI + formatter | `aipo run/check/fmt`, exit codes, golden corpus |
| S11 | Conformance hardening | pass/fail fixture suite, snapshot matrix, Gauntlet rubric score, MVP gate |

## MVP exit gate (evidence required)

- `aipo run` executes real small programs; `aipo check` reports diagnostics; `aipo fmt` idempotent.
- pass/fail fixtures green; snapshots committed; integration tests green.
- No Rust panic escapes as user error (fuzz smoke over pipeline with random bytes).
- CI: fmt --check, clippy -D warnings, test, doc all green.
- `prumo validate` and `prumo doctor` green; docs delta resolved; evidence recorded here.
