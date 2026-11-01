# ADR-001 — Sintaxe canônica Aipo

**Status:** aceito
**Data:** 2026-10-06
**Substitui:** `fn Type.name(...)` associated syntax, `satisfy`, `impl`, `self!`, `end`, `div`
**Alvo:** Aipo V1

---

## 1. Princípio

Aipo é projetada para pessoas neurodivergentes (TDAH, dislexia, sobrecarga
cognitiva). Quatro regras governam toda decisão de superfície:

1. **Uma forma só para cada coisa.** Nunca duas grafias para a mesma operação,
   nunca escolha de estilo em tempo de escrita.
2. **Palavra completa vence símbolo.** `and` sobre `&&`, `not` sobre `!`. Símbolos
   densos colados têm silhueta parecida e causam releitura.
3. **Zero cerimônia para o caso comum.** Estruturas que aparecem em todo arquivo
   são curtas e não exigem cabeçalho.
4. **O erro diz o que fazer.** Diagnóstico aponta a posição, nomeia o problema e
   oferece a correção, em uma linha.

Não-objetivos explícitos: brevity deExpert, densidade deexpressão, compatibilidade
com outras linguagens.

---

## 2. Blocos

Toda abertura usa `{`, todo fechamento usa `}`.

```text
if cond {
    io.println("ok")
}
```

A keyword `end` é **removida**. Ela era ambígua quando três blocos aninhados
terminavam em sequência, e chaves têm contorno visual que editores highlightam
como par.

Aplica-se a: `fn`, `async fn`, `struct`, `interface`, `if`/`elif`/`else`,
`match` (cada braço `when`), `loop`, `while`, `repeat`, `each`, `attempt`/`failed`,
`await do`, bloco trailer.

### 2.1 `then`

- **Com chaves:** nunca usa `then`.
- **Sem chaves (inline, com valor):** usa `then`. É a única forma sem bloco.

```text
let rotulo = if idade >= 18 then "maior" else "menor"
```

Regra: tem `{}`? Sem `then`. Não tem `{}`? Com `then`.

---

## 3. Tipos

### 3.1 Struct — só dados

```text
struct Servidor {
    id: Int                       # imutável por padrão
    var status: String = "online" # `var` = mutável
    var carga: Float = 0.0
}
```

A anotação de tipo no campo é **opcional**. Sem ela, o campo não promete nada ao
compilador — útil em protótipo, oneroso evitado.

`invariant()` verifica regras de **valor**, não tipos: roda em runtime, após a
construção e nas fronteiras mutáveis estáveis.

### 3.2 Interface — só assinaturas

```text
interface Drawable {
    draw() -> Int
}
```

Não contém corpos, não contém `fn`, não contém `self`.

### 3.3 Satisfação estrutural

Um tipo satisfaz uma interface quando possui todos os métodos com assinatura
compatível. A satisfação é **automática** e não requer declaração.

Comparação de assinatura inclui o tipo de retorno e os tipos dos parâmetros. Para
mutabilidade do receptor, a regra é covariante:

- Interface `draw()` (imutável) aceita implementação `draw(var self)` — mais
  forte, seguro.
- Interface `move(var self)` exige implementação mutável.

---

## 4. Métodos e hooks

### 4.1 Associação por `:`

Um método pertence a um tipo pelo prefixo `Tipo:`.

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

- `init` e `invariant` são **hooks** reconhecidos pelo nome; os demais são métodos.
- **`self` implícito:** não se escreve em método imutável. O parser injeta, como já
  faz com `init` e `invariant` na gramática atual.
- **`var self` explícito:** obrigatório para mutação. Uma única regra em toda a
  linguagem: `var` significa mutável.
- `fn` **não** aparece. A associação `Tipo:` já declara que é uma função.

### 4.2 Chamada

A chamada usa `.`, igual hoje:

```text
let p = Point{x = 3, y = 4}
let d = p.dist()          # passa
let m = p.move(var dx = 1, dy = 1)
```

`:` marca **associação na declaração**; `.` marca **acesso no uso**. Um operador,
cada emprego.

### 4.3 Binding em lote por `::`

Funções livres podem ser promovidas a métodos de um tipo sem reescrever o corpo.
**Exigência:** o primeiro parâmetro deve ser `self` (ou `var self`).

```text
fn calcular_area(self) {
    return self.largura * self.altura
}

Retangulo::[calcular_area]

fn ajustar(var self, delta) {   # continua mutável
    self.largura += delta
}

Retangulo::[ajustar]           # método mutável promovido
```

O `::` é o token de **associação em lote**. Mutabilidade vem da assinatura de origem:
`self` → método read-only, `var self` → método mutável.

Funções sem `self` como primeiro parâmetro **são rejeitadas pelo compilador**: batch
existe para promover comportamento do tipo, não para namespacing.

Semântica: o binding é uma **promoção**. A função permanece chamável como função
livre e ganha o método associado.

> Nota: `:` é o token canônico de associação. `::` existe **apenas** para o lote.

### 4.4 Keyword `fn`

`fn` fica reservado a **funções livres**.

```text
fn soma(a, b) -> Int { return a + b }    # livre → usa fn
Point:dist() -> Float { ... }              # associado → sem fn
```

O contraste torna `fn` mais significativo, não menos: ele marca o que não pertence
a um tipo.

### 4.5 `self` explícito é aceito

A forma implícita é a **canônica** e a que o formatter produz. A forma explícita é
aceita sem erro nem warning:

```text
Point:dist() -> Float { ... }              # canônico — implícito
Point:dist(self) -> Float { ... }          # aceito — explícito
Point:move(var self, dx, dy) { ... }       # mutação — `var self` obrigatório
```

O parser normaliza as duas para a mesma forma interna (o receptor é sempre o primeiro
slot do frame), então não há custo em aceitar ambas. Não há diagnóstico: quem escreve
`self` explicitamente sabe exatamente o que está fazendo, e a regra `var` continua
sendo a única forma de declarar mutação.

---

## 4bis. Diagnósticos de superfície

Objetivo: quem erra **vê o que fazer**, não só o que está errado. Um erro ocupa no
máximo uma linha de mensagem, uma linha de código com cursor, e uma dica.

Formato de renderização:

```
error [AIPO_SEM_TIPO_NAO_ESTRUTURA]: 'with' só funciona com struct

  14 │     let novo = n with { y: 1 }
     │                ^ sem struct aqui

  dica: use `with` sobre uma struct, não sobre Int
```

Limites rígidos: **uma** mensagem, **um** span primário, **no máximo uma** nota e
**uma** sugestão. Cor carrega apenas estrutura (o código, o `error`, o cursor), nunca
significado.

### 4bis.1 Aviso: nome de hook em método comum

`init` e `invariant` são reconhecidos pelo nome. Um método chamado `inicializar`,
`initialize`, `constructor`, `validar` ou `validate` parece querer ser hook e não é.

```
warning [AIPO_SEM_NOME_DE_HOOK]: 'validar' parece um hook, não um método

   7 │ Point:validar() -> Bool { ... }
     │       ^^^^^^^^ método comum; não roda após construção

  dica: renomeie para `Point:invariant { ... }` se a intenção é validar
```

Mesmo formato para `init`:

```
warning [AIPO_SEM_NOME_DE_HOOK]: 'constructor' parece um hook, não um método

   5 │ Point:constructor(x) { ... }
     │       ^^^^^^^^^^^ método comum; não roda na construção

  dica: use `Point:init(x) { ... }` para o hook de construção
```

Severidade `Warning`, não `Error`: o código funciona, é só a intenção que provavelmente
não foi atingida. Um aviso sem ação possível é ruído; este tem ação.

### 4bis.2 Aviso: hook com corpo vazio

```
warning [AIPO_SEM_HOOK_VAZIO]: 'invariant' sem condição não verifica nada

   9 │ Point:invariant {
     │            ^^^^^^^^^ nenhum predicado

  dica: escreva ao menos uma condição, ou remova o hook
```

### 4bis.3 Erro: hook repetido

```
error [AIPO_SEM_HOOK_DUPLICADO]: 'Point' já declara 'invariant'

  21 │ Point:invariant { self.x > 0 }
     │       ^^^^^^^^^ já declarado na linha 9

  dica: reúna as condições em um único `invariant`
```

### 4bis.4 Erro: `self` mutável ausente

```
error [AIPO_SEM_RECEPTOR_IMUTAVEL]: 'self' é somente leitura aqui

  14 │ Point:avancar() { self.x += 1 }
     │                ^^^ atribuição em receptor imutável

  dica: declare `Point:avancar(var self)` para permitir mutação
```

Este é `Error`, não `Warning`: sem ele, o código não compila de forma alguma, e o
motivo exato é o mais provável ponto de atrito de quem está migrando.

### 4bis.5 Erro: os três tokens de caminho

`.` acessa **valor**, `:` associa **uma** declaração, `::` associa **várias**.

Erro ao chamar método sem receptor pelo `.` com valor solto:

```
error [AIPO_SEM_METODO_NAO_ASSOCIADO]: 'somar' não é método de 'Int'

   9 │ let total = 3.somar(4)
     │             ^^^^^ Int não tem esse método

  dica: use a função livre `somar(3, 4)`, ou defina `Int:somar()`
```

Erro ao usar `::` sem lista:

```
error [AIPO_PARSE_LOTE_INVALIDO]: '::' espera uma lista entre colchetes

   6 │ Point::desenhar()
     │       ^^ use ':' para declarar um único método

  dica: `Point:desenhar()` declara um método; `Point::[f1, f2]` agrupa funções
```

Este par de mensagens é o que **pagou** a escolha de manter os três tokens: o erro
nomeia o token, o motivo e a forma correta na mesma tela.

### 4bis.6 Aviso: `#!` sem âncora

Diretiva solta no fim do arquivo, sem item seguinte:

```
warning [AIPO_PARSE_DIRETIVA_ORFA]: '#!test' não tem função seguinte

  42 │ #!test
     │       ^^^^^ nenhuma declaração após a diretiva

  dica: a diretiva aplica-se ao próximo `fn`; mova-a para logo acima do teste
```

### 4bis.7 Nota: `self` explícito onde o implícito é canônico

Aceito, **sem** diagnóstico (ver §4.5). Não entra na lista.

---

## 4ter. Binding em lote

Aprovado em 2026-10-06, com o status registrado em
[`docs/decisions/surface-decisions.md`](surface-decisions.md).

---

## 5. Diretivas `#!`

Comentário é `#` até o fim da linha. Não existe comentário de bloco.

`#!` introduz uma **diretiva**: um comentário que o compilador lê.

| Diretiva | Onde | Efeito |
|---|---|---|
| `#!satisfies I1, I2` | imediatamente antes de `struct` | Exige que o tipo satisfaça as interfaces. Erro se falhar. |
| `#!test` | imediatamente antes de `fn` livre | Marca a função como teste unitário. |
| `#!test[tag]` | idem | Teste com etiqueta, filtrável por `aipo test --filter tag`. |
| `#!test("nome")` | idem | Teste com nome descritivo explícito. |

Comentários comuns são descartados pelo parser. Diretivas passam: o lexer emite
`TokenKind::Directive`, e o parser associa cada diretiva ao **próximo item**
declarado.

### 5.1 Por que diretiva em vez de declaração

`#!satisfies` mantém a lista de keywords curta, deixa a intenção visível a quem lê
o arquivo, e não polui a assinatura do tipo. O contrato sobrevive a ferramentas que
descartam comentários porque é verificado pelo compilador.

### 5.2 Descoberta de testes

Duas vias, ambas ativas:

- **Convenção de arquivo:** `*_test.aipo`, `test_*.aipo` (compatibilidade)
- **Diretiva:** `#!test` dentro de qualquer arquivo `.aipo`

```text
#!test
fn soma_basico() {
    io.println(String(soma(1, 2)))    # 3
}

#!test[parser]
fn parse_match_com_guard() {
    ...
}

#!test("divisão inteira trunca em direção a zero")
fn divide_trunca() {
    ...
}
```

Um teste por arquivo deixa de ser obrigatório: funções pequenas ficam agrupadas
no arquivo do que testam.

### 5.3 Limites

- A diretiva aplica-se **ao item seguinte**, não ao anterior. Uma diretiva sem item
  seguinte é erro de parse.
- No máximo uma diretiva `#!test` por função.
- `#!test` marca **funções livres**. Um método `Tipo:nome` não é teste: precisa de
  `self`, logo não é unidade isolada.
- A forma e o argumento são validados: `#!test` não aceita conteúdo que não seja
  etiqueta entre `[]` ou string entre aspas.

---

## 6. Divisão inteira

`//` e `//=` são divisão inteira truncada em direção a zero.

`div` e `div=` são **removidos**; `div` volta a ser identificador comum.

```text
let metade = total // 2
```

`/` continua divisão que produz `Float`.

### 6.1 A armadilha e a mitigação

`//` é division, não comentário — o que colide com o hábito formado em JavaScript,
Java, C, C++, Rust, Go, C#, PHP e Swift. O próprio piloto automático escreve
comentário com `//`.

Mitigação: quando `//` aparece onde nenhuma expressão válida pode segui-lo, o
diagnóstico sugere `#`.

```text
// isto é um comentário
// ^ AIPO_PARSE_UNEXPECTED_TOKEN
//   dica: use `#` para comentários; `//` é divisão inteira
```

---

## 7. Parâmetros

```text
fn salvar(usuario: User, var contador: Int, modo = "rapido") { }
```

Duas dimensões ortogonais:

- **Mutabilidade:** `var nome` ou `nome` (imutável por padrão)
- **Tipo:** `: T`, opcional

`nome!` é removido. O `!` era sutil demais para leitura e confundia com `not`.

Formas aceitas: `nome` · `var nome` · `nome: T` · `var nome: T` · `nome = default` ·
`var nome: T = default` · `_`.

---

## 8. Módulos

Um arquivo `.aipo` é um módulo.

```text
import math_util
import math_util as mu
import math_util: somar, dobro
export somar, dobro
```

Privado por padrão. Acesso a símbolo não exportado é `AIPO_SEM_UNKNOWN_NAME`.

`.` é o acesso a membro de módulo **e** a campo. `::` é associação de tipo. Os três
empregos ficam distintos: `.` para valor, `:`/`::` para código.

---

## 9. Falhas

```text
fail "mensagem"
let v = ler_porta() or_else 8080
let w = risky()?
attempt { ... } failed erro { ... }
```

`?` propaga. `or_else` é fallback. `attempt`/`failed` captura bloco.

Não existe `try`, `catch`, `finally`, `throw`, `raise`, `defer`.

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

`await` é legal em statement, inicializador e valor de retorno. Subexpressão é
`AIPO_SEM_AWAIT_IN_SUBEXPRESSION`.

---

## 11. Literais

### 11.1 Strings

| Forma | Nome |
|---|---|
| `"..."` | normal |
| `f"..."` | interpolada |
| `r"..."` | raw |
| `fr"..."` / `rf"..."` | raw interpolada |
| `"""..."""` | multilinha |
| `r"""..."""` / `fr"""..."""` | raw multilinha |

Escapes: `\n` `\r` `\t` `\\` `\"` `\0` `\u{HEX}`. Conteúdo é normalizado em NFC.

### 11.2 Números

`0x` `0b` `0o`, `_` como separador entre dígitos da mesma base. `Int` é inteiro
assinado de 64 bits com faixa segura ±(2^53−1). `Float` é IEEE 754 sem `NaN` e
sem `Infinity`.

---

## 12. Comentário

`#` até o fim da linha. **Único** comentário da linguagem.

`#!` introduz diretiva (§5).

---

## 13. Contagem final

**42 keywords.** Removidas nesta decisão: `end`, `div`, `self!`, `impl`, `satisfy`.

Tokens não-keyword adicionados: `:` (associação), `::` (lote), `//=` já existia.

---

## 14. `enum` — tipo de soma

`enum` é **promovido a V1** (ver §17 do `SYNTAX.md` para o desenho completo).

Um `enum` é um tipo fechado: o compilador conhece todas as variantes e o `match` sobre
ele é **verificado por exaustividade**. Esse é o motivo de existir — sem `enum`,
cobertura de casos é convenção; com ele, é obrigação do compilador.

Três formas de variante, sem sintaxe nova de payload:

```text
enum Estado {
    Inicial,                     # sem payload
    Ativo { desde: Int },        # campos nomeados
    Desligado(motivo: String),   # um campo posicional
}
```

Construção é o nome qualificado, sem `new` e sem `::`:

```text
let e = Estado.Desligado("manutenção")
```

O `match` nomeia tipo **e** variante, reusando o destructure de §6.1:

```text
match e {
    when Estado.Inicial { }
    when Estado.Ativo { desde } { }
    when Estado.Desligado(motivo) { }
}
```

**Nenhum conceito novo de pattern ou binding:** `enum` compõe com `fail` (§7),
`Task` (§9), hooks `Tipo:invariant` (§5.2) e `#!satisfies` (§5.4) exatamente como
`struct`. Regras de Design Considerations da §1 valem: uma forma só para cada coisa
(`Tipo.Variante` para construir e para casar), palavra completa sobre símbolo, e o
erro de exaustividade nomeia as variantes faltantes.

Representação em runtime permanece **decisão de backend** (tagged union com heap vs
int tag + payload inline); a superfície não depende dela.

---

## 15. O que não existe

`defer` · `try` · `catch` · `finally` · `throw` · `raise` · `for` ·
`class` · `const` · `static` · `pub` · `private` · `unsafe` · `yield` · `lambda` ·
`module` · `void` · `case` · `where` · generics · bitwise · `;` · `&&` · `||` ·
`++` · `--` · `**` · `?:` · cast com `as`

Operadores não tokenizados: `@` `&` `;` `&&` `||` `++`. Um `|` isolado produz
*"did you mean '|>'?"*.