# Aipo — positive and negative grammatical rules (guide for LLMs)

This document establishes the strict syntactic conformance contract for language models. Any
violation of these rules produces a lexical or syntactic parse error (`parse error`) in the
Aipo V1 compiler.

---

## 1. The 4 cardinal rules of the language

1. **One form per thing:** there are never two syntaxes for the same operation.
2. **Complete word beats symbol:** `and` instead of `&&`, `not` instead of `!`, `or` instead of `||`.
3. **Zero ceremony for the common case:** blocks uniformly use `{}`. No redundant headers or annotations.
4. **The error says what to do:** diagnostics point at the line, the cause and the expected correction.

---

## 2. Negative inventory — what does NOT exist (absolute prohibitions)

The list below contains words, symbols and constructions common in other languages that **DO NOT
EXIST** in Aipo V1. Emitting any of these items is a generation failure.

### 2.1 Bloated / non-existent keywords

| Forbidden to the LLM | Motivation / hallucination cause | Correct form in Aipo V1 |
|---|---|---|
| `// comment` | Bias from C/JS/Rust/Go. In Aipo, `//` is integer division! | `# comment to end of line` |
| `/* block */` | No block comment exists. | Repeat `#` on each line. |
| `end` | Removed in V1 (ADR-001). Blocks use `{}`. | `}` to close any block. |
| `for ... in ...` | Bias from Python/JS/Rust. | `each item in xs { ... }` |
| `for (i=0; ...)` | Bias from C/JS. | `repeat N as i { ... }` or `while` |
| `try { } catch` | Bias from JS/Java/Python/C#. | `attempt { ... } failed erro { ... }` |
| `throw` / `raise` | Bias from JS/Python. | `fail "mensagem"` or `return fail(...)` |
| `finally` | Bias from exception-oriented languages. | No equivalent. The `attempt` block has atomic rollback. |
| `defer` | Bias from Go/Zig/Swift. | Does not exist. Resources and state use the transactional journal. |
| `class` | Bias from traditional OOP. | `struct Nome { ... }` + `Nome:metodo() { ... }` |
| `impl Nome { }` | Removed in V1. Methods are declared individually. | `Nome:metodo() { ... }` at the top of the file. |
| `satisfy` | Removed in V1. Satisfaction is 100% structural. | Remove it. Optionally use the directive `#!satisfies I1`. |
| `self!` | Removed in V1. | `var self` in the first parameter of the mutable method. |
| `div` / `div=` | Removed in V1. | `//` for integer division and `//=` for assignment. |
| `const` | Bias from JS/C/Rust. | `let` (bindings are already immutable by default). |
| `pub` / `public` / `private` | Bias from Rust/Java/TS. Visibility is per module. | Everything is private by default. Expose with `export nome1, nome2`. |
| `switch` / `case` | Bias from C/JS/Go. | `match alvo { when padrao { ... } else { ... } }` |
| `where` | Bias from Rust/SQL. | No `where` clause exists. Use guards: `when p if cond { }`. |
| `void` / `nil` / `null` | Bias from C/Go/JS/Python. | `none` is the canonical unit / null value. |
| `lambda` / `function` | Bias from Python/JS. | `(x) => expr` or `fn(x) { ... }` |
| `import.types` | Invalid dotted token in the parser. | `import types` (with a space). |

### 2.2 Forbidden / non-existent operators

| Forbidden symbol | Canonical equivalent in Aipo V1 |
|---|---|
| `&&` | `and` |
| `||` | `or` |
| `!` (logical negation) | `not` (e.g. `not ativo`, `not (x > 0)`) |
| `++` / `--` | `x += 1` / `x -= 1` |
| `;` (semicolon) | Newline terminates statements. Semicolon forbidden. |
| `?:` (ternary operator) | `if c then a else b` |
| `a < b < c` (chained comparison) | `a < b and b < c` (or the elision sugar: `v >= 0 and <= 100`) |
| `is not T` | `not v is T` |
| `x as Tipo` (cast) | `Tipo(x)` (e.g. `Int(3.14)`, `String(42)`). `as` is only used in `import ... as` and `repeat N as i`. |
| `@decorador` | No `@` decorator exists. Directives use `#!nome`. |

---

## 3. Positive rules — how to write canonical code

### 3.1 Blocks and indentation
- **Rule:** every block opens with `{` and closes with `}`.
- **Rule for `then`:**
  - Multiline block: **NEVER** use `then`. Example: `if ativo { ... }`.
  - Inline value expression: **ALWAYS** use `then`. Example: `let x = if c then 1 else 2`.

### 3.2 Comments and integer division
- `#` starts a single-line comment to `\n`.
- `//` is the **truncated integer division** arithmetic operator (`7 // 2` results in `3`).

### 3.3 Variables and mutability
- `let x = 10`: immutable binding (default).
- `var x = 10`: mutable binding.
- Destructuring: `let [a, b] = lista`, `let {nome, idade} = pessoa`.
- Discard: `_` discards an unwanted value.

### 3.4 Structs (data only)
```text
struct Usuario {
    id: Int                        # Immutable by default
    nome: String                   # Immutable by default
    var status: String = "ativo"   # Mutable with default value
    var tentativas: Int = 0        # Mutable with default value
}
```
- **Field typing:** type annotation on struct fields is **optional**, but **EXTREMELY RECOMMENDED**. Leaving fields untyped (`id` instead of `id: Int`) is tolerated in quick prototypes but is an engineering anti-pattern in production code and in LLM responses.
- Structs contain **only fields**. NEVER put methods inside `struct { }`.
- Canonical construction: `Usuario{ id: 1, nome: "Ana" }`. The type name **MUST** start with a capital letter.
- Functional update (modified copy): `let u2 = u1 with { status: "inativo" }`.

### 3.4.1 Enums (closed sum types — §17)
`enum` represents closed alternatives ("this is A **or** B") with **mandatory exhaustiveness** checking by the compiler.

```text
enum EstadoConexao {
    Desconectado,
    Tentando(tentativa: Int),
    Conectado { ip: String, ping_ms: Int },
    Erro(mensagem: String),
}
```

Three variant forms are supported:
1. **Without payload:** constant value (`Desconectado`).
2. **Named payload:** labeled fields (`Conectado { ip: String, ping_ms: Int }`).
3. **Positional payload:** a single value in parentheses (`Tentando(tentativa: Int)`).

**Grammatical rules:**
- Comma required between variants and a trailing comma required before `}`.
- Direct construction qualified by the type: `let e = EstadoConexao.Conectado { ip: "127.0.0.1", ping_ms: 10 }`.
- **Exhaustiveness in `match`:** a `match` over an enum **without** an `else` clause MUST cover every variant, or it is a compile error.
- Enums accept methods (`EstadoConexao:metodo()`) and hooks (`EstadoConexao:invariant`).

### 3.5 Methods and type hooks
Methods are associated outside the struct or enum using the `Tipo:nome` syntax:
```text
# Initialization hook (runs at construction)
Usuario:init(id, nome) {
    self.id = id
    self.nome = nome
}

# Invariant hook (predicate tested on every state mutation)
Usuario:invariant {
    self.id > 0
    len(self.nome) > 0
}

# Immutable method: `self` is implicit (do NOT use `fn`)
Usuario:descricao() -> String {
    return f"#{self.id}: {self.nome} ({self.status})"
}

# Mutable method: `var self` is required on the receiver
Usuario:desativar(var self) {
    self.status = "inativo"
}
```

### 3.5.1 Promotion and batch association with `::`
When free functions already exist, they can be promoted in batch to methods of a type through the `::` operator:
```text
# MANDATORY REQUIREMENT: the first parameter MUST be 'self' (or 'var self')
fn calcular_area(self) -> Float {
    return self.largura * self.altura
}

fn redimensionar(var self, fator: Float) {
    self.largura *= fator
    self.altura *= fator
}

# Associates both functions as methods of Retangulo
Retangulo::[calcular_area, redimensionar]
```
- **Mandatory first parameter:** the function promoted in batch **MUST** have `self` (for a read method) or `var self` (for a mutable method) as its first parameter. Functions without `self` as the first parameter are rejected by the compiler.
- **Promotion, not copy:** the original function remains accessible as a normal free function (`calcular_area(ret)`) and gains the associated method version (`ret.calcular_area()`).
- **Mutability:** preserved from the original signature: `self` → immutable method; `var self` → mutable method.

### 3.5.2 The three path tokens
Aipo strictly divides the role of each symbol:
| Token | Role | Usage example |
|---|---|---|
| `.` | **Value** access: struct fields and method/module calls | `let x = ponto.x`, `ponto.dist()` |
| `:` | Association of **one item** in a declaration: methods and hooks | `Point:dist()`, `Point:init()` |
| `::` | **Batch** association of free functions to methods | `Point::[f1, f2]` |

### 3.6 Free functions vs. methods
- `fn` is reserved **exclusively for free functions**:
  ```text
  fn somar(a: Int, b: Int) -> Int {
      return a + b
  }
  ```
- Associated methods **DO NOT USE `fn`**:
  - `Conta:saldo() -> Int { ... }` (correct)
  - `fn Conta:saldo() -> Int { ... }` (PARSE ERROR!)
  - `Conta:fn saldo() -> Int { ... }` (PARSE ERROR!)

### 3.7 Interfaces and structural satisfaction
- Interfaces contain **only method signatures**:
  ```text
  interface Renderizavel {
      render() -> String
  }
  ```
- There is no `implements` or `satisfy` keyword. Matching is purely structural by signature.
- To formally document and validate that a struct satisfies an interface, use the `#!satisfies` directive.

### 3.7.1 The canonical directive system (`#!nome`)
Directives are comments processed by the compiler that apply to the **immediately following item**:

| Directive | Target | Effect |
|---|---|---|
| `#!satisfies I1, I2` | `struct` | Requires structural conformance with the interfaces at compile time. |
| `#!test` / `#!test[tag]` / `#!test("desc")` | free `fn` | Registers the function as a unit test for the `aipo test` runner. |
| `#!deprecated` / `#!deprecated("msg")` | `fn`, `struct` or method | Marks the element obsolete; the analyzer emits a warning diagnostic. |
| `#!todo` / `#!todo("msg")` | any item | Tracks technical debt or pending work without breaking compilation. |

```text
#!deprecated("use calcular_novo()")
fn calcular_antigo(x: Int) -> Int {
    return x * 2
}

#!todo("add support for negative numbers")
fn validar_positivo(x: Int) -> Bool {
    return x >= 0
}

#!satisfies Renderizavel
struct Botao {
    rotulo: String
}

Botao:render() -> String {
    return f"[{self.rotulo}]"
}
```

### 3.8 Control flow
```text
# Conditional
if x > 10 {
    io.println("maior")
} elif x == 10 {
    io.println("igual")
} else {
    io.println("menor")
}

# Iteration over collections (NEVER use `for`)
each item in itens {
    io.println(item)
}

# Key-value iteration
each chave, valor in dicionario {
    io.println(f"{chave} = {valor}")
}

# Counted repetition
repeat 5 as i {
    io.println(f"passo {i}")
}

# Conditional loop
while saldo > 0 {
    saldo -= 1
}

# Canonical infinite loop
loop {
    if pronto { break }
}
```

### 3.9 Pattern matching (`match` / `when`)
```text
match resposta {
    when { status, corpo } if status == 200 {
        io.println("Sucesso: " + corpo)
    }
    when { status } if status >= 400 and status < 500 {
        io.println("Erro do cliente")
    }
    when _ {
        io.println("Outro status")
    }
}
```

### 3.10 Failures and transactions
```text
# Direct failure
fn dividir(a: Int, b: Int) -> Int {
    if b == 0 {
        fail "divisão por zero"
    }
    return a // b
}

# Fallback with or_else
let porta = ler_config("porta") or_else 8080

# Propagation with the '?' operator
let resultado = operacao_arriscada()?

# Atomic capture: mutations are rolled back if a failure occurs
attempt {
    conta_origem.debitar(100)
    conta_destino.creditar(100)
} failed erro {
    # If any operation fails or violates an invariant,
    # the changes in conta_origem and conta_destino are undone automatically!
    io.println("Transação abortada: " + erro.message)
}
```

### 3.11 Modules, imports and exports
- Each `.aipo` file is an independent module.
- Everything is private by default. Symbols visible to other files **must** be declared with `export`:
  ```text
  export calcular_imposto, GeradorNota
  ```
- Importing:
  ```text
  import math_util                 # Qualified: math_util.somar()
  import math_util as mu           # With alias: mu.somar()
  import math_util: somar, subtrair # Selective: somar() directly
  ```

---

## 4. Mental conversion table (other languages → Aipo V1)

| Intent | In Python / JS / Rust | How it MUST be in Aipo V1 |
|---|---|---|
| Comment | `// texto` or `/* texto */` | `# texto` |
| Integer division | `Math.floor(a / b)` or `a // b` | `a // b` |
| Iterate list | `for item in xs:` / `for (const x of xs)` | `each item in xs { ... }` |
| Loop N times | `for i in range(5):` | `repeat 5 as i { ... }` |
| AND operator | `a && b` | `a and b` |
| OR operator | `a || b` | `a or b` |
| Negation | `!cond` | `not cond` |
| Ternary | `cond ? a : b` | `if cond then a else b` |
| Exception / error | `try { ... } catch (e) { ... }` | `attempt { ... } failed e { ... }` |
| Throw error | `throw new Error("msg")` / `raise` | `fail "msg"` |
| Null-check fallback | `val ?? default` | `val or_else default` |
| Error propagation | `op()?` in Rust | `op()?` |
| Create object/struct | `new User(1)` or `User { id: 1 }` | `User{ id: 1 }` (capitalized name required) |
| Class method | `class P { move() {} }` | `P:move() { ... }` |
| Mutating method | `fn move(&mut self)` / `this.x = 1` | `P:move(var self) { self.x = 1 }` |
| Interface / Trait | `class P implements I` / `impl I for P` | `interface I { ... }` + `#!satisfies I` structural |
| Update struct | `{ ...user, status: "ok" }` | `user with { status: "ok" }` |
