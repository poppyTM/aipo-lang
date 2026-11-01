---
name: lang-aipo
description: Aipo V1 canonical engineering, ADR-001 syntax conformance, braces blocks, Tipo:nome method bindings, structural interfaces, transactional failure handling, and anti-hallucination guardrails for LLMs.
---

# Aipo V1 Canonical Engineering & Zero-Hallucination Contract

Esta skill governa a leitura, escrita, refatoração e revisão de código na linguagem **Aipo V1** conforme a especificação canônica normativa ([`SYNTAX.md`](../../SYNTAX.md) e [`ADR-001`](../../docs/decisions/adr-001-canonical-syntax.md)).

---

## 1. As 4 Regras Cardeais Inegociáveis

1. **Uma forma só para cada coisa:** Nunca existem duas sintaxes para a mesma operação.
2. **Palavra completa vence símbolo:** `and` sobre `&&`, `not` sobre `!`, `or` sobre `||`.
3. **Zero cerimônia para o caso comum:** Todo bloco abre com `{` e fecha com `}`.
4. **O erro diz o que fazer:** Diagnósticos apontam a causa e sugerem a correção imediata.

---

## 2. Barreira Anti-Alucinação (Tokens Proibidos)

Modelos de linguagem treinados em C, JS, Python e Rust sofrem de fortes vieses probabilísticos. Antes de emitir qualquer código, aplique a seguinte censura léxica:

| Token / Padrão Proibido | Gravidade | Substituto Obrigatório |
|---|---|---|
| `// comentário` | **FATAL** (`//` é divisão inteira!) | `# comentário até o fim da linha` |
| `/* comentário de bloco */` | **FATAL** | `# comentário de linha` |
| `end` | **FATAL** (removido no ADR-001) | `}` |
| `for item in lista` | **FATAL** | `each item in lista { ... }` |
| `repeat N:` ou `for i in range(N)` | **FATAL** | `repeat N as i { ... }` |
| `try { } catch` / `finally` | **FATAL** | `attempt { ... } failed erro { ... }` |
| `throw` / `raise` | **FATAL** | `fail "mensagem"` ou `return fail(...)` |
| `defer` | **FATAL** | Transacional com journal em `attempt` |
| `class Nome { }` | **FATAL** | `struct Nome { ... }` + `Nome:metodo()` |
| `impl Nome { }` | **FATAL** (removido no ADR-001) | `Nome:metodo() { ... }` |
| `satisfy Tipo: Interface` | **FATAL** (removido no ADR-001) | Satisfação estrutural + `#!satisfies Interface` |
| `self!` | **FATAL** (removido no ADR-001) | `var self` |
| `div` / `div=` | **FATAL** (removido no ADR-001) | `//` e `//=` |
| Promoção `::` sem `self` | **FATAL** | O primeiro parâmetro da função deve ser `self` ou `var self` |
| `&&`, `||`, `!` | **FATAL** | `and`, `or`, `not` |
| `;` no fim de statement | **FATAL** | Quebra de linha termina statement |
| `c ? a : b` | **FATAL** | `if c then a else b` |
| `import io` / `import math` / `import task` | **FATAL** | Módulos de prelude já estão visíveis: `import io` é `AIPO_SEM_UNKNOWN_MODULE` |
| `sys.args()` / `sys.exit()` / `sys.env()` | **FATAL** | Módulo `sys` não existe. Ambiente é `env.get` / `env.has` (por capability) |
| `fn main() { … }` como ponto de entrada na VM | **FATAL** | Não existe `main` implícito no VM: o **topo do módulo** é o script (só o runner WebAssembly usa `main` quando existe) |
| `return match e { … }` | **FATAL** | `match` é statement: atribua a uma variável e retorne-a |
| `await do { … }` como inicializador | **FATAL** | `await do { … }` é statement (o `return` dentro dele sai da função externa) |
| `f(var x)` no argumento da chamada | **FATAL** | `var` é forma de parâmetro na *declaration*; no argumento passe o valor direto |

---

## 3. Padrões Sintáticos Canônicos

### 3.1 Struct e Métodos
```text
# Struct contém APENAS dados. Campos imutáveis por padrão.
# Tipagem nos campos é opcional no compilador, mas EXTREMAMENTE RECOMENDADA para LLMs.
struct Servidor {
    id: Int                        # Tipagem fortemente recomendada
    var status: String = "online"
    var carga: Float = 0.0
}

# Hooks reconhecidos pelo nome:
Servidor:init(id: Int) {
    self.id = id
    self.status = "online"
    self.carga = 0.0
}

Servidor:invariant {
    self.id > 0
    self.carga >= 0.0
}

# Método de leitura: self implícito, NUNCA usa 'fn'
Servidor:esta_livre() -> Bool {
    return self.status == "online" and self.carga < 0.8
}

# Método mutável: 'var self' obrigatório no receptor
Servidor:atribuir_carga(var self, incremento: Float) {
    if incremento <= 0.0 { fail "incremento deve ser positivo" }
    self.carga += incremento
}
```

### 3.1.1 Enum (Tipo de Soma Fechado — §17)
```text
enum EstadoConexao {
    Desconectado,
    Tentando(tentativa: Int),
    Conectado { ip: String, ping_ms: Int },
    Erro(motivo: String),
}

# 'match' é STATEMENT, nunca expressão: os braços atribuem/corpo próprio.
# `return match …` NÃO existe (ver §17.3 do SYNTAX.md).
# Match exaustivo: sem 'else' exige cobrir todas as variantes
fn avaliar(e: EstadoConexao) -> String {
    var rotulo = "desconhecido"
    match e {
        when EstadoConexao.Desconectado {
            rotulo = "offline"
        }
        when EstadoConexao.Tentando(tentativa) {
            rotulo = f"tentativa #{tentativa}"
        }
        when EstadoConexao.Conectado { ip, ping_ms } {
            rotulo = f"{ip}:{ping_ms}"
        }
        when EstadoConexao.Erro(motivo) {
            rotulo = motivo
        }
    }
    return rotulo
}
```

### 3.2 Funções Livres vs. Métodos
- Funções sem tipo receptor **devem** usar `fn`:
  ```text
  fn formatar_id(id: Int) -> String {
      return f"ID-{id}"
  }
  ```
- Métodos associados **nunca** usam `fn`: `Servidor:esta_livre()` ✓.

### 3.2.1 Associação em Lote com `::` (Batch Promotion)
Promove funções livres existentes a métodos de um tipo:
```text
# EXIGÊNCIA: O primeiro parâmetro DEVE ser 'self' ou 'var self'
fn desligar(var self) {
    self.status = "offline"
}

# Promove 'desligar' para método de Servidor
Servidor::[desligar]
```
- Funções sem `self` como 1º parâmetro são rejeitadas pelo compilador.
- `.` acessa valores (`s.status`, `s.desligar()`).
- `:` associa uma declaração de método/hook (`Servidor:init`).
- `::` associa uma lista de funções em lote (`Servidor::[f1, f2]`).

### 3.2.2 Diretivas do Compilador (`#!nome`)
Diretivas aplicam-se ao item imediatamente seguinte:
- `#!satisfies Interface`: Valida satisfação de interface estrutural em structs.
- `#!test`: Marca função livre como teste unitário (`#!test`, `#!test[tag]`, `#!test("nome")`).
- `#!deprecated("msg")`: Marca função/struct/método como obsoleto (emite warning de uso).
- `#!todo("msg")`: Rastreia pendência técnica associada ao item.

### 3.3 Tratamento Transacional de Falhas
```text
# Falha simples ou com fallback
let porta = ler_porta() or_else 8080

# Propagação com '?'
let config = carregar_config()?

# Transação atômica: mutações são revertidas se ocorrer fail
attempt {
    servidor_a.transferir(servidor_b, 50)
} failed erro {
    io.println("Falha na transferência: " + erro.message)
}
```

---

## 4. Workflow de Implementação da LLM

Ao gerar ou editar código Aipo:

1. **Revisar Tabus:** Garantir que nenhum token da Seção 2 esteja presente no rascunho mental.
2. **Checar Comentários:** Confirmar que todos os comentários usam `#` e nunca `//`.
3. **Checar Métodos:** Confirmar que `Tipo:nome` não possui `fn` e que receptores mutáveis usam `var self`.
4. **Checar Laços:** Confirmar que listas usam `each` e contagens usam `repeat`.
5. **Checar Saída com o Linter:** Se o ambiente permitir execução de comandos, rodar `python skills/lang-aipo/scripts/lint_aipo.py <arquivo>`.
6. **Checar Saída com o Compilador:** O linter é apenas lexical — ele **não** valida gramática nem biblioteca padrão. Quando o compilador estiver disponível, rode `aipo check <arquivo>` (e `aipo test`/`aipo run`). `AIPO_BIN=<caminho> bash scripts/verify.sh` roda linter + compilador em todos os exemplos canônicos.
