# Aipo V1 — Cheat sheet for prompts and LLMs (ultra-compact)

> **Golden rule:** one form per thing. Words beat symbols. Zero ceremony.

---

## 1. The 10 non-negotiable taboos (NEVER GENERATE)
1. **Comments are `#`**. NEVER use `//` for a comment (`//` is integer division!).
2. **Blocks use `{}`**. NEVER use `end`.
3. **Iteration is `each x in xs { }`**. NEVER use `for`.
4. **No traditional exceptions**. NEVER use `try`, `catch`, `finally`, `throw` or `defer`.
5. **No classes or impl**. NEVER use `class` or `impl Tipo { }`. Methods use `Tipo:nome()`.
6. **No loose mutation in a method**. Mutable methods require `Tipo:nome(var self)`.
7. **Logic uses words**. NEVER use `&&`, `||`, `!`. Use `and`, `or`, `not`.
8. **No semicolons**. NEVER use `;` at the end of statements.
9. **Batch binding requires self**. NEVER promote through `Tipo::[...]` a function without `self` (or `var self`) as the 1st param.
10. **Ternary is `if c then a else b`**. NEVER use `c ? a : b`.

---

## 2. Canonical code skeleton

```text
# 1. Imports at the top
import math
import utils: formatar

# 2. Public exports (module is private by default)
export Conta, criar_conta

# 3. Interface (structural signatures only)
interface Notificavel {
    notificar(msg: String) -> Bool
}

# 4. Struct (data only, capital letter; field typing optional but EXTREMELY RECOMMENDED)
#!satisfies Notificavel
struct Conta {
    id: Int                        # Strongly recommended typing
    titular: String
    var saldo: Float = 0.0
}

# 5. Type hooks
Conta:init(id, titular) {
    self.id = id
    self.titular = titular
    self.saldo = 0.0
}

Conta:invariant {
    self.id > 0
    self.saldo >= 0.0
}

# 6. Methods (no 'fn'; implicit self on read; 'var self' for mutation)
Conta:consultar_saldo() -> Float {
    return self.saldo
}

Conta:depositar(var self, valor: Float) {
    if valor <= 0.0 { fail "valor deve ser positivo" }
    self.saldo += valor
}

Conta:notificar(msg: String) -> Bool {
    io.println(f"Conta #{self.id}: {msg}")
    return true
}

# 7. Canonical enum (closed sum type)
enum StatusConta {
    Ativa,
    Bloqueada(motivo: String),
    Encerrada,
}

# 8. Free function (uses 'fn')
fn criar_conta(id: Int, titular: String) -> Conta {
    return Conta{ id: id, titular: titular }
}

# 9. Batch promotion with '::' (requires 'self' as 1st param)
fn calcular_rendimento(self, taxa: Float) -> Float {
    return self.saldo * taxa
}
Conta::[calcular_rendimento]   # Becomes callable as conta.calcular_rendimento(taxa)

# 10. Tests with a directive
#!test("deposit flow")
fn test_deposito() {
    var c = criar_conta(1, "Ana")
    c.depositar(100.0)
    if c.consultar_saldo() != 100.0 { fail "saldo incorreto" }
}
```

---

## 3. Quick reference for control flow and errors

| Construct | Aipo V1 syntax |
|---|---|
| Enum declaration | `enum Nome { A, B(x: T), C { y: T }, }` (trailing comma required) |
| Match on enum | `match e { when Nome.A { } when Nome.B(x) { } }` (strict exhaustiveness) |
| Batch association | `Tipo::[f1, f2]` (promotes functions with `self` as 1st parameter) |
| Typing in struct | `campo: Tipo` (optional in the parser, **strongly recommended**) |
| Compiler directives | `#!test`, `#!satisfies`, `#!deprecated("msg")`, `#!todo("msg")` |
| Conditional | `if c { } elif c { } else { }` |
| Inline expression | `let x = if cond then valor_a else valor_b` |
| List iteration | `each item in lista { io.println(item) }` |
| Key-value iteration | `each chave, valor in mapa { ... }` |
| Repeat N times | `repeat 5 as i { io.println(i) }` |
| Conditional loop | `while c { }` or infinite loop `loop { break }` |
| Pattern matching | `match val { when { id } if id > 0 { } else { } }` |
| Raise a failure | `fail "mensagem de erro"` |
| Safe fallback | `let porta = ler_env("PORT") or_else 8080` |
| Propagate failure | `let usuario = buscar_usuario(id)?` |
| Transactional block | `attempt { acao1() acao2() } failed err { log(err.message) }` |
| Copy with changes | `let alterado = usuario with { status: "ativo" }` |
| Integer division | `let inteiro = 7 // 2` (result: 3) |
| Real division | `let real = 7 / 2` (result: 3.5, always Float) |
