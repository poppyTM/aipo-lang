# Aipo V1 — Cheat Sheet para Prompts e LLMs (Ultra-Compacto)

> **Regra de Ouro:** Uma forma só para cada coisa. Palavra vence símbolo. Zero cerimônia.

---

## 1. Os 10 Tabus Inegociáveis (NUNCA GERE)
1. **Comentário é `#`**. NUNCA use `//` para comentário (`//` é divisão inteira!).
2. **Blocos usam `{}`**. NUNCA use `end`.
3. **Iteração é `each x in xs { }`**. NUNCA use `for`.
4. **Sem exceções tradicionais**. NUNCA use `try`, `catch`, `finally`, `throw` ou `defer`.
5. **Sem classes ou impl**. NUNCA use `class` ou `impl Tipo { }`. Métodos usam `Tipo:nome()`.
6. **Sem mutação solta em método**. Métodos mutáveis exigem `Tipo:nome(var self)`.
7. **Lógica usa palavras**. NUNCA use `&&`, `||`, `!`. Use `and`, `or`, `not`.
8. **Sem ponto e vírgula**. NUNCA use `;` no fim de statements.
9. **Batch binding exige self**. NUNCA promova via `Tipo::[...]` função sem `self` (ou `var self`) como 1º param.
10. **Ternário é `if c then a else b`**. NUNCA use `c ? a : b`.

---

## 2. Esqueleto Canônico de Código

```text
# 1. Imports no topo
import math
import utils: formatar

# 2. Exports públicos (módulo é privado por padrão)
export Conta, criar_conta

# 3. Interface (só assinaturas estruturais)
interface Notificavel {
    notificar(msg: String) -> Bool
}

# 4. Struct (apenas dados, maiúscula; tipagem de campos é opcional mas EXTREMAMENTE RECOMENDADA)
#!satisfies Notificavel
struct Conta {
    id: Int                        # Tipagem fortemente recomendada
    titular: String
    var saldo: Float = 0.0
}

# 5. Hooks de Tipo
Conta:init(id, titular) {
    self.id = id
    self.titular = titular
    self.saldo = 0.0
}

Conta:invariant {
    self.id > 0
    self.saldo >= 0.0
}

# 6. Métodos (sem 'fn'; self implícito em leitura; 'var self' para mutação)
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

# 7. Enum Canônico (tipo de soma fechado)
enum StatusConta {
    Ativa,
    Bloqueada(motivo: String),
    Encerrada,
}

# 8. Função Livre (usa 'fn')
fn criar_conta(id: Int, titular: String) -> Conta {
    return Conta{ id: id, titular: titular }
}

# 9. Promoção em Lote com '::' (exige 'self' como 1º param)
fn calcular_rendimento(self, taxa: Float) -> Float {
    return self.saldo * taxa
}
Conta::[calcular_rendimento]   # Torna chamável como conta.calcular_rendimento(taxa)

# 10. Testes com diretiva
#!test("fluxo de deposito")
fn test_deposito() {
    var c = criar_conta(1, "Ana")
    c.depositar(100.0)
    if c.consultar_saldo() != 100.0 { fail "saldo incorreto" }
}
```

---

## 3. Guia Rápido de Controle de Fluxo e Erros

| Construção | Sintaxe Aipo V1 |
|---|---|
| Declaração de enum | `enum Nome { A, B(x: T), C { y: T }, }` (vírgula final obrigatória) |
| Match em enum | `match e { when Nome.A { } when Nome.B(x) { } }` (exaustividade estrita) |
| Associação em lote | `Tipo::[f1, f2]` (promove funções que têm `self` como 1º parâmetro) |
| Tipagem em struct | `campo: Tipo` (opcional no parser, **extremamente recomendada**) |
| Diretivas do compilador | `#!test`, `#!satisfies`, `#!deprecated("msg")`, `#!todo("msg")` |
| Condicional | `if c { } elif c { } else { }` |
| Expressão inline | `let x = if cond then valor_a else valor_b` |
| Iteração de lista | `each item in lista { io.println(item) }` |
| Iteração chave-valor | `each chave, valor in mapa { ... }` |
| Repetição N vezes | `repeat 5 as i { io.println(i) }` |
| Laço condicional | `while c { }` ou laço infinito `loop { break }` |
| Pattern Matching | `match val { when { id } if id > 0 { } else { } }` |
| Disparar falha | `fail "mensagem de erro"` |
| Fallback seguro | `let porta = ler_env("PORT") or_else 8080` |
| Propagar falha | `let usuario = buscar_usuario(id)?` |
| Bloco transacional | `attempt { acao1() acao2() } failed err { log(err.message) }` |
| Cópia com alteração | `let alterado = usuario with { status: "ativo" }` |
| Divisão inteira | `let inteiro = 7 // 2` (resultado: 3) |
| Divisão real | `let real = 7 / 2` (resultado: 3.5, sempre Float) |
