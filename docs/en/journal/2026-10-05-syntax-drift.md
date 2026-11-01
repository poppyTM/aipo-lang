# Journal — 2026-10-05 — syntax surface audit and `aipo.ui` repair

**Scope:** verify the implemented grammar against the manual, then repair `packages/aipo-ui`
**Method:** extracted every keyword and operator from `aipo-lexer`, cross-read the grammar in
`aipo-syntax` and the AST in `aipo-ast`, then compiled and executed the package
**Related:** `SYNTAX.md` (new canonical reference), `packages/aipo-ui/tests/ui_test.aipo`,
`crates/aipo-cli/tests/package_suites.rs`

## Why the audit happened

A previous report claimed four broken `.aipo` files. Compiling them showed the real number:
**148 parse errors across all 7 modules** of `aipo.ui`. No test, script or CI job ever compiled
that package, so the drift accumulated unnoticed. `scripts/verify.sh` re-locks every package but
never builds one, and the Rust suites that execute `.aipo` sources only cover `aipo-zoe` and
`aipo-game`.

## Grammar limits found (all confirmed by execution)

These are language properties, not package bugs. Each is a decision input, not something this
change fixes.

### 1. Struct fields take no type annotation

`struct Rect { x: Float }` is a parse error. A field is bare (fixed), `var` (mutable), or has a
default. The manual never showed this because every manual example uses bare fields.

### 2. Type annotations accept one bare name

`-> mod.Tipo` is a parse error. An imported module's exported names arrive unqualified, so
`-> Tipo` works after `import mod as mod_alias`. This applies to `init`, which injects a mutable
`self` only when you do not write one.

### 3. `Type{...}` construction requires an uppercase target

`mod.Tipo{...}` never parses as a construction. Only a bare uppercase identifier does.

### 4. `export` cannot be qualified, and imports are not namespaced

`export mod.Name` is a parse error. `export` of a name that merely arrived via `import` **passes
`aipo check` and then fails at import time** with `AIPO_SEM_EXPORT_UNKNOWN`. Combined with the
fact that `import mod as alias` mixes `mod`'s exported names into the local scope, a facade must
own every public name and its submodules must export distinct names.
`packages/aipo-zoe/src/lib.aipo` and `packages/aipo-html/src/lib.aipo` already follow this
(`make_*` inside, short names in the facade); `aipo.ui` now does too.

### 5. `init` and `div` are reserved

`fn mount(init: Any)` is a parse error — `init` is `TokenKind::Init`. `div` is likewise a
keyword (`TokenKind::Div`), as are `self!` and `div=`.

### 6. `none` does not satisfy a non-nullable contract

`body: Fn = none` then passing `none` is `AIPO_SEM_CONTRACT_VIOLATION_STATIC`. The contract
must be `Fn?`. `Any` behaves the same way.

### 7. `dict.get` takes exactly one argument

`props.get("gap", 0.0)` faults; `dict_get` is `require_arity(args, 1, ...)` and returns `none`
for a missing key. This is the deliberate decision recorded in
`docs/manual/syntax-and-types.md`. `aipo.ui` now uses a `prop_of` helper built on the idiom
from `packages/aipo-zoe/src/widgets.aipo:151`.

## Defects found (compiler/VM — not fixed here)

Each has a minimal, self-contained repro. All are outside the package repair, and per
`CONTRIBUTING.md` a language fix needs a committed conformance fixture.

### D1 — named arguments plus a trailing block skip default-filling

```text
fn f8(a = 0, b = 0, c = 0, d = 0, e = 0, g = 0, h = 0, body = none) -> String {
    return "f8"
}
let r = f8(a = 1) do { io.println("blk") }
```

| named args | result |
|---|---|
| 0 | ok |
| 1..6 of 7 | `AIPO_RT_TYPE_MISMATCH: expected 8 arguments, found N+1 arguments` |
| 7 | ok |

With zero named arguments, or with every parameter supplied, default-filling works. In between,
the named-argument lowering emits a short positional list. Workaround: pass children as
`body = fn() { ... }`.

### D2 — a closure invoked from inside another closure corrupts the operand stack

```text
fn outer(cb) {
    let f = fn() {
        let inner = fn() { cb("x") }
        return "feito"
    }
    return f()
}
outer(fn(m) { io.println(m) })   # AIPO_RT_TYPE_MISMATCH: operand stack underflow
```

Removing the nested closure, or not invoking `f`, makes it pass. Independent of named vs
positional arguments. This blocks `ui.Button(on_click = fn() { ... })` inside a component
`body`, because `body` closures are invoked by the runtime.

### D3 — `aipo check` does not validate member existence

```text
var l = [1, 2, 3]
l.pop()      # check: clean;  run: AIPO_RT_TYPE_MISMATCH ... found List.pop
```

`List` has `remove_last`, not `pop`. A missing member is a runtime fault only, which is how
`aipo.ui` shipped a `List.pop()` call. Worth a `AIPO_SEM_UNKNOWN_MEMBER` in sema.

### D4 — nested quotes break f-string interpolation

`f"gap={n.props["gap"]}"` fails to parse with `unclosed interpolation placeholder`. Bind the
value to a local first.

## Package repair

`packages/aipo.ui`, 7 modules, 148 parse errors to zero. The four layout tags became structs
carrying a canonical token plus one `make_*` constructor per variant, since `enum` is post-V1.
`render_frame` now assigns the view result to `root_node`, which nothing assigned before —
the descriptor pass was receiving `none`.

Also: `List.pop()` → `remove_last()`, and the facade split into local forwarders because
`export` cannot re-export an imported name.

### Gate added

`crates/aipo-cli/tests/package_suites.rs` runs `aipo test` for every host-free package and
asserts `aipo.ui` stays formatter-clean. Verified it fails when drift is injected.

## Pre-existing drift left alone

`aipo fmt --check` reports drift in **37 of 45** package source files (`aipo-html` 4/4,
`aipo-http` 4/5, `aipo-zoe` 15/16, `aipo-game` 12/13, `aipo-egui` 2/2). The conformance
goldens are clean, so this is shipped-package drift, not a formatter bug. Reformatting them
would bury this repair under an unrelated ~4k-line diff, so it is tracked separately.

Correction to an earlier note in this session: the first drift baseline was taken by piping
stdout only, but `aipo fmt --check` writes `would reformat` to **stderr**. Every package
looked clean. The table above is the corrected measurement.
