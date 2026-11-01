# Sintaxe & Tipos de Dados

O Aipo foi projetado para oferecer uma sintaxe enxuta, determinística e livre de comportamentos implícitos perigosos. Esta seção detalha o sistema de tipos, as melhores práticas canônicas de escrita e o uso das ferramentas modernas da linguagem.

---

## O Jeito Aipo: Princípios de Escrita Idiomática

Para garantir código limpo, de alta performance e fácil de ler (especialmente convidativo para desenvolvedores com TDAH e dislexia), o Aipo estabelece regras canônicas claras:

| Prática Canônica (O Jeito Aipo) | Anti-Padrão a Evitar | Por que isso importa? |
| :--- | :--- | :--- |
| `let nome = "Dev"` (Imutabilidade por padrão) | `var nome = "Dev"` sem necessidade de mutação | Previne mutações acidentais e facilita o raciocínio determinístico sobre o estado. |
| `struct Ponto { x, var y = 0 }` (Campos imutáveis por padrão) | Mutações globais ou campos mutáveis desnecessários | Mantém integridade de estruturas; mutabilidade exige intenção explícita com `var`. |
| `Usuario{ nome: "Ana", idade: 28 }` (Chave-valor com `:`) | Sintaxe divergente de mapeamento | Dois-pontos unifica a sintaxe de associação em dicionários e structs sem conflito motor. |
| `if condicao { ... }` (Blocos com chaves) | Parênteses redundantes ao redor de condições | Sintaxe enxuta, sem parênteses extras e compatível com realce visual por pares (*rainbow brackets*). |
| `Tipo:depositar(var self, valor)` (Receptor mutável explícito) | Efeitos colaterais ocultos em métodos | `var` é a palavra universal de mutabilidade na linguagem, tornando efeitos colaterais visíveis. |
| `a // b` e `a //= b` (Divisão inteira) | Conversões manuais truncadas | Operador aritmético canônico expressivo e simétrico com `+`, `-`, `*`, `/`. |
| `f"Usuário {id}: {email}"` (Interpolação direta) | Concatenação `"Usuário " + String(id) + ": " + email` | Interpolação direta elimina ruído visual e múltiplas alocações temporárias no heap. |
| `r"C:\dados\relatorio.csv"` (Strings brutas) | Sequências de escape `"C:\\dados\\relatorio.csv"` | Strings brutas eliminam o excesso de barras invertidas em caminhos de arquivos e regex. |
| `let cidade = usuario?.perfil?.cidade` (Navegação segura) | Encadeamentos defensivos `if usuario != none and ...` | Navegação segura evita verificações aninhadas redundantes de nulidade. |
| `let porta = ler_porta() or_else 8080` (Fallback `or_else`) | Tratamento burocrático `attempt { porta = ... } failed ...` | `or_else` fornece valores padrão imediatos em expressões sujeitas a falha. |
| `dados |> filtrar() |> calcular()` (Pipeline `\|>`) | Aninhamento profundo `calcular(filtrar(dados))` | O operador pipeline expressa transformações na ordem natural de execução dos dados. |
| `lista.add(item)` (Inserção universal) | Métodos heterogêneos de coleção | `.add()` é a operação canônica universal de inserção em listas e conjuntos. |
| `dados.lazy().filter(...).collect()` (Avaliação lazy) | Múltiplas etapas intermediárias no heap | `.lazy()` consome memória constante sem gerar listas temporárias intermediárias. |

---

## 1. Strings Modernas no Aipo

O manuseio de texto no Aipo é robusto, expressivo e matematicamente previsível. Toda string é garantida em **UTF-8 válido com normalização canônica NFC automática**.

### Interpolação de Strings (`f"..."`)
A interpolação com o prefixo `f` é o padrão canônico para formatar mensagens e compor textos:

```aipo
let usuario = "Alice"
let pontuacao = 98.5
let nivel = 4

# Interpolação com variáveis e expressões numéricas
let relatorio = f"Jogador: {usuario} | Nível: {nivel} | Pontos: {pontuacao}"
io.println(relatorio)
# Imprime: "Jogador: Alice | Nível: 4 | Pontos: 98.5"

# Executando operações dentro das chaves interpoladas
let delta = 1.5
io.println(f"Próxima meta: {pontuacao + delta}") # 100.0
```

#### Escapando Chaves Literais
Se precisar incluir os caracteres `{` ou `}` literalmente dentro de uma f-string, duplique-os (<code>&#123;&#123;</code> e <code>&#125;&#125;</code>):

```aipo
let chave = "token"
let valor = "xyz123"

# Renderiza um objeto JSON com chaves literais e valor interpolado
let json_payload = f"{{\"{chave}\": \"{valor}\"}}"
io.println(json_payload) # {"token": "xyz123"}
```

---

### Strings Brutas (`r"..."`)
Em linguagens convencionais, escrever caminhos de arquivo no Windows ou expressões regulares exige dobrar todas as barras invertidas (`\\\\`), gerando poluição visual que dificulta a leitura. 

Com as **Raw Strings** do Aipo (`r"..."`), as barras invertidas são lidas literalmente:

```aipo
# 1. Caminhos de arquivo do sistema hospedeiro
let caminho_windows = r"C:\Users\dev\AppData\Local\Aipo\config.toml"
io.println(caminho_windows)
# Imprime literalmente: C:\Users\dev\AppData\Local\Aipo\config.toml

# 2. Padrões de expressões regulares limpos e sem escape duplo
let padrao_email = r"^[a-zA-Z0-9_.+-]+@[a-zA-Z0-9-]+\.[a-zA-Z0-9-.]+$"
io.println(padrao_email)
```

---

### Strings Multilinha (`"""..."""` e `r"""..."""`)
Para blocos extensos de texto, documentação inline, consultas SQL ou templates, utilize as aspas triplas. Elas preservam formatação, identação e quebras de linha com clareza:

```aipo
let consulta_sql = """
SELECT u.id, u.nome, p.cargo
FROM usuarios u
JOIN permissoes p ON p.usuario_id = u.id
WHERE u.ativo = true
ORDER BY u.nome ASC
"""

io.println(consulta_sql)
```

---

### Raw Format Strings (`fr"..."` / `rf"..."`)
Quando você precisa compor um padrão regex dinâmico ou um caminho que contenha variáveis interpoladas sem ter que escapar barras invertidas, combine ambos os prefixos:

```aipo
let pasta = "logs"
let extensao = "txt"

# Combina raw string (barras não escapadas) com interpolação de variáveis:
let caminho_dinamico = fr"C:\sistema\{pasta}\app.{extensao}"
io.println(caminho_dinamico)
# Imprime: C:\sistema\logs\app.txt
```

---

### Conversão Binária de Strings (`.encode()` e `.decode()`)
Em operações de rede e gravação de arquivos binários, strings e buffers `Bytes` convertem-se diretamente sem dependências externas:

```aipo
let mensagem = "Aipo determinístico"

# Converte String UTF-8 para buffer bruto de Bytes
let bytes_brutos = mensagem.encode()
io.println(f"Tamanho em bytes: {bytes_brutos.len()}")

# Decodifica buffer Bytes de volta para String UTF-8 NFC
let texto_original = bytes_brutos.decode()
io.println(texto_original) # "Aipo determinístico"
```

---

## 2. Variáveis e Imutabilidade

O Aipo adota o princípio de imutabilidade padrão (*immutable-by-default*) de forma consistente em todos os escopos:

```aipo
# Imutável: o compilador impede reatribuições posteriores
let taxa_servico = 0.15

# Mutável: reservado para acumuladores ou estados locais em laços
var total_acumulado = 100.0
total_acumulado += 25.0
io.println(total_acumulado) # 125.0
```

::: tip Regra de Ouro
Sempre inicie novas declarações com `let`. Apenas altere para `var` se a variável for explicitamente modificada no fluxo local.
:::

---

## 3. Tipos Primitivos & Aritmética Segura

### Inteiros (`Int`)
Inteiros com sinal limitados à faixa segura **±(2^53 − 1)** — o mesmo domínio exato de um `Float` IEEE 754 de precisão dupla, o que mantém a conversão entre `Int` e `Float` sem perda. Um literal ou resultado fora dessa faixa dispara `AIPO_RT_OVERFLOW` em vez de envolver silenciosamente. Suportam representações em diferentes bases numéricas e separadores visuais com sublinhado (`_`):

```aipo
let decimal = 42
let milhao = 1_000_000    # Separador para facilitar a leitura visual
let hexadecimal = 0xFF   # 255
let binario = 0b101010   # 42
let octal = 0o777        # 511
```

### Ponto Flutuante (`Float`)
Números de precisão dupla de 64 bits (IEEE 754).
```aipo
let pi = 3.14159
let fracionario = 0.005
```

### Divisão Precisa (`/`) vs Divisão Inteira (`//`)
No Aipo, o operador `/` sempre retorna um `Float`. Para realizar divisão inteira truncada, utilize o operador simétrico `//`:

```aipo
let a = 10
let b = 3

let divisao_real = a / b     # 3.3333333333333335 (Float)
let divisao_inteira = a // b # 3 (Int)

# Atribuição composta simétrica
var valor = 20
valor //= 3
io.println(valor) # 6
```

::: warning Sem NaN ou Infinito Silencioso
O modelo de valores do Aipo proíbe valores corrompidos como `NaN` ou `Infinity`. Operações inválidas (como divisão por zero ou raiz de número negativo) geram falhas estruturadas imediatamente.
:::

---

## 4. Coleções Idiomáticas

### 1. Listas (`List`)
Vetores dinâmicos ordenados indexados a partir de `0`.
- Inserção de elementos: utilize `.add(item)` (o Aipo padroniza `.add` para listas e conjuntos).
- Indexação reversa: índices negativos contam a partir do final (`[-1]` acessa o último elemento).
- Suporte a vírgula final (*trailing comma*) em declarações multilinha.

```aipo
let linguagens = [
    "Rust",
    "Aipo",
    "TypeScript",
]

linguagens.add("Odin")

io.println(linguagens[0])  # "Rust"
io.println(linguagens[-1]) # "Odin"
io.println(linguagens.len()) # 4
```

### 2. Dicionários (`Dict`)
Mapas associativos chave-valor indexados com `{}`. Preservam rigorosamente a ordem de inserção original:

```aipo
let configuracao = {
    "servidor": "api.aipo.dev",
    "porta": 8443,
    "ssl": true,
}

# Verificação explícita de presença com .has()
if configuracao.has("porta") {
    let porta = configuracao["porta"]
    io.println(f"Porta configurada: {porta}")
}
```

::: tip `dict[chave]` falha; `dict.get(chave)` devolve `none`
O Aipo tem dois acessos com contratos diferentes. `configuracao["porta"]` é a leitura **direta**: se a chave não existir, o programa dispara `AIPO_RT_KEY_NOT_FOUND` — o erro aparece onde aconteceu, sem virar um `none` que só quebra depois. `configuracao.get("porta")` é a leitura **opcional**: devolve o valor quando a chave existe e `none` quando não existe.

Escolha pelo que você sabe sobre a chave: use `[]` quando a ausência é um bug, e `.get()` quando a ausência é um caso normal. Para testar presença sem ler, use `.has(chave)`.
:::

### 3. Conjuntos com Ordem de Inserção (`Set`)
Armazenam valores únicos e mantêm a ordem em que foram inseridos:

```aipo
let tags = Set(["backend", "compilador", "backend"]) # Duplicata descartada

tags.add("cli")
io.println(tags.has("backend")) # true
io.println(tags.len())           # 3
io.println(tags.to_list())       # ["backend", "compilador", "cli"]
```

### 4. Sequências Lazy (`Sequence`)
Para processar grandes volumes de dados sem alocar coleções intermediárias, crie uma sequência com `.lazy()`:

```aipo
let numeros = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]

# O encadeamento abaixo executa sob demanda em memória constante.
# Escreva a cadeia em uma linha: o parser não continua uma expressão
# na linha seguinte só porque ela começa com `.`.
let pares_triplicados = numeros.lazy().filter(x => x % 2 == 0).map(x => x * 3).take(2).collect()

io.println(pares_triplicados) # [6, 12]
```

Se a linha ficar longa demais, divida em bindings intermediários em vez de quebrar antes do ponto:

```aipo
let numeros = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
let pares = numeros.lazy().filter(x => x % 2 == 0)
let triplicados = pares.map(x => x * 3)
io.println(triplicados.take(2).collect()) # [6, 12]
```

### 5. Enums (`enum`)
O `enum` define um tipo de soma fechado com variantes conhecidas pelo compilador, suportando variantes sem payload, com payload posicional ou com campos nomeados:

```aipo
enum ModoExecucao {
    Depuracao,
    Otimizado { nivel: Int },
    Perfilado(arquivo: String),
}

let modo = ModoExecucao.Otimizado { nivel: 3 }
```

---

## 5. Operadores Modernos e Expressivos

### Navegação Segura (`?.`)
Evita checagens manuais de `none`. Se qualquer elo da cadeia for `none`, o resultado final é `none` sem disparar erro:

```aipo
struct Endereco {
    cidade
}

struct Usuario {
    endereco
}

# Instanciação simétrica usando dois-pontos (:) consistente:
let u1 = Usuario{ endereco: Endereco{ cidade: "Curitiba" } }
let u2 = Usuario{ endereco: none }

io.println(u1?.endereco?.cidade) # "Curitiba"
io.println(u2?.endereco?.cidade) # none
```

### Operador de Fallback para Falhas (`or_else`)
Permite fornecer um valor de recuperação imediato se uma expressão disparar um `fail`, sem a necessidade de abrir um bloco `attempt`:

```aipo
fn carregar_porta(ambiente) {
    if ambiente == "producao" {
        return 443
    }
    return fail("ambiente desconhecido")
}

# Se carregar_porta() disparar fail, o operador or_else assume o valor à direita:
let porta = carregar_porta("teste") or_else 8080
io.println(f"Porta ativa: {porta}") # 8080
```

### Operador Pipeline (`|>`)
Permite estruturar transformações de dados em uma sequência natural da esquerda para a direita:

```aipo
fn limpar(txt) {
    return txt.trim()
}

fn destacar(txt, prefixo) {
    return prefixo + txt
}

# "  alerta  " é passado como primeiro argumento para limpar(), e o resultado para destacar():
let rotulo = "  alerta  " |> limpar() |> destacar("[URGENTE] ")
io.println(rotulo) # "[URGENTE] alerta"
```

### Verificação de Tipos (`is`)
Verifica se um valor pertence a um tipo concreto da linguagem:

```aipo
let valor = 42

if valor is Int {
    io.println("É um inteiro seguro de 64 bits")
}

# Para verificar ausência de valor, use igualdade direta com none:
let dado = none
if dado == none {
    io.println("Valor nulo")
}
```
