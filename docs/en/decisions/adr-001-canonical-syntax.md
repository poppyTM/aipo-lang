# ADR-001 — Aipo canonical syntax

**Status:** accepted
**Date:** 2026-10-06
**Replaces:** `fn Type.name(...)` associated syntax, `satisfy`, `impl`, `self!`, `end`, `div`
**Target:** Aipo V1

---

## 1. Principle

Aipo is designed for neurodivergent people (ADHD, dyslexia, cognitive overload). Four rules
govern every surface decision:

1. **One form per thing.** Never two spellings for the same operation, never a style choice at
   write time.
2. **Complete word beats symbol.** `and` over `&&`, `not` over `!`. Dense symbols glued
   together have similar silhouettes and cause re-reading.
3. **Zero ceremony for the common case.** Structures that appear in every file are short and
   need no header.
4. **The error says what to do.** A diagnostic points at the position, names the problem and
   offers the correction, in one line.

Explicit non-goals: brevity of expression, density of expression, compatibility with other
languages.

---

## 2. Blocks

Every opening uses `{`, every closing uses `}`.

```text
if cond {
    io.println("ok")
}
```

The `end` keyword is **removed**. It was ambiguous when three nested blocks ended in sequence,
and braces have a visual outline that editors highlight as pairs.

Applies to: `fn`, `async fn`, `struct`, `interface`, `if`/`elif`/`else`, `match` (each
`when` arm), `loop`, `while`, `repeat`, `each`, `attempt`/`failed`, `await do`, trailing block.

### 2.1 `then`

- **With braces:** never uses `then`.
- **Without braces (inline, with value):** uses `then`. The only blockless form.

```text
let rotulo = if idade >= 18 then "maior" else "menor"
```

Rule: has `{}`? No `then`. No `{}`? With `then`.

---

## 3. Types

### 3.1 Struct — data only

```text
struct Servidor {
    id: Int                       # immutable by default
    var status: String = "online" # `var` = mutable
    var carga: Float = 0.0
}
```

Field type annotation is **optional**. Without it, the field promises nothing to the compiler —
useful in prototypes, costly to avoid.

`invariant()` verifies **value** rules, not types: it runs at runtime, after construction and
at stable mutable boundaries.

### 3.2 Interface — signatures only

```text
interface Drawable {
    draw() -> Int
}
```

No bodies, no `fn`, no `self`.

### 3.3 Structural satisfaction

A type satisfies an interface when it has all methods with a compatible signature.
Satisfaction is **automatic** and requires no declaration.

Signature comparison includes the return type and parameter types. For receiver mutability the
rule is covariant:

- Interface `draw()` (immutable) accepts implementation `draw(var self)` — stronger, safe.
- Interface `move(var self)` requires a mutable implementation.

---

## 4. Methods and hooks

### 4.1 Association through `:`

A method belongs to a type through the `Tipo:` prefix.

```text
Point:init(x, y) {
    self.x = x
    self.y = y
}

Point:invariant {
    self.x >= 0
}

Point:dist() -> Float {
    return math.sqrt(self.x * self.x + self.y * self.y)
}

Point:move(var self, dx, dy) {
    self.x += dx
    self.y += dy
}
```

- `init` and `invariant` are **hooks** recognized by name; the rest are methods.
- **Implicit `self`:** not written in an immutable method. The parser injects it, as it
  already does for `init` and `invariant` in the current grammar.
- **Explicit `var self`:** required for mutation. One rule throughout the language: `var`
  means mutable.
- `fn` **does not** appear. The `Tipo:` association already declares that it is a function.

### 4.2 Call

Calls use `.`, as today:

```text
let p = Point{x = 3, y = 4}
let d = p.dist()          # passes
let m = p.move(var dx = 1, dy = 1)
```

`:` marks **association in declaration**; `.` marks **access in use**. One operator, each use.

### 4.3 Batch binding through `::`

Free functions can be promoted to methods of a type without rewriting the body.
**Requirement:** the first parameter must be `self` (or `var self`).

```text
fn calcular_area(self) {
    return self.largura * self.altura
}

Retangulo::[calcular_area]

fn ajustar(var self, delta) {   # stays mutable
    self.largura += delta
}

Retangulo::[ajustar]           # mutable method promoted
```

`::` is the **batch association** token. Mutability comes from the source signature: `self` →
read-only method, `var self` → mutable method.

Functions without `self` as the first parameter are **rejected by the compiler**: batch exists
to promote type behavior, not for namespacing.

Semantics: the binding is a **promotion**. The function remains callable as a free function
and gains the associated method.

> Note: `:` is the canonical association token. `::` exists **only** for the batch.

### 4.4 `fn` keyword

`fn` is reserved for **free functions**.

```text
fn soma(a, b) -> Int { return a + b }    # free → uses fn
Point:dist() -> Float { ... }              # associated → no fn
```

The contrast makes `fn` more meaningful, not less: it marks what does not belong to a type.

### 4.5 Explicit `self` is accepted

The implicit form is **canonical** and what the formatter produces. The explicit form is
accepted without error or warning:

```text
Point:dist() -> Float { ... }              # canonical — implicit
Point:dist(self) -> Float { ... }          # accepted — explicit
Point:move(var self, dx, dy) { ... }       # mutation — `var self` mandatory
```

The parser normalizes both to the same internal form (the receiver is always the first slot of
the frame), so there is no cost in accepting both. There is no diagnostic: whoever writes
`self` explicitly knows exactly what they are doing, and the `var` rule remains the only way
to declare mutation.

---

## 4bis. Surface diagnostics

Goal: whoever errs **sees what to do**, not just what is wrong. An error occupies at most one
line of message, one line of code with a caret, and a hint.

Rendering format:

```
error [AIPO_SEM_TIPO_NAO_ESTRUTURA]: 'with' only works with a struct

  14 │     let novo = n with { y: 1 }
     │                ^ no struct here

  hint: use `with` on a struct, not on Int
```

Hard limits: **one** message, **one** primary span, **at most one** note and **one**
suggestion. Color carries only structure (the code, `error`, the caret), never meaning.

### 4bis.1 Warning: hook name in a common method

`init` and `invariant` are recognized by name. A method called `inicializar`,
`initialize`, `constructor`, `validar` or `validate` looks like it wants to be a hook and
is not.

```
warning [AIPO_SEM_NOME_DE_HOOK]: 'validar' looks like a hook, not a method

   7 │ Point:validar() -> Bool { ... }
     │       ^^^^^^^^ common method; does not run after construction

  hint: rename to `Point:invariant { ... }` if the intent is validation
```

Same format for `init`:

```
warning [AIPO_SEM_NOME_DE_HOOK]: 'constructor' looks like a hook, not a method

   5 │ Point:constructor(x) { ... }
     │       ^^^^^^^^^^^ common method; does not run at construction

  hint: use `Point:init(x) { ... }` for the construction hook
```

Severity `Warning`, not `Error`: the code works, it is just the intent that was probably
not achieved. A warning with no possible action is noise; this one has an action.

### 4bis.2 Warning: empty hook body

```
warning [AIPO_SEM_HOOK_VAZIO]: 'invariant' with no condition checks nothing

   9 │ Point:invariant {
     │            ^^^^^^^^^ no predicate

  hint: write at least one condition, or remove the hook
```

### 4bis.3 Error: repeated hook

```
error [AIPO_SEM_HOOK_DUPLICADO]: 'Point' already declares 'invariant'

  21 │ Point:invariant { self.x > 0 }
     │       ^^^^^^^^^ already declared on line 9

  hint: gather the conditions into a single `invariant`
```

### 4bis.4 Error: missing mutable `self`

```
error [AIPO_SEM_RECEPTOR_IMUTAVEL]: 'self' is read-only here

  14 │ Point:avancar() { self.x += 1 }
     │                ^^^ assignment on an immutable receiver

  hint: declare `Point:avancar(var self)` to allow mutation
```

This is `Error`, not `Warning`: without it the code does not compile at all, and the
exact reason is the most likely friction point for anyone migrating.

### 4bis.5 Error: the three path tokens

`.` accesses **value**, `:` associates **one** declaration, `::` associates **several**.

Error when calling a method with no receiver through `.` with a loose value:

```
error [AIPO_SEM_METODO_NAO_ASSOCIADO]: 'somar' is not a method of 'Int'

   9 │ let total = 3.somar(4)
     │             ^^^^^ Int has no such method

  hint: use the free function `somar(3, 4)`, or define `Int:somar()`
```

Error when using `::` without a list:

```
error [AIPO_PARSE_LOTE_INVALIDO]: '::' expects a list in brackets

   6 │ Point::desenhar()
     │       ^^ use ':' to declare a single method

  hint: `Point:desenhar()` declares a method; `Point::[f1, f2]` groups functions
```

This pair of messages is what **paid for** keeping the three tokens: the error names
the token, the reason and the correct form on the same screen.

### 4bis.6 Warning: orphan `#!`

A directive loose at the end of the file, with no following item:

```
warning [AIPO_PARSE_DIRETIVA_ORFA]: '#!test' has no following function

  42 │ #!test
     │       ^^^^^ no declaration after the directive

  hint: the directive applies to the next `fn`; move it just above the test
```

### 4bis.7 Note: explicit `self` where implicit is canonical

Accepted, **without** diagnostic (see §4.5). Not on the list.

---

## 4ter. Batch binding

Approved on 2026-10-06, with the status recorded in
[`docs/decisions/surface-decisions.md`](surface-decisions.md).

---

## 5. `#!` directives

A comment is `#` to end of line. There is no block comment.

`#!` introduces a **directive**: a comment the compiler reads.

| Directive | Where | Effect |
|---|---|---|
| `#!satisfies I1, I2` | immediately before `struct` | Requires the type to satisfy the interfaces. Error if it fails. |
| `#!test` | immediately before a free `fn` | Marks the function as a unit test. |
| `#!test[tag]` | idem | Test with a tag, filterable by `aipo test --filter tag`. |
| `#!test("nome")` | idem | Test with an explicit descriptive name. |

Common comments are discarded by the parser. Directives pass through: the lexer emits
`TokenKind::Directive`, and the parser associates each directive with the **next
declared item**.

### 5.1 Why a directive instead of a declaration

`#!satisfies` keeps the keyword list short, leaves the intent visible to anyone reading
the file, and survives tools that discard comments because it is verified by the
compiler.

### 5.2 Test discovery

Two paths, both active:

- **File convention:** `*_test.aipo`, `test_*.aipo` (compatibility)
- **Directive:** `#!test` inside any `.aipo` file

```text
#!test
fn soma_basico() {
    io.println(String(soma(1, 2)))    # 3
}

#!test[parser]
fn parse_match_com_guard() {
    ...
}

#!test("integer division truncates toward zero")
fn divide_trunca() {
    ...
}
```

One test per file is no longer mandatory: small functions stay grouped in the file
they test.

### 5.3 Limits

- The directive applies to the **following item**, not the previous one. A directive
  with no following item is a parse error.
- At most one `#!test` directive per function.
- `#!test` marks **free functions**. A `Tipo:nome` method is not a test: it needs
  `self`, so it is not an isolated unit.
- Form and argument are validated: `#!test` does not accept content that is not a tag
  between `[]` or a string in quotes.

---

## 6. Integer division

`//` and `//=` are integer division truncated toward zero.

`div` and `div=` are **removed**; `div` becomes a common identifier again.

```text
let metade = total // 2
```

`/` remains division that produces a `Float`.

### 6.1 The trap and the mitigation

`//` is division, not a comment — which collides with the habit formed in JavaScript,
Java, C, C++, Rust, Go, C#, PHP and Swift. Muscle memory writes a comment with `//`.

Mitigation: when `//` appears where no valid expression can follow, the diagnostic
suggests `#`.

```text
// isto é um comentário
// ^ AIPO_PARSE_UNEXPECTED_TOKEN
//   hint: use `#` for comments; `//` is integer division
```

---

## 7. Parameters

```text
fn salvar(usuario: User, var contador: Int, modo = "rapido") { }
```

Two orthogonal dimensions:

- **Mutability:** `var nome` or `nome` (immutable by default)
- **Type:** `: T`, optional

`nome!` is removed. The `!` was too subtle to read and confused with `not`.

Accepted forms: `nome` · `var nome` · `nome: T` · `var nome: T` · `nome = default` ·
`var nome: T = default` · `_`.

---

## 8. Modules

A `.aipo` file is a module.

```text
import math_util
import math_util as mu
import math_util: somar, dobro
export somar, dobro
```

Private by default. Accessing a non-exported symbol is `AIPO_SEM_UNKNOWN_NAME`.

`.` is member access on a module **and** a field. `::` is type association. The three
uses stay distinct: `.` for value, `:`/`::` for code.

---

## 9. Failures

```text
fail "mensagem"
let v = ler_porta() or_else 8080
let w = risky()?
attempt { ... } failed erro { ... }
```

`?` propagates. `or_else` is the fallback. `attempt`/`failed` captures a block.

There is no `try`, `catch`, `finally`, `throw`, `raise`, `defer`.

---

## 10. Async

```text
async fn buscar(recurso) {
    return http.get(recurso)
}

let v = await buscar("usuarios")

await do {
    let a = await etapa_1()
    let b = await etapa_2()
    return a + b
}
```

`await` is legal in statement, initializer and return value. Subexpression is
`AIPO_SEM_AWAIT_IN_SUBEXPRESSION`.

---

## 11. Literals

### 11.1 Strings

| Form | Name |
|---|---|
| `"..."` | normal |
| `f"..."` | interpolated |
| `r"..."` | raw |
| `fr"..."` / `rf"..."` | raw interpolated |
| `"""..."""` | multiline |
| `r"""..."""` / `fr"""..."""` | raw multiline |

Escapes: `\n` `\r` `\t` `\\` `\"` `\0` `\u{HEX}`. Content is normalized to NFC.

### 11.2 Numbers

`0x` `0b` `0o`, `_` as separator between digits of the same base. `Int` is a signed
integer of 64 bits with safe range ±(2^53−1). `Float` is IEEE 754 without `NaN` and
without `Infinity`.

---

## 12. Comments

`#` to end of line. The **only** comment in the language.

`#!` introduces a directive (§5).

---

## 13. Final count

**42 keywords.** Removed in this decision: `end`, `div`, `self!`, `impl`, `satisfy`.

Non-keyword tokens added: `:` (association), `::` (batch); `//=` already existed.

---

## 14. `enum` — sum type

`enum` is **promoted to V1** (see `SYNTAX.md` §17 for the full design).

An `enum` is a closed type: the compiler knows every variant and `match` over it is
**verified by exhaustiveness**. That is the reason it exists — without `enum`, case
coverage is convention; with it, it is a compiler obligation.

Three variant forms, with no new payload syntax:

```text
enum Estado {
    Inicial,                     # without payload
    Ativo { desde: Int },        # named fields
    Desligado(motivo: String),   # one positional field
}
```

Construction is the qualified name, without `new` and without `::`:

```text
let e = Estado.Desligado("manutenção")
```

`match` names type **and** variant, reusing the destructure from §6.1:

```text
match e {
    when Estado.Inicial { }
    when Estado.Ativo { desde } { }
    when Estado.Desligado(motivo) { }
}
```

**No new pattern or binding concept:** `enum` composes with `fail` (§7), `Task` (§9),
`Tipo:invariant` hooks (§5.2) and `#!satisfies` (§5.4) exactly like `struct`. The
design considerations of §1 apply: one form per thing (`Tipo.Variante` to construct
and to match), complete words over symbols, and the exhaustiveness error names the
missing variants.

Runtime representation stays a **backend decision** (tagged union with heap vs. int tag +
inline payload); the surface does not depend on it.

---

## 15. What does not exist

`defer` · `try` · `catch` · `finally` · `throw` · `raise` · `for` · `class` · `const` ·
`static` · `pub` · `private` · `unsafe` · `yield` · `lambda` · `module` · `void` ·
`case` · `where` · generics · bitwise · `;` · `&&` · `||` · `++` · `--` · `**` · `?:` ·
cast with `as`

Non-tokenized operators: `@` `&` `;` `&&` `||` `++`. An isolated `|` produces
*"did you mean '|>'?"*.
