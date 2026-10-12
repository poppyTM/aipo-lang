# Aipo V1 — Inventário da Biblioteca Padrão (Stdlib)

Este documento descreve os módulos embutidos do runtime Aipo V1.

> **Autoridade:** o **compilador** é a fonte normativa, não este documento.
> Um nome listado aqui que não exista no runtime é rejeitado por
> `aipo check` com `AIPO_SEM_UNKNOWN_NAME` (`unknown host member 'mod.membro'`);
> um nome que exista no runtime mas falte aqui continua sendo válido.
> Este inventário cobre os módulos de uso corrente — não é exaustivo.
> O catálogo completo e as aridades vivem em
> `crates/aipo-stdlib/src/*` (`register_stdlib`) e `aipo-runtime::NativeRegistry`.

---

## 0. Módulos de prelude

Os módulos abaixo já estão **visíveis sem `import`**. Escrever `import io`,
`import math` ou `import task` é erro (`AIPO_SEM_UNKNOWN_MODULE`): `import`
serve apenas para módulos do usuário e pacotes.

| Módulo | Domínio |
|---|---|
| `io` | Saída de texto |
| `math` | Matemática |
| `string` | Operações sobre texto |
| `task` | Agendador cooperativo, tempo virtual e combinadores |
| `time` | Relógios civis e durações |
| `env` | Variáveis de ambiente (por capability) |
| `fs` | Sistema de arquivos (por capability, assíncrono) |
| `log` | Logging estruturado |
| `sh` | Execução de processos (por capability) |
| `random`, `json`, `encoding`, `binary`, `path`, `url`, `regex` | Utilitários |
| `expect`, `testing` | Asserções e runner de testes |

---

## 1. Módulo `io` (Entrada e Saída)

| Função | Assinatura | Descrição |
|---|---|---|
| `io.print` | `io.print(valor) -> none` | Escreve na saída padrão sem quebra de linha. |
| `io.println` | `io.println(valor) -> none` | Escreve na saída padrão seguido de `\n`. |

> **Nota:** `io` é só saída. Leitura de linha e leitura/escrita de arquivo
> **não existem** em `io`; arquivos passam por `fs` (ver seção 4):
> `fs.read_text` (leitura) e `fs.write_text` (escrita) são assíncronos e
> sujeitos às capabilities `filesystem.read` e `filesystem.write`.

---

## 2. Módulo `math` (Matemática e Aritmética)

| Função | Assinatura | Descrição |
|---|---|---|
| `math.sqrt` | `math.sqrt(x: Float) -> Float` | Raiz quadrada. |
| `math.abs` | `math.abs(x: Num) -> Num` | Valor absoluto de Int ou Float. |
| `math.floor` | `math.floor(x: Float) -> Int` | Arredonda para baixo. |
| `math.ceil` | `math.ceil(x: Float) -> Int` | Arredonda para cima. |
| `math.round` | `math.round(x: Float) -> Int` | Arredonda para o mais próximo. |
| `math.min` | `math.min(a: Num, b: Num) -> Num` | Menor entre dois números. |
| `math.max` | `math.max(a: Num, b: Num) -> Num` | Maior entre dois números. |
| `math.pow` | `math.pow(base: Num, exp: Num) -> Float` | Exponenciação. |
| `math.clamp` | `math.clamp(x, min, max) -> Num` | Restringe ao intervalo. |
| `math.sin`, `cos`, `tan`, `asin`, `acos`, `atan` | `f(x: Float) -> Float` | Trigonometria (radianos). |
| `math.atan2` | `math.atan2(y: Float, x: Float) -> Float` | Arco-tangente de dois argumentos. |
| `math.hypot` | `math.hypot(a: Float, b: Float) -> Float` | `sqrt(a² + b²)` sem overflow intermediário. |
| `math.log`, `log2`, `log10`, `exp` | `f(x: Float) -> Float` | Logaritmos e exponencial. |
| `math.sign` | `math.sign(x: Num) -> Num` | `-1`, `0` ou `1`. |
| `math.rad` / `math.deg` | `f(x: Num) -> Float` | Conversão graus ↔ radianos. |

Constantes: `math.pi`, `math.e`, `math.tau`.

> **Nota:** Não existem `NaN` nem `Infinity` em Float. Operações inválidas disparam falha estruturada do runtime.

---

## 3. Módulo `task` (Concorrência e Tempo Virtual)

| Função | Assinatura | Descrição |
|---|---|---|
| `task.spawn` | `task.spawn(fn, args: List) -> Task` | Cria uma tarefa que chamará `fn` com os itens de `args`. |
| `task.sleep` | `task.sleep(ms: Int)` | Pausa a tarefa por `ms` milissegundos de tempo virtual. |
| `task.all` | `task.all(tasks: List) -> List` | Aguarda todas, preservando a ordem dada. |
| `task.race` | `task.race(tasks: List)` | Resultado da primeira conclusão em ordem determinística. |
| `task.timeout` | `task.timeout(task, ms: Int)` | Valor em tempo, ou `Failure` de timeout (a tarefa é cancelada). |
| `task.cancel` | `task.cancel(task)` | Cancela a tarefa. |
| `task.group` | `task.group() -> Group` | Escopo estruturado com `spawn` e `wait`. |

> **Nota:** **não existe `task.now`**. Tempo de calendário é `time.now()` /
> `time.monotonic()` (capability `clock`). Lembrar que `await do { … }` é
> statement e que `await` é legal em statement, inicializador e valor de retorno.

---

## 4. Módulos `env`, `fs`, `log` e `sh` (Ambiente e Sistema Operacional)

**Não existe módulo `sys`.** O acesso ao ambiente é por capability e vem
negado por padrão no perfil da CLI.

| Função | Assinatura | Descrição |
|---|---|---|
| `env.get` | `env.get(nome) -> String?` | Lê a variável de ambiente (`none` se ausente ou se a capability estiver negada). |
| `env.has` | `env.has(nome) -> Bool` | Informa se a variável existe. |
| `fs.read_text` | `fs.read_text(caminho) -> Task[String]` | Lê arquivo de texto (capability `filesystem.read`, assíncrono). |
| `fs.write_text` | `fs.write_text(caminho, texto) -> Task[none]` | Escreve texto em arquivo, criando ou substituindo (capability `filesystem.write`, assíncrono). |
| `fs.roots` | `fs.roots() -> List[String]` | Lista as raízes expostas pelo host (capability `filesystem.roots`). |
| `log.trace` / `debug` / `info` / `warning` / `error` | `f(msg)` | Logging estruturado por nível. |
| `sh.run` | `sh.run(programa, args: List)` | Executa processo (capability `sh.*`). |
| `sh.cd`, `sh.pwd`, `sh.env`, `sh.which` | `sh.cd(dir)`, `sh.pwd()`, `sh.env(nome)`, `sh.which(nome)` | Consultas e navegação do shell. |

> **Nota:** não existe `env.args()`. O Aipo **não expõe argumentos de linha de
> comando** na V1: um programa recebe seus argumentos como parâmetro (`List`) e
> o topo do módulo é o script executado (não há `fn main` implícito).

---

## 5. Primitivos Globais e Conversões de Tipo

Funções globais sempre acessíveis sem necessidade de `import`:

| Primitivo | Exemplo | Descrição |
|---|---|---|
| `len(x)` | `len([1, 2, 3])`, `len("texto")`, `len({\"a\": 1})` | Retorna comprimento de lista, dicionário ou string. |
| `Int(x)` | `Int(3.14)` $\rightarrow$ `3`, `Int("42")` $\rightarrow$ `42` | Conversão e truncamento para inteiro de 64 bits. |
| `Float(x)` | `Float(42)` $\rightarrow$ `42.0` | Conversão para número de ponto flutuante de 64 bits. |
| `String(x)` | `String(123)` $\rightarrow$ `"123"` | Converte qualquer valor primitivo em representação de string. |
| `valor is Tipo` | `1 is Int` $\rightarrow$ `true`, `[1] is List` $\rightarrow$ `true` | Teste de tipo em runtime, com `Int`, `Float`, `String`, `Bool`, `List`, `Dict`, `Bytes`, `Range` e nomes de struct/enum. |

> **Nota (verificado no compilador):** são chamáveis como conversão/construção
> `Int(x)`, `Float(x)`, `String(x)`, `Bytes(count)` e `Duration(ms)`;
> `Dict(lista)` e `Set(lista)` existem com **exatamente 1 argumento** (uma lista).
> Já **`Bool(x)`, `List(x)` e `Range(x)` são `AIPO_RT_NOT_CALLABLE`**: são *type
> values*, usados em `valor is Bool` / `valor is List` / `valor is Range`, não construtores.

### 5.1 `Range` como *type value*

`Range` está **vinculado no runtime** como *type value*, igual a `Bool` e `List`:
`valor is Range` funciona, mas `Range(0, 3)` continua `AIPO_RT_NOT_CALLABLE` (não há
forma de construção na V1). Valores de range são produzidos por literais de intervalo
(`0..2`) usados em fatiamento (`lista[0..2]`).
