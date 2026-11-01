# Aipo — Referência Canônica de Sintaxe

**Status:** normative — sintaxe canônica da Aipo V1
**Norma:** [`docs/decisions/adr-001-canonical-syntax.md`](docs/decisions/adr-001-canonical-syntax.md)
**Escopo:** superfície sintática da Aipo V1 conforme o ADR-001
**Estado do compilador:** implementado. As formas antigas (`end`, `div`, `self!`) foram removidas.
`Tipo:nome`, binding em lote `Tipo::[...]`, diretivas `#!satisfies`, `#!test` e `enum` estão plenamente integrados.
A tabela de evolução e compatibilidade está em [§16](#16-divergência-com-o-compilador-atual).

Aipo é projetada para leitura, escrita e manutenção por pessoas neurodivergentes
(TDAH, dislexia, sobrecarga cognitiva). Quatro regras governam toda a superfície:

1. **Uma forma só para cada coisa** — nunca duas grafias para a mesma operação.
2. **Palavra completa vence símbolo** — `and` sobre `&&`, `not` sobre `!`.
3. **Zero cerimônia para o caso comum** — o que aparece em todo arquivo é curto.
4. **O erro diz o que fazer** — aponta a posição, nomeia o problema, oferece a correção.

---

## 0. TL;DR

- **42 keywords + `_` (descarte) = 43 tokens reservados.** Removidas: `end`, `div`, `self!`, `impl`, `satisfy`; `fixed` permanece como forma legada aceita.
- Bloco abre com `{` e fecha com `}`. **Não existe `end`.**
- Comentário é `#`. Não existe comentário de bloco.
- **`//` NÃO é comentário** — é divisão inteira. Use `#`.
- Método pertence a um tipo por `Tipo:nome`. `self` é implícito; `var self` declara mutação.
- `fn` é só para função **livre**. `interface` satisfaz por **tipagem estrutural** — sem declaração.
- `#!` introduz **diretivas**: `#!satisfies`, `#!test`, `#!deprecated`, `#!todo`.

---

## 1. Terminação de bloco

Toda abertura usa `{`, todo fechamento usa `}`.

```aipo
let ativo = true
if ativo {
    io.println("ok")
}
```

Aplica-se a: `fn`, `async fn`, `struct`, `interface`, `if`/`elif`/`else`,
`match` (cada braço `when`), `loop`, `while`, `repeat`, `each`,
`attempt`/`failed`, `await do`, bloco trailer `do`.

### 1.1 `then`

| Forma | `then` |
|---|---|
| Bloco — `if c { }` | **nunca** |
| Inline com valor — `if c then a else b` | **sempre** |

Regra: tem `{}`? Sem `then`. Não tem `{}`? Com `then`.

---

## 2. Comentários e diretivas

| Item | Regra |
|---|---|
| Comentário de linha | `#` até o fim da linha — **único** comentário |
| Comentário de bloco | não existe |
| Diretiva | `#!nome` — lida pelo compilador, aplica-se ao **próximo item** |
| Newline | **significativo** — termina statement; newlines consecutivos colapsam em um |
| Espaços | espaço, tab e CR ignorados |
| BOM | removido; CRLF/CR normalizados para LF |
| Identificadores | começam com `_` ou letra Unicode; continuam com `_` ou alfanumérico Unicode |
| NFC | aplicado ao **conteúdo de literais de string**, não a identificadores |

### 2.1 A armadilha do `//`

`//` é divisão inteira, não comentário — colide com o hábito formado em JavaScript,
Java, C, C++, Rust, Go, C#, PHP e Swift.

Mitigação: quando `//` aparece onde nenhuma expressão válida pode segui-lo, o
diagnóstico sugere `#`.

### 2.2 Diretivas

| Diretiva | Posição | Efeito |
|---|---|---|
| `#!satisfies I1, I2` | antes de `struct` | Exige que o tipo satisfaça as interfaces. Erro se falhar. |
| `#!test` | antes de `fn` livre | Marca a função como teste unitário. |
| `#!test[tag]` | idem | Teste com etiqueta, filtrável por `aipo test --filter tag`. |
| `#!test("nome")` | idem | Teste com nome descritivo explícito. |
| `#!deprecated` / `#!deprecated("msg")` | antes de `fn`, `struct` ou método | Marca o item como obsoleto; analisador emite aviso no uso. |
| `#!todo` / `#!todo("msg")` | antes de qualquer item | Anota débito técnico ou pendência rastreável por ferramentas. |

```aipo
interface Drawable {
    area() -> Float
}

#!deprecated("use Circle{ raio: Float }")
struct CirculoAntigo {
    r: Float
}

#!todo("otimizar para evitar raiz quadrada desnecessária")
fn distancia(x1: Float, y1: Float, x2: Float, y2: Float) -> Float {
    let dx = x2 - x1
    let dy = y2 - y1
    return math.sqrt(dx * dx + dy * dy)
}

#!satisfies Drawable
struct Circle {
    raio: Float
}

Circle:area() -> Float {
    return 3.14159 * self.raio * self.raio
}

#!test[geometry]
fn area_de_circulo_unitario() {
    io.println(String(Circle{ raio: 1.0 }.area()))
}

#!test("divisão inteira trunca em direção a zero")
fn divide_trunca() {
    io.println(String(7 // 2))    # 3
}
```

Limites:

- A diretiva aplica-se ao **item seguinte**; diretiva sem item seguinte é erro de parse.
- No máximo uma `#!test` por função.
- `#!test` marca **funções livres**; um método `Tipo:nome` não é teste, porque precisa de `self`.
- Forma e argumento são validados: `#!test` não aceita conteúdo fora de `[...]` ou `"..."`.

### 2.3 Descoberta de testes

Duas vias, ambas ativas:

- **Convenção de arquivo:** `*_test.aipo`, `test_*.aipo`
- **Diretiva:** `#!test` dentro de qualquer `.aipo`

Um teste por arquivo deixa de ser obrigatório: funções pequenas ficam agrupadas no
arquivo do que testam.

---

## 3. Literais

### 3.1 Strings

| Forma | Nome |
|---|---|
| `"..."` | normal |
| `f"..."` | interpolada |
| `r"..."` | raw |
| `fr"..."` / `rf"..."` | raw interpolada |
| `"""..."""` | multilinha |
| `r"""..."""` / `fr"""..."""` | raw multilinha |

- Escapes (fora de raw): `\n` `\r` `\t` `\\` `\"` `\0` `\u{HEX}`
- String de linha única **não** contém newline cru
- F-string: `f"{expr}"`; `{{` e `}}` produzem brace literal; `}` isolado é erro
- Conteúdo é normalizado em **NFC** no boundary de construção

```aipo
let pasta = "logs"
let chave = "status"
let caminho = fr"C:\sistema\{pasta}\app.txt"   # C:\sistema\logs\app.txt
let json = f"{{\"k\": \"{chave}\"}}"            # {"k": "status"}
```

### 3.2 Números

| Base | Prefixo | Exemplo |
|---|---|---|
| Decimal | — | `42`, `3.14`, `1.5e-3`, `12e5` |
| Hexadecimal | `0x` / `0X` | `0xFF` → 255 |
| Binário | `0b` / `0B` | `0b101010` → 42 |
| Octal | `0o` / `0O` | `0o777` → 511 |

- `_` é separador **apenas entre dois dígitos da mesma base**: `1_000_000` ✓, `0xFF_FF` ✓
- Inválidos: `_1`, `1_`, `1__0`, `1e_5`, `0x_FF` → `AIPO_LEX_INVALID_NUMBER`
- `Int` é inteiro com sinal na faixa segura ±(2^53−1); overflow → `AIPO_RT_OVERFLOW`
- `Float` é IEEE 754 de 64 bits. **Sem `NaN` e sem `Infinity`.**

### 3.3 Literais nomeados

`true` · `false` · `none` · `_`

---

## 4. Declarações de topo

```text
fn nome(params) -> T { }
async fn nome(params) -> T { }
struct Nome { campos }
Tipo:init(params) { }
Tipo:invariant { }
Tipo:metodo(params) -> T { }
Tipo::[funcao_livre, outra]
interface Nome { assinatura(params) -> T }
import a.b.c [as alias] [: n1, n2]
export n1, n2, n3
```

| Observação | Regra |
|---|---|
| `fn` | **só** função livre; método não usa `fn` |
| `Tipo:` | associa método ou hook a um tipo |
| `interface` | só assinaturas — sem corpo, sem `fn`, sem `self` |
| `export` | apenas **nomes simples** — `export mod.Nome` é erro de parse |
| `import` | caminho pontilhado (`auth.user`), alias com `as`, seleção com `:` |
| Módulo | **um arquivo `.aipo` é um módulo.** Não há declaração de módulo |

---

## 5. Structs e contratos

```aipo
struct Servidor {
    id: Int                        # imutável por padrão
    var status: String = "online"  # var = mutável
    var carga: Float = 0.0
}
```

- Anotação de tipo no campo é **opcional**.
- Bare `x` = imutável sem default; `var x` = mutável sem default; `x = v` / `var x = v` = com default.
- Reatribuir campo imutável após construção → `AIPO_SEM_FIXED_REASSIGN`.
- `struct` contém **só dados**. Comportamento fica em `Tipo:nome`.

### 5.1 Construção

```aipo
struct Servidor {
    id: Int
    var status: String = "online"
}

let s = Servidor{ id: 1, status: "online" }   # ':' e '=' ambos aceitos
let t = Servidor{ id = 1, status = "online" }
let u = Servidor{ 1, "online" }                # posicional também
io.println(s.id)
io.println(t.id)
io.println(u.id)
```

Construção qualificada por módulo também é aceita: `mod.Servidor{ id: 1 }`.

> **Armadilha:** construção `Tipo{...}` exige **primeira letra maiúscula**.
> `ponto{...}` nunca é construção — o diagnóstico sugere capitalize.

### 5.2 Métodos e hooks

```aipo
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

| Regra | Detalhe |
|---|---|
| Receptor implícito | Não se escreve `self` em método imutável; o parser injeta. |
| Receptor mutável | `var self` **obrigatório** para mutar. `var` significa mutável em toda a linguagem. |
| Hooks | `init` e `invariant` reconhecidos pelo nome; demais nomes são métodos. |
| `fn` | Não aparece — `Tipo:` já declara que é função. |

Chamada usa `.`:

```aipo
struct Point {
    var x = 0.0
    var y = 0.0
}

Point:dist() -> Float {
    return math.sqrt(self.x * self.x + self.y * self.y)
}

Point:move(var self, dx, dy) {
    self.x += dx
    self.y += dy
}

let p = Point{ x = 3.0, y = 4.0 }
let d = p.dist()      # passa
let m = p.move(dx = 1.0, dy = 1.0)
```

`:` marca associação na **declaração**; `.` marca acesso no **uso**.

### 5.3 Binding em lote

```aipo
fn desloca(var self, dx, dy) {
    self.x += dx
    self.y += dy
}

Ponto::[desloca]
```

`::` é o token de associação **em lote**. Cada nome listado promove a função livre
a método do tipo.

| Regra | Detalhe |
|---|---|
| Primeiro parâmetro | **Obrigatoriamente `self`.** Função sem `self` na origem é erro. |
| Receptor mutável | `var self` na origem → método mutável. `self` → método read-only. |
| Forma livre | A função continua chamável como `fn`: `desloca(p, 1, 2)`. |
| Receiver obrigatório | A chamada é sempre `p.desloca(1, 2)` — sem auto-`self` escondido. |

A promoção é **adição, não movimento**: a função existe nas duas formas. É uma
**promoção**, não uma cópia.

`self` na origem é o mesmo `self` de `Tipo:nome` (§5.2) — mesma palavra, mesmo
significado. Não existe token separado para "self mutável em lote".

```aipo
struct Ponto {
    var x = 0
    var y = 0
}

fn area_de(l, a) {
    return l * a
}
```

`Ponto::[area_de]` é rejeitado com `AIPO_SEM_UNKNOWN_NAME`: `area_de` não declara
`self` como primeiro parâmetro.

**Por que a restrição:** batch existe para promover *comportamento do tipo*. Uma função
que nunca toca no receiver não é comportamento do tipo — é namespacing, e continua
função livre. Permitir deixaria um receiver injetado e morto, que é pior que rejeitar.

> **Armadilha:** ler `self` exige `self` **na assinatura**, não só no corpo.
> Um corpo que menciona `self` sem declarar o parâmetro é `AIPO_SEM_UNKNOWN_NAME`.

### 5.4 Interfaces

```aipo
interface Drawable {
    draw() -> Int
}
```

Satisfação **estrutural e automática**: um tipo satisfaz uma interface quando possui
todos os métodos com assinatura compatível. Nenhuma declaração é necessária.

Para **exigir** a relação e documentá-la, use a diretiva:

```aipo
interface Drawable {
    draw() -> Int
}

#!satisfies Drawable
struct Circle {
    raio: Float
}

Circle:draw() -> Int {
    return 1
}
```

Comparação de assinatura inclui retorno e parâmetros. Mutabilidade do receptor é
covariante:

- Interface `draw()` aceita implementação `draw(var self)` — mais forte, seguro.
- Interface `move(var self)` exige implementação mutável.

---

## 6. Statements

| Forma | Observação |
|---|---|
| `let <padrão> = expr` | imutável |
| `var <padrão> = expr` | mutável |
| `alvo = expr` | atribuição simples |
| `alvo op= expr` | `+=` `-=` `*=` `/=` `//=` `%=` |
| `alvo with { campo: valor }` | atualização funcional; devolve novo valor, base intacto |
| `if c { } elif c { } else { }` | sem parênteses na condição; usa `elif` (`else if` é rejeitado) |
| `if c then a else b` | **inline, com valor** |
| `match alvo { when p1, p2 { } else { } }` | `when`, nunca `case` |
| `loop { }` | laço infinito canônico |
| `while c { }` | sem palavra `do` |
| `repeat N [as i] { }` | `as` vincula o índice |
| `each a in xs { }` | multi-binding: `each i, v in dict { }` |
| `break` / `continue` | sem label |
| `return [expr]` | sem expr retorna `none` |
| `fail expr` | ou `return fail(...)` |
| `attempt { } failed [erro] { }` | `failed` obrigatório, binding opcional |
| `fn nome(...) { }` | função local, auto-recursão resolvida |
| `await expr` / `await do { }` | ver §9 |

Padrões de binding: `nome` · `_` · `[a, b]` · `{x, y}`

### 6.1 `match` — destructuring e guards

```aipo
struct Pessoa {
    name
    age
}

let u = Pessoa{ name: "ana", age: 70 }
match u {
    when { name, age } if age >= 65 {
        io.println("senior " + name)
    }
    when { name, age } if age >= 18 {
        io.println("adult " + name)
    }
    else {
        io.println("minor")
    }
}
```

- `when { campo, ... }` desestrutura o alvo em nomes do **escopo do braço**.
- `if guard` é opcional; guard `false` cai para o **próximo braço**, não para o `else`.
- Guard é condição: exige `Bool`.
- Extrair campo ausente é o mesmo fault de `valor.campo`.

---

## 7. Falhas

```aipo
fn ler_porta() {
    return 8080
}

fn falhar() {
    fail "mensagem"
}

fn exemplo() {
    let v = ler_porta() or_else 8080
    let w = falhar()?
    return v + w
}

attempt {
    io.println(String(exemplo()))
} failed erro {
    io.println(erro.message)
}
```

| Forma | Semântica |
|---|---|
| `expr?` | propaga `Failure` ao `attempt` mais próximo ou ao caller |
| `or_else` | avalia a direita se a esquerda disparar `fail` |
| `attempt`/`failed` | captura `Failure` de um bloco |
| `fail` | cria `Failure` |

Modelo transacional: `attempt` registra um journal e reverte as mutações em caso de
`fail` ou violação de invariante.

**Não existe:** `try`, `catch`, `finally`, `throw`, `raise`, `defer`.

---

## 8. Funções

```aipo
fn soma(a, b) -> Int { return a + b }

fn conecta(host, porta = 8080, timeout = 5000) {
    io.println(f"{host}:{porta}/{timeout}")
}

conecta("api", timeout = 1000)                 # argumento nomeado

let dobro = fn(x) { return x * 2 }             # closure com captura léxica
let triplo = x => x * 3                        # lambda curto
let soma_lambda = (a, b) => a + b
let vazio = () => 0

fn com_bloco(valor, bloco) {
    return bloco() + valor
}

io.println(com_bloco("!") do { return "hello" })
```

Formas de parâmetro: `nome` · `var nome` · `nome: T` · `var nome: T` ·
`nome = default` · `var nome: T = default` · `_`.

Duas dimensões ortogonais: **mutabilidade** (`var` ou não) × **tipo** (`: T` ou nada).

Retorno: `-> T` e `-> T?`. Sem `->`, o contrato não é declarado.

> **Armadilha:** anotação de tipo aceita **um único nome simples**.
> `fn f(x: mod.Tipo)` é erro de parse — use `fn f(x: Tipo)` e importe o módulo.

### 8.1 `fn` é marcador de função livre

```aipo
fn soma(a, b) -> Int { return a + b }    # livre → usa fn
Point:dist() -> Float { }                 # associado → sem fn
```

---

## 9. Async

```aipo
async fn etapa_1() {
    task.sleep(10)
    return 1
}

async fn etapa_2() {
    task.sleep(10)
    return 2
}

async fn buscar(recurso) {
    task.sleep(50)
    return recurso
}

async fn somar_etapas() {
    await do {
        let a = await etapa_1()
        let b = await etapa_2()
        return a + b
    }
}

let v = await buscar("usuarios")
let total = await somar_etapas()
io.println(v)
io.println(total)
```

`await` é legal em statement, inicializador e valor de retorno. Subexpressão é
`AIPO_SEM_AWAIT_IN_SUBEXPRESSION`.

Combinadores são stdlib, não sintaxe: `task.spawn`, `task.sleep`, `task.all`,
`task.race`, `task.timeout`, `task.cancel`, `task.group`.

Tempo virtual e determinístico. Espera mútua transitiva → `AIPO_RT_AWAIT_CYCLE`.

---

## 10. Módulos e pacotes

```text
import math_util                 # qualificado
import math_util as mu           # com apelido
import math_util: somar, dobro   # seletivo
export somar, dobro
```

- **Privado por padrão.** Símbolo não exportado → `AIPO_SEM_UNKNOWN_NAME`.
- `import.types` (sem espaço) é **erro de parse**. Use `import types`.
- Pacote = manifesto `aipo.toml` com coordenadas `namespace.name`.
- Dependência remota exige **commit SHA de 40 dígitos**; branch e tag são rejeitados.
- `aipo run` e `aipo build` nunca acessam a rede.

### 10.1 Emprego dos operadores de caminho

| Token | Significado |
|---|---|
| `.` | acesso a **valor**: campo de struct e membro de módulo |
| `:` | associação na **declaration**: `Point:dist()` |
| `::` | associação em **lote**: `Retangulo::[f1, f2]` |

---

## 11. Operadores e precedência

Do mais fraco ao mais forte:

| # | Operadores |
|---|---|
| 1 | `\|>` pipeline |
| 2 | `or_else` |
| 3 | `or` |
| 4 | `and` |
| 5 | `not` (prefixo) |
| 6 | `==` `!=` `<` `<=` `>` `>=` `is` / `is T?` |
| 7 | `..` (range e slice) |
| 8 | `+` `-` |
| 9 | `*` `/` `//` `%` |
| 10 | prefixo `-x` `+x` · pós-fixos `()` `[]` `.` `?.` `do` `?` `with` |

### 11.1 Notas por operador

| Operador | Semântica |
|---|---|
| `/` | **sempre** retorna `Float` |
| `//` | divisão inteira truncada em direção a zero |
| `//=` | atribuição com divisão inteira |
| `\|>` | passa o valor como **primeiro** argumento da chamada à direita |
| `or_else` | avalia a direita se a esquerda disparar `fail` |
| `?` | propaga `Failure` |
| `with` | atualização funcional de struct |
| `..` | range como valor (`0..3`) e em slice |
| `?.` | encadeável; qualquer elo `none` propaga `none` |
| `is T?` | teste de tipo anulável |

Slice aceita limite omitido: `xs[..n]` · `xs[n..]` · `xs[..]`

### 11.2 Rejeições explícitas

| Construção | Código |
|---|---|
| Comparação encadeada `a < b < c` | `AIPO_PARSE_UNEXPECTED_TOKEN` |
| `is not T` | use `not v is T` |
| Elisão de comparação **após** `or` | só funciona após `and` |

Elisão após `and` é açúcar: `v >= 0 and <= 100` → `(v >= 0) and (v <= 100)`.
Açúcar multi-sujeito em `is`: `a, b, c is Int` → os três `is Int`.

---

## 12. Keywords (42 + `_` = 43 tokens reservados)

| Grupo | Keywords |
|---|---|
| Binding | `let` `var` |
| Função | `fn` `async` `await` |
| Tipo | `struct` `interface` `enum` |
| Hook | `init` `invariant` |
| Controle | `if` `elif` `else` `then` `match` `when` `loop` `while` `repeat` `each` `in` `break` `continue` `return` |
| Falha | `fail` `or_else` `attempt` `failed` |
| Módulo | `import` `export` |
| Operador-word | `is` `not` `and` `or` `as` |
| Literal | `true` `false` `none` |
| Receptor | `self` |
| Bloco | `do` |
| Atualização funcional | `with` |
| Legado (imutabilidade explícita antiga) | `fixed` |
| Descarte | `_` |

Removidas neste ciclo: `end` · `div` · `self!` · `impl` · `satisfy`.

Tokens não-keyword novos: `:` (associação) · `::` (lote) · `//=` (composto).

> `as` **não** é cast. Aparece só em alias de `import` e em `repeat N as i`.
> `self` é palavra-chave e não pode ser identificador comum.

---

## 13. O que NÃO existe

`defer` · `try` · `catch` · `finally` · `throw` · `raise` · `for` ·
`class` · `const` · `static` · `pub` · `private` · `public` · `unsafe` ·
`yield` · `lambda` · `module` · `package` · `void` · `case` · `where` ·
generics · bitwise

Operadores inexistentes: `&&` · `||` · `++` · `--` · `**` · `&` · `;` ·
`<<` · `>>` · `~` · `^` · `@attr` · `?:` · cast com `as` · `*ref` · `->campo`

Os caracteres `@`, `&`, `;`, `&&`, `||`, `++` não são tokenizados: caem em
`unexpected character`. Um `|` isolado é erro com dica *"did you mean '|>'?"*.

**`enum` está promovido a V1** — ver §17.

---

## 14. Limites de aninhamento

| Limite | Valor | Diagnóstico |
|---|---|---|
| Profundidade de expressão | 128 | `AIPO_PARSE_NESTING_TOO_DEEP` |
| Profundidade de bloco | 64 | `AIPO_PARSE_NESTING_TOO_DEEP` |

---

## 15. Onde conferir

| Fonte | Caminho |
|---|---|
| Norma desta sintaxe | `docs/decisions/adr-001-canonical-syntax.md` |
| Tokens e keywords | `crates/aipo-lexer/src/token.rs`, `crates/aipo-lexer/src/lexer.rs` |
| Tabela de grafia | `crates/aipo-formatter/src/tokens.rs` |
| Gramática e precedência | `crates/aipo-syntax/src/parser.rs` |
| AST | `crates/aipo-ast/src/ast.rs` |
| Snapshots de saída | `docs/conformance/programs/*.stdout` |
| Código que **deve** falhar | `docs/conformance/diagnostics/*.aipo` + `.code` |
| Fonte canônica por convenção | `docs/conformance/formatting/*.expected.aipo` |

Para alterar a gramática: o corpus de conformidade é o árbitro. `*.stdout` é a
saída exata de `aipo run`; `*.code` lista um código de diagnóstico por linha e
falhar pelo motivo errado também falha.

---

## 16. Divergência com o compilador atual

Histórico e estado de transição da gramática legada para a sintaxe canônica ADR-001:

| Forma antiga / Item | Status atual | Alvo / Realizado |
|---|---|---|
| `end` como terminador | **removido** | blocos exigem `{ ... }` exclusivamente |
| `div` / `div=` | **removido** | `div` é identificador comum; `//` e `//=` são os únicos operadores de divisão inteira |
| `self!` / `nome!` | **removido** | `var self` e `var nome` exclusivos |
| `impl Tipo { }` | **rejeitado pelo parser** | `Tipo:nome` é a única forma aceita |
| `satisfy T: I` | **rejeitado pelo parser** | `#!satisfies` é a forma aceita e verificada estaticamente |
| `fixed` em campo | **legado aceito** | novo código usa campo simples (imutável por padrão); `fixed` permanece por compatibilidade |
| `fn` dentro de `interface` | **opcional** | interface aceita assinaturas com ou sem `fn` |
| Campos com anotação de tipo | **implementado** | `campo: Tipo` opcional em structs |
| `#!` diretivas (`test`, `todo`, `deprecated`, `satisfies`) | **implementado** | lexer, parser, AST, HIR, sema |
| `:` e `::` tokens | **implementado** | `:` método/hook individual, `::` binding em lote |
| Binding em lote `Tipo::[...]` | **implementado** | restrito a funções com `self` como 1º param; mutabilidade via `var self` |
| Construção qualificada `mod.Tipo{...}` | **implementado** | parser, IR, VM |
| Diagnóstico "// é divisão" | **implementado** | emite aviso amigável sugerindo `#` em statement/prefix |
| `enum` (tipos de soma fechados) | **implementado V1** | unit, struct e tuple variants, matching e verificação de exaustividade |

### 16.1 Ordem sugerida de implementação

1. **Remoções puras** — `end`, `div`/`div=`, `self!`. Sem mudança de semântica; só
   recusa de formas antigas. Actualiza fixtures e `examples/` na mesma mudança.
2. **Tokens novos** — `:`, `::`, `#!`. Parser aceita mas ainda sem semântica.
3. **`impl` → `Tipo:nome`** — a maior mudança; HIR/IR/VM/Wasm/JS juntos.
4. **`satisfy` → `#!satisfies`** — remove a declaração, adiciona a diretiva.
5. **Binding em lote** — `Tipo::[f1, f2]`.
6. **`#!test`** — descoberta por diretiva, mantendo convenção de nome.
7. **Construção qualificada** — `mod.Tipo{...}`.
8. **Diagnóstico do `//`.**
9. **`enum`** — ver §17.

---

## 17. `enum` — tipo de soma

`enum` é um **tipo fechado**: o conjunto de valores é escrito uma vez, no tipo, e o
compilador conhece todos os casos. Serve para "isto é A **ou** B", que `struct` não
expressa — `struct` é "isto tem estes campos".

### 17.1 Declaração

```aipo
enum Estado {
    Inicial,
    Ativo { desde: Int },
    Desligado(motivo: String),
    Erro,
}
```

Um item pode assumir três formas:

| Forma | Payload | Lê-se como |
|---|---|---|
| `Nome` | nenhum | valor constante |
| `Nome { a: T, b: T }` | campos nomeados | "este caso carrega estes campos" |
| `Nome(valor: T)` | um campo posicional | "este caso carrega este valor" |

Regras: vírgula obrigatória entre itens; `}` fecha; `,` final é obrigatório; variantes
vazias `{}` são erro de parse; nomes de variante seguem regra de identificador comum
(`Maiuscula` recomendado por convenção, não exigido).

### 17.2 Construção

O nome qualificado **é** o construtor:

```aipo
enum Estado {
    Inicial,
    Ativo { desde: Int },
    Desligado(motivo: String),
    Erro,
}

let a = Estado.Inicial
let b = Estado.Ativo { desde: 2024 }
let c = Estado.Desligado("manutenção")
io.println(a is Estado)
```

Sem `new`, sem `::`, sem palavra `variant`. A construção de `struct` é `Tipo{...}`;
a de `enum` é `Tipo.Variante{...}` ou `Tipo.Variante(...)`.

### 17.3 `match` — o único lugar que conhece os casos

```aipo
enum Estado {
    Inicial,
    Ativo { desde: Int },
    Desligado(motivo: String),
    Erro,
}

let e = Estado.Ativo { desde: 2024 }
var ret = 0
match e {
    when Estado.Inicial {
        ret = 0
    }
    when Estado.Ativo { desde } {
        ret = desde
    }
    when Estado.Desligado(motivo) {
        io.println(motivo)
    }
    when Estado.Erro {
        ret = -1
    }
}
io.println(ret)
```

| Regra | Detalhe |
|---|---|
| Forma do padrão | `when Tipo.Variante { campos }` — nomeia tipo **e** variante |
| Payload nomeado | `{ desde }` liga o campo à variável de mesmo nome |
| Payload posicional | `(motivo)` liga o único valor à variável `motivo` |
| Variante sem payload | `when Estado.Erro { }` — sem parênteses, sem chaves |

O `match` reusa o destructure de §6.1; a diferença é que aqui o `{ ... }` nomeia
**variante + campos**, não só campos.

### 17.4 Exhaustividade — o ganho real

`match` sobre `enum` **sem** `else` e sem todas as variantes é erro de compilação:

```text
match e {
    when Estado.Inicial { }      # erro: falta Ativo, Desligado, Erro
}
```

Este é o motivo de `enum` existir na linguagem: sem ele, cobertura de casos é
convenção; com ele, é verificada. Um `enum` fechado não tem "variante desconhecida"
em runtime — o `match` é total por construção.

### 17.5 Rejeições explícitas

| Construção | Código | Por quê |
|---|---|---|
| `enum` sem corpo | parse | tipo vazio não tem valor |
| Variante com payload sem tipo | parse | tipo fechado exige contrato |
| `match` sem `else` cobrindo tudo | sema | exhaustividade é obligation |
| Variante duplicada | sema | nome identifica o caso |
| `Estado.Inicial` sem `match`/atribuição | sema | payload sem tipo é erro |
| Enum sem `enum` (sintaxe antiga) | parse | não existe forma implícita |

### 17.6 Mutabilidade e invariantes

Variantes **não** têm `invariant` próprio: o hook pertence ao tipo, como em §5.2.

```aipo
Estado:invariant {
    self is Estado
}
```

A forma `Tipo:invariant` (§5.2) aplica-se igualmente a `enum`, porque `enum` é
um tipo de primeira classe para efeitos de hook e `match`.

### 17.7 Relação com `interface`

`enum` satisfaz uma interface como `struct`, se tiver os métodos:

```aipo
interface Descritivel {
    descrever() -> String
}

#!satisfies Descritivel
enum Estado {
    Inicial,
    Ativo { desde: Int },
}

Estado:descrever() -> String {
    return "estado"
}

io.println(Estado.Inicial.descrever())
```

O `enum` é um tipo fechado de **dados**; o comportamento vem de `Tipo:nome`, igual
ao `struct`. Os dois compartilham hooks, interfaces e batch (§5.3).

### 17.8 Reuso das formas existentes

`enum` não introduz conceito novo dePattern ou binding:

| Precisa de | Forma | Onde já existe |
|---|---|---|
| Falhar com variante | `fail Estado.Erro` | §7 |
| Transportar em `Task` | `async fn` → `Task<Estado>` | §9 |
| Serializar | `json.stringify(v)` | stdlib |

`enum` acrescenta **um** conceito: o tipo fechado com conjunto de variantes
conhecido. Todo o resto é composição.