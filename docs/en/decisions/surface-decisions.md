# Surface-decisions roadmap: decided · deferred · pending

Living record of Aipo's surface-language decisions, so context is not lost between
conversations. **Rejected options** are deliberately kept here: a future conversation
may reopen them without rebuilding the history.

## Decided

### A.1 — `expr?` (propagation of `Failure`)

- **Syntax:** `let v = f(x)?` — postfix, binds tighter than every binary operator.
- **Semantics:** evaluates the operand; if the result is `Failure`, propagates to the nearest
  `attempt` (or to the caller when there is no handler); otherwise delivers the value.
- **Legal in any expression position.** Unlike `await`, which is restricted to statement,
  initializer and return value (`AIPO_SEM_AWAIT_IN_SUBEXPRESSION`).
- **Chain:** the IR lowers `PropagateFailure` to `CheckFailure` (`OpCode`), which is exactly
  the check the VM already runs at every statement boundary. No new VM instruction.
- **Conformance:** `docs/conformance/programs/29_try_propagation_operator.{aipo,stdout}`.
- **Tests:** `crates/aipo-cli/tests/try_operator_tests.rs` (3 green).

### A.2 — `expr with { ... }` (functional struct update)

- **Syntax:** `let novo = p with { y: 99 }` — **Option A**, keyword `with` + braces.
- **Semantics:** produces a **new** value; `p` stays untouched. Fields absent from the block
  are copied from the original.
- **Shannon's law:** the block lists **only the changes**, so field order does not matter
  and there is no observable intermediate state.
- **Legal in any expression position**, like `?`.
- **Reuse:** the block is the same field list as `Construct`, reusing the struct-field
  validation already in sema.

#### Option C — recorded, not chosen

`Point(p, y: 99)` — no new keyword; reuses `Construct` with the struct as the first
argument.

- **Pros:** zero new syntax in the lexer; possibly the smallest compiler diff.
- **Cons:** repeats the struct name on every use; changes `Construct`'s precedence (it now
  accepts a base value before the brace); worse readability when nested
  (`Point(a, x: Point(b, x: 1))`).
- **When to reopen:** if the priority is eliminating syntax expansion at any cost, or if
  `with` leaves scope and struct updates become rare enough not to justify a keyword.
  Reopening requires revisiting the ambiguity with positional `Construct`.

#### Discarded without record

- **Option B** (`p with y: 99`, no braces): braces delimit the change block and keep
  reading locally predictable. Only differs from A in visual noise; no gain.

## Pending

### A.3 — Destructuring and guards in `match`

- **Destructuring — Option A** (`when { nome, idade }`): no type repetition, because
  extraction follows the same guarantee as dot access — a missing field is `TypeMismatch`
  at runtime, and the typing that C (narrowing) brings only makes the error earlier.
  Option B (`when Point { nome, idade }`) was considered and dismissed as noise; when
  arms mix different structs, natural fallthrough already selects the first that matches.
- **Guards — Option A** (`when pattern if guard then`): reuses `if`, zero new keyword.
  A `false` guard falls through to the next arm, not to `else` — it is a filter, not a
  diversion.
- **Bindings live in the arm:** declared in the arm's scope and visible in the guard and
  body; they do not leak to siblings or outside the statement.
- **Guard is a condition:** `check_condition_bool` applies, same rule as `if`/`while`.
- **Implementation:** destructuring becomes `Load target → GetField → Store` before the
  body (bindings established before the guard, so the guard can test fields); the guard
  becomes `JumpIfFalse` to the arm's fail-jump. Destructure emits an unconditional `Jump`
  to the body (it cannot fail) — that jump was missing in the first build and left the
  arm unreachable.
- **Wasm:** faced with destructuring or guards it returns an explicit `UnsupportedStmt`,
  instead of compiling wrong in silence (same pattern as `Try`, which falls to the
  catch-all).
- **Canonical spec contradicts:** `Language Reference` (l. 929) and `Especificação Viva`
  (l. 1368) place guards and destructuring **outside V1**. A.3 is a deliberate extension;
  docs need a closing section on the change before release.
- **Conformance:** `docs/conformance/programs/31_match_destructure_and_guards.{aipo,stdout}`.
- **Tests:** `crates/aipo-cli/tests/match_destructure_tests.rs` (6 green: bind, guards
  accept/reject, subset, guard in value pattern, arm scope, non-Bool guard).

### B — ADTs (`enum`) — **decided 2026-10-06, promoted to V1**

Context: `enum` was in the post-V1 backlog. Promoted because the language has no other way
to express "this is A **or** B" with coverage checking, and `match` over a closed type
is what guarantees exhaustiveness at compile time.

Surface decision (canon in `SYNTAX.md` §17, norm in `adr-001` §14):

- Three variant forms: `Nome`, `Nome { campo: T }`, `Nome(valor: T)`. No new payload syntax.
- Construction is the qualified name: `Estado.Desligado("x")`. No `new`, no `::`.
- `match` names type **and** variant: `when Estado.Ativo { desde }`. Reuses A.3's
  destructure; the difference is that `{ ... }` binds variant + fields.
- **Exhaustiveness is mandatory:** `match` without `else` and without all variants is an error.
- `enum` brings no new pattern or binding concept: it composes with `fail`, `Task`,
  `Tipo:invariant` and `#!satisfies` like `struct`.

Batch mutability (decided together): batch requires `self` as the first parameter of the
source function; `self` → read-only method, `var self` → mutable. No new token — `var`
keeps the same meaning throughout the language.

Runtime representation (decided 2026-10-07): **erasure to the named struct
`"Enum.Variante"` as the V1 representation**. Each variant becomes a struct instance with
that type name; tuple variants get a synthesized constructor `E.V(params)`; `match`
dispatches through the `IsVariant` opcode (name comparison, with a suffix for module
qualification) and the payload is read with ordinary `GetField`. Total VM↔JS parity through
the same scheme; Wasm rejects explicitly (`UnsupportedStmt`, pinned in
`crates/aipo-wasm/tests/match_tests.rs`). A real tagged union (`Value::Enum` with a
discriminant) is left for the future if measurement shows string dispatch is a bottleneck
— the swap is internal, with no surface change. Static validity is guaranteed in sema:
duplicate variant (`AIPO_SEM_REDECLARED_IN_SCOPE`), unknown variant in `match` and in
calls (`AIPO_SEM_UNKNOWN_NAME`), tuple arity and calling a non-tuple variant
(`AIPO_SEM_ARITY_MISMATCH`), non-exhaustive match without `else`
(`AIPO_SEM_NON_EXHAUSTIVE_MATCH`). Observable fixture:
`docs/conformance/programs/32_enum_variants_and_matching.aipo`.

### C — Type narrowing

## Canonical syntax decisions (ND philosophy) — 2026-10-06

Context: Aipo is a language that is easy to read, write and maintain, with priority for
neurodivergent people (ADHD, dyslexia, cognitive overload). Rules guiding everything below:
**one form per thing**, complete words instead of dense symbols, and errors that say what to do.

General status: **decided, pending implementation**. The compiler still accepts the old
forms; the normative docs (`SYNTAX.md`, Language Reference, Especificação Viva) still
describe them. Implementation removes the dead code and updates the docs in the same change
— never one without the other.

### T1 — Blocks: only `{}` (`end` removed)

Every opening uses `{`, every closing uses `}`. The `end` keyword leaves the lexer.
Reason: triple `end` is visually ambiguous; braces have a clear outline and editors
highlight pairs. Already decided in ADP-012; removal remains.

### T2 — Integer division: only `//` and `//=`; `div`/`div=` removed

`div` becomes a common identifier again. Choice **B** for the `//` trap: `//` keeps being
division, but the compiler gains a friendly diagnostic — when `//` appears where no valid
expression follows, it suggests `#` as a comment instead of emitting `DIV_ZERO` or
`UNKNOWN_NAME`.

### T3 — Types in struct: optional annotation + `invariant()` for value rules

Fields accept an optional type (`id: Int`, `var status: String = "online"`).
`invariant()` is **not** the tool for types: it runs at runtime, after construction and
at mutable boundaries, so it serves **value** rules (`self.id > 0`), not type promises.
Annotation = what the author promises (allows static checking once C/narrowing arrives);
invariant = what the value must satisfy (always checked at runtime). With no annotation,
nothing changes for prototyping — progressive disclosure, without punishing exploration.

### T4 — Receiver: only `self` (immutable) and `var self` (mutable)

`self!` leaves the parser (`SelfMut` token removed). One rule only: `var` = mutable,
everywhere. Already decided in ADP-012; dead code remains to be removed (the parser still
accepts `self!` in `parse_params`).

### T5 — Parameters: `nome` (immutable) and `var nome` (mutable), with optional `: T`

`nome!` leaves the parser (`!` is too subtle for dyslexics and confuses with `not`).
Two orthogonal dimensions: mutability (`var` or not) × type (`: T` or nothing).

### T6 — `then`: never in a block, only inline

Rule: has `{}`? No `then`. No `{}`? With `then` (`if c then a else b`).
Removes the current inconsistency (`then` optional in block, mandatory inline).

### T7 — Qualified construction `mod.Tipo{}` + friendly diagnostic

`geo.Point{ x: 1 }` becomes valid. A lowercase start (`ponto{...}`) becomes a diagnostic
("struct names start with a capital letter: did you mean `Ponto`?") instead of a generic
parse error.

### Keyword count (target)

`end`, `div`/`div=`, `self!` leave: 46 → 43 (confirm the exact number in the
implementation, once the tokens are removed from `token.rs`).

### Docs that contradict and need updating on implementation

- `SYNTAX.md` §1 and §12 ("two terminators", table with `end`; `div` as keyword)
- Language Reference l.41 ("blocks end with `end`") and l.929 (match without guards)
- Especificação Viva l.636–637 (`end` official) and l.1368 (destructuring/guards outside V1)
- Hooks interlude (`self!` canonical), ADP-002 (`init(self!, ...)` in examples)

### Open

- **Topic A** — function keyword (`fn` vs `func` vs others): under discussion.
- **Topic B** — error/warning format: under discussion.

## Bug fixed (was pre-existing)

`docs/conformance/programs/17_unicode_nfc.aipo` failed: the **IR constant folder**
concatenated strings with `format!` without normalizing, while the VM's `Add` composes
NFC. Because `aipo run` uses `optimize = true` by default, inlined literals
(`"e" + "\u{0301}"`) were folded without normalization. Fixed in
`crates/aipo-ir/src/opt.rs` (the folder normalizes like the runtime), with regression
test `test_folds_string_concatenation_to_nfc`. The conformance suite is **green for
the first time** (3/3 `test_programs*`).
