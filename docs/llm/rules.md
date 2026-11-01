# Aipo — Regras Gramaticais Positivas e Negativas (Guia para LLMs)

Este documento estabelece o contrato estrito de conformidade sintática para modelos de linguagem. Qualquer violação destas regras produz erro de análise léxica ou sintática (`parse error`) no compilador Aipo V1.

---

## 1. As 4 Regras Cardeais da Linguagem

1. **Uma forma só para cada coisa:** Nunca existem duas sintaxes para a mesma operação.
2. **Palavra completa vence símbolo:** `and` em vez de `&&`, `not` em vez de `!`, `or` em vez de `||`.
3. **Zero cerimônia para o caso comum:** Blocos usam `{}` uniformemente. Sem cabeçalhos ou anotações redundantes.
4. **O erro diz o que fazer:** Diagnósticos apontam a linha, a causa e a correção esperada.

---

## 2. Inventário Negativo — O que NÃO EXISTE (Proibições Absolutas)

A lista a seguir contém palavras, símbolos e construções comuns em outras linguagens que **NÃO EXISTEM** em Aipo V1. Emitir qualquer um destes itens é falha de geração.

### 2.1 Palavras-chave Broidas / Inexistentes

| Proibido na LLM | Motivo / Causa da Alucinação | Forma Correta em Aipo V1 |
|---|---|---|
| `// comentário` | Viés de C/JS/Rust/Go. Em Aipo, `//` é divisão inteira! | `# comentário até o fim da linha` |
| `/* bloco */` | Não existe comentário de bloco. | Repita `#` em cada linha. |
| `end` | Removido na V1 (ADR-001). Blocos usam `{}`. | `}` para fechar qualquer bloco. |
| `for ... in ...` | Viés de Python/JS/Rust. | `each item in xs { ... }` |
| `for (i=0; ...)` | Viés de C/JS. | `repeat N as i { ... }` ou `while` |
| `try { } catch` | Viés de JS/Java/Python/C#. | `attempt { ... } failed erro { ... }` |
| `throw` / `raise` | Viés de JS/Python. | `fail "mensagem"` ou `return fail(...)` |
| `finally` | Viés de linguagens orientadas a exceções. | Sem equivalente. O bloco `attempt` possui rollback atômico. |
| `defer` | Viés de Go/Zig/Swift. | Não existe. Recursos e estado usam journal transacional. |
| `class` | Viés de OOP tradicional. | `struct Nome { ... }` + `Nome:metodo() { ... }` |
| `impl Nome { }` | Removido na V1. Métodos são declarados individualmente. | `Nome:metodo() { ... }` no topo do arquivo. |
| `satisfy` | Removido na V1. Satisfação é 100% estrutural. | Remova. Opcionalmente use diretiva `#!satisfies I1`. |
| `self!` | Removido na V1. | `var self` no primeiro parâmetro do método mutável. |
| `div` / `div=` | Removido na V1. | `//` para divisão inteira e `//=` para atribuição. |
| `const` | Viés de JS/C/Rust. | `let` (bindings `let` já são imutáveis por padrão). |
| `pub` / `public` / `private` | Viés de Rust/Java/TS. Visibilidade é por módulo. | Tudo é privado por padrão. Exponha com `export nome1, nome2`. |
| `switch` / `case` | Viés de C/JS/Go. | `match alvo { when padrao { ... } else { ... } }` |
| `where` | Viés de Rust/SQL. | Não existe cláusula `where`. Use guards: `when p if cond { }`. |
| `void` / `nil` / `null` | Viés de C/Go/JS/Python. | `none` é o valor unitário / nulo canônico. |
| `lambda` / `function` | Viés de Python/JS. | `(x) => expr` ou `fn(x) { ... }` |
| `import.types` | Token pontilhado inválido no parser. | `import types` (com espaço). |

### 2.2 Operadores Proibidos / Inexistentes

| Símbolo Proibido | Equivalente Canônico em Aipo V1 |
|---|---|
| `&&` | `and` |
| `||` | `or` |
| `!` (negação lógica) | `not` (ex: `not ativo`, `not (x > 0)`) |
| `++` / `--` | `x += 1` / `x -= 1` |
| `;` (ponto e vírgula) | Newline termina statements. Proibido ponto e vírgula. |
| `?:` (operador ternário) | `if c then a else b` |
| `a < b < c` (chained comparison) | `a < b and b < c` (ou açúcar de elisão: `v >= 0 and <= 100`) |
| `is not T` | `not v is T` |
| `x as Tipo` (cast) | `Tipo(x)` (ex: `Int(3.14)`, `String(42)`). `as` só é usado em `import ... as` e `repeat N as i`. |
| `@decorador` | Não existe decorador com `@`. Diretivas usam `#!nome`. |

---

## 3. Regras Positivas — Como Escrever Código Canônico

### 3.1 Blocos e Indentação
- **Regra:** Todo bloco abre com `{` e fecha com `}`.
- **Regra do `then`:** 
  - Bloco multilinha: **NUNCA** use `then`. Exemplo: `if ativo { ... }`.
  - Expressão inline de valor: **SEMPRE** use `then`. Exemplo: `let x = if c then 1 else 2`.

### 3.2 Comentários e Divisão Inteira
- `#` inicia comentário de linha única até `\n`.
- `//` é o operador aritmético de **divisão inteira truncada a zero** (`7 // 2` resulta em `3`).

### 3.3 Variáveis e Mutabilidade
- `let x = 10`: Ligação imutável (padrão).
- `var x = 10`: Ligação mutável.
- Desestruturação: `let [a, b] = lista`, `let {nome, idade} = pessoa`.
- Descarte: `_` descarta valor indesejado.

### 3.4 Structs (Apenas Dados)
```text
struct Usuario {
    id: Int                        # Imutável por padrão
    nome: String                   # Imutável por padrão
    var status: String = "ativo"   # Mutável com valor padrão
    var tentativas: Int = 0        # Mutável com valor padrão
}
```
- **Tipagem de campos:** A anotação de tipo nos campos de struct é **opcional**, mas **EXTREMAMENTE RECOMENDADA**. Deixar campos sem tipo (`id` em vez de `id: Int`) é tolerado em protótipos rápidos, mas é anti-padrão de engenharia em código de produção e respostas de LLMs.
- Structs contêm **apenas campos**. NUNCA coloque métodos dentro de `struct { }`.
- Construção canônica: `Usuario{ id: 1, nome: "Ana" }`. O nome do tipo **DEVE** começar com letra maiúscula.
- Atualização funcional (cópia modificada): `let u2 = u1 with { status: "inativo" }`.

### 3.4.1 Enums (Tipos de Soma Fechados — §17)
O `enum` representa alternativas fechadas ("isto é A **ou** B") com checagem de **exaustividade obrigatória** pelo compilador.

```text
enum EstadoConexao {
    Desconectado,
    Tentando(tentativa: Int),
    Conectado { ip: String, ping_ms: Int },
    Erro(mensagem: String),
}
```

Três formas de variante são suportadas:
1. **Sem payload:** Valor constante (`Desconectado`).
2. **Payload nomeado:** Campos rotulados (`Conectado { ip: String, ping_ms: Int }`).
3. **Payload posicional:** Um valor único entre parênteses (`Tentando(tentativa: Int)`).

**Regras gramaticais:**
- Vírgula obrigatória entre variantes e vírgula final obrigatória antes de `}`.
- Construção direta com qualificação pelo tipo: `let e = EstadoConexao.Conectado { ip: "127.0.0.1", ping_ms: 10 }`.
- **Exaustividade no `match`:** Um `match` sobre enum **sem** cláusula `else` DEVE cobrir obrigatoriamente todas as variantes do tipo, sob pena de erro de compilação.
- Enums aceitam métodos (`EstadoConexao:metodo()`) e hooks (`EstadoConexao:invariant`).

### 3.5 Métodos e Hooks de Tipos
Métodos são associados fora da struct ou enum usando a sintaxe `Tipo:nome`:
```text
# Hook de inicialização (executado na construção)
Usuario:init(id, nome) {
    self.id = id
    self.nome = nome
}

# Hook de invariante (predicado testado a cada mutação de estado)
Usuario:invariant {
    self.id > 0
    len(self.nome) > 0
}

# Método imutável: `self` é implícito (NÃO use `fn`)
Usuario:descricao() -> String {
    return f"#{self.id}: {self.nome} ({self.status})"
}

# Método mutável: `var self` é obrigatório no receptor
Usuario:desativar(var self) {
    self.status = "inativo"
}
```

### 3.5.1 Promoção e Associação em Lote com `::`
Quando funções livres já existem, elas podem ser promovidas em lote para métodos de um tipo através do operador `::`:
```text
# EXIGÊNCIA OBRIGATÓRIA: o primeiro parâmetro DEVE ser 'self' (ou 'var self')
fn calcular_area(self) -> Float {
    return self.largura * self.altura
}

fn redimensionar(var self, fator: Float) {
    self.largura *= fator
    self.altura *= fator
}

# Associa ambas as funções como métodos de Retangulo
Retangulo::[calcular_area, redimensionar]
```
- **Primeiro Parâmetro Obrigatório:** A função a ser promovida em lote **DEVE** ter `self` (para método de leitura) ou `var self` (para método mutável) como seu primeiro parâmetro. Funções sem `self` no primeiro parâmetro são rejeitadas pelo compilador.
- **Promoção, não cópia:** A função original continua acessível como função livre normal (`calcular_area(ret)`) e ganha a versão associada como método (`ret.calcular_area()`).
- **Mutabilidade:** Preservada da assinatura original: `self` $\rightarrow$ método imutável; `var self` $\rightarrow$ método mutável.

### 3.5.2 Os Três Tokens de Caminho
Aipo divide com rigor o papel de cada símbolo:
| Token | Papel | Exemplo de Uso |
|---|---|---|
| `.` | Acesso a **valor**: campos de struct e chamadas de métodos/módulos | `let x = ponto.x`, `ponto.dist()` |
| `:` | Associação de **um item** na declaração: métodos e hooks | `Point:dist()`, `Point:init()` |
| `::` | Associação em **lote** de funções livres para métodos | `Point::[f1, f2]` |

### 3.6 Funções Livres vs. Métodos
- `fn` é reservado **exclusivamente para funções livres**:
  ```text
  fn somar(a: Int, b: Int) -> Int {
      return a + b
  }
  ```
- Métodos associados **NÃO USAM `fn`**:
  - `Conta:saldo() -> Int { ... }` (Correto)
  - `fn Conta:saldo() -> Int { ... }` (ERRO DE PARSE!)
  - `Conta:fn saldo() -> Int { ... }` (ERRO DE PARSE!)

### 3.7 Interfaces e Satisfação Estrutural
- Interfaces contêm **apenas assinaturas de métodos**:
  ```text
  interface Renderizavel {
      render() -> String
  }
  ```
- Não há palavra `implements` ou `satisfy`. A correspondência é puramente estrutural por assinatura.
- Para documentar e validar formalmente que uma struct satisfaz uma interface, use a diretiva `#!satisfies`.

### 3.7.1 O Sistema Canônico de Diretivas (`#!nome`)
Diretivas são comentários processados pelo compilador que se aplicam ao **próximo item** imediatamente declarado:

| Diretiva | Alvo | Efeito |
|---|---|---|
| `#!satisfies I1, I2` | `struct` | Exige conformidade estrutural com as interfaces em tempo de compilação. |
| `#!test` / `#!test[tag]` / `#!test("desc")` | `fn` livre | Registra a função como teste unitário para o runner `aipo test`. |
| `#!deprecated` / `#!deprecated("msg")` | `fn`, `struct` ou método | Marca elemento obsoleto; analisador emite diagnóstico de aviso. |
| `#!todo` / `#!todo("msg")` | qualquer item | Rastreia débito técnico ou pendência sem quebrar a compilação. |

```text
#!deprecated("use calcular_novo()")
fn calcular_antigo(x: Int) -> Int {
    return x * 2
}

#!todo("adicionar suporte a números negativos")
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

### 3.8 Controle de Fluxo
```text
# Condicional
if x > 10 {
    io.println("maior")
} elif x == 10 {
    io.println("igual")
} else {
    io.println("menor")
}

# Iteração sobre coleções (NUNCA use `for`)
each item in itens {
    io.println(item)
}

# Iteração chave-valor
each chave, valor in dicionario {
    io.println(f"{chave} = {valor}")
}

# Repetição contada
repeat 5 as i {
    io.println(f"passo {i}")
}

# Laço condicional
while saldo > 0 {
    saldo -= 1
}

# Laço infinito canônico
loop {
    if pronto { break }
}
```

### 3.9 Pattern Matching (`match`)
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
- Braços usam `when`, **nunca** `case`.
- Suporta guardas condicionais com `if <expr-bool>`.
- Braço padrão é `else { ... }` ou `when _ { ... }`.

### 3.10 Falhas e Tratamento Transacional
Aipo não usa exceções nem call stacks não controlados. O modelo é baseado em `Failure` com journal transacional:
```text
# Disparar falha
fn dividir(a: Int, b: Int) -> Int {
    if b == 0 {
        fail "divisão por zero"
    }
    return a // b
}

# Fallback com or_else
let porta = ler_config("porta") or_else 8080

# Propagação com o operador '?'
let resultado = operacao_arriscada()?

# Captura com reversão automática de mutações
attempt {
    conta_origem.debitar(100)
    conta_destino.creditar(100)
} failed erro {
    # Se qualquer operação falhar ou violar invariant,
    # as alterações em conta_origem e conta_destino são desfeitas automaticamente!
    io.println("Transação abortada: " + erro.message)
}
```

### 3.11 Módulos, Importação e Exportação
- Cada arquivo `.aipo` é um módulo independente.
- Tudo é privado por padrão. Símbolos visíveis para outros arquivos **devem** ser declarados com `export`:
  ```text
  export calcular_imposto, GeradorNota
  ```
- Importação:
  ```text
  import math_util                 # Qualificado: math_util.somar()
  import math_util as mu           # Com alias: mu.somar()
  import math_util: somar, subtrair # Seletivo: somar() direto
  ```

---

## 4. Tabela de Conversão Mental (Outras Linguagens $\rightarrow$ Aipo V1)

| Intenção | Em Python / JS / Rust | Como DEVE ser em Aipo V1 |
|---|---|---|
| Comentário | `// texto` ou `/* texto */` | `# texto` |
| Divisão inteira | `Math.floor(a / b)` ou `a // b` | `a // b` |
| Iterar lista | `for item in xs:` / `for (const x of xs)` | `each item in xs { ... }` |
| Loop N vezes | `for i in range(5):` | `repeat 5 as i { ... }` |
| Operador E | `a && b` | `a and b` |
| Operador OU | `a || b` | `a or b` |
| Negação | `!cond` | `not cond` |
| Ternário | `cond ? a : b` | `if cond then a else b` |
| Exceção / Erro | `try { ... } catch (e) { ... }` | `attempt { ... } failed e { ... }` |
| Lançar erro | `throw new Error("msg")` / `raise` | `fail "msg"` |
| Null check fallback | `val ?? default` | `val or_else default` |
| Propagação de erro | `op()?` em Rust | `op()?` |
| Criar objeto/struct | `new User(1)` ou `User { id: 1 }` | `User{ id: 1 }` (Capitalizado obrigatório) |
| Método de classe | `class P { move() {} }` | `P:move() { ... }` |
| Método que muta | `fn move(&mut self)` / `this.x = 1` | `P:move(var self) { self.x = 1 }` |
| Interface / Trait | `class P implements I` / `impl I for P` | `interface I { ... }` + `#!satisfies I` estrutural |
| Atualizar struct | `{ ...user, status: "ok" }` | `user with { status: "ok" }` |
