# Controle de Fluxo & Falhas

O Aipo oferece estruturas de controle de fluxo limpas, determinísticas e altamente legíveis: blocos delimitados por chaves `{ ... }` sem parênteses redundantes ao redor de condições, combinados com um modelo transacional de tratamento de erros com rollback atômico.

---

## Estruturas Condicionais

### Bloco `if ... elif ... else`

As condições dispensam parênteses obrigatórios e os blocos são abertos e fechados por chaves `{ ... }`. A sintaxe fornece clareza visual imediata, com suporte nativo a realce de pares (*rainbow brackets*) e dobragem de código (*code folding*) em qualquer IDE:

```aipo
var pontuacao = 85
var status = ""

if pontuacao >= 90 {
    status = "Excelente"
} elif pontuacao >= 70 {
    status = "Aprovado"
} else {
    status = "Recuperação"
}

io.println(status) # "Aprovado"
```

### Expressão Inline `if condition then a else b`

O Aipo também suporta expressões condicionais de valor em linha única utilizando `then`:

```aipo
let ativo = true
let mensagem = if ativo then "Online" else "Offline"
io.println(mensagem) # "Online"
```

---

## Seleção por Padrão (`match ... when`)

O comando `match` permite bifurcar o fluxo comparando uma expressão contra valores literais, desestruturação de estruturas e variantes de enums:

### Padrões Simples e Guards Condicionais
Cada braço é introduzido por `when` (nunca `case`). Suporta cláusulas de guarda opcionais com `if <condicao>`:

```aipo
struct Usuario {
    nome
    idade
}

let usuario = Usuario{ nome: "Ana", idade: 70 }

match usuario {
    when { nome, idade } if idade >= 65 {
        io.println(f"Sênior: {nome}")
    }
    when { nome, idade } if idade >= 18 {
        io.println(f"Adulto: {nome}")
    }
    else {
        io.println("Menor de idade")
    }
}
```

### Casamento de Variantes de Enum e Exaustividade
Quando o alvo é um `enum`, o compilador impõe checagem estática de exaustividade. Na ausência de um braço `else`, todas as variantes devem ser cobertas:

```aipo
enum EstadoConexao {
    Desconectado,
    Tentando(tentativa: Int),
    Conectado { ip: String, ping_ms: Int },
    Erro(motivo: String),
}

let estado = EstadoConexao.Tentando(2)

match estado {
    when EstadoConexao.Desconectado {
        io.println("Desconectado")
    }
    when EstadoConexao.Tentando(t) {
        io.println(f"Tentando #{t}")
    }
    when EstadoConexao.Conectado { ip, ping_ms } {
        io.println(f"Conectado a {ip}")
    }
    when EstadoConexao.Erro(msg) {
        io.println(f"Erro: {msg}")
    }
}
```

---

## Estruturas de Repetição

Todas as estruturas de repetição em Aipo utilizam blocos delimitados `{ ... }` e **dispensam** parênteses ou palavras de ligação como `do`.

### `while`

Executa o corpo enquanto a condição booleana for verdadeira:

```aipo
var i = 0
while i < 3 {
    io.println(f"Passo: {i}")
    i += 1
}
```

### `loop`

Laço contínuo canônico, projetado para repetições que dependem de `break` explícito:

```aipo
var tentativas = 0
loop {
    tentativas += 1
    if tentativas >= 3 {
        break
    }
}
io.println(f"Total de tentativas: {tentativas}")
```

### `repeat`

Repete o bloco um número fixo de vezes com um contador opcional (`repeat count as indice`):

```aipo
# Executa 3 vezes (com índices 0, 1 e 2)
repeat 3 as idx {
    io.println(f"Iteração número: {idx}")
}
```

### `each`

Iteração canônica sobre coleções (`List`, `Dict`, `Set`, `Sequence`):

```aipo
# Iteração simples sobre lista
let frutas = ["Maçã", "Banana", "Laranja"]
each fruta in frutas {
    io.println(fruta)
}

# Iteração com índice e elemento
each idx, fruta in frutas {
    io.println(f"{idx}: {fruta}")
}

# Iteração sobre dicionário (chave e valor na ordem de inserção)
let config = {
    "host": "127.0.0.1",
    "port": 5432,
}
each chave, valor in config {
    io.println(f"{chave} => {valor}")
}
```

---

## Modelo de Falhas & Transações (`attempt ... failed`)

Em Aipo, erros não são exceções globais com saltos de pilha descontrolados, nem códigos de status que podem ser esquecidos. Falhas operacionais são disparadas explicitamente com `fail` (ou retornadas com `return fail(...)`) e tratadas por blocos transacionais com **journaling e rollback automático**.

```aipo
struct Cofre {
    var saldo = 0.0
}

Cofre:init(saldo_inicial = 0.0) {
    self.saldo = saldo_inicial
}

Cofre:invariant {
    self.saldo >= 0.0
}

let c = Cofre{ saldo: 100.0 }

attempt {
    # Esta operação temporariamente reduz o saldo para -100.0
    c.saldo -= 200.0
} failed erro {
    # Como violou a invariante, o rollback restaura self.saldo para 100.0!
    io.println(f"Falha capturada: {erro.message}")
}

# O saldo permanece exatamente no valor anterior à tentativa!
io.println(f"Saldo preservado: {c.saldo}") # 100.0
```

Se qualquer operação dentro do bloco `attempt` disparar um `fail` ou violar uma invariante estrutural, todas as mutações ocorridas nos objetos rastreados no journal são revertidas atomicamente para o estado original.

### Fallback Imediato com `or_else`

Para expressões onde você deseja apenas prover um valor padrão de recuperação sem a verbosidade de um bloco `attempt`, utilize o operador canônico `or_else`:

```aipo
fn ler_arquivo(caminho) {
    # Se o arquivo não existir ou falhar, retorna fail
    return fail("arquivo não encontrado")
}

# Se ler_arquivo disparar fail, or_else avalia e retorna a alternativa:
let conteudo = ler_arquivo("config.toml") or_else "host = 127.0.0.1"
io.println(conteudo) # "host = 127.0.0.1"
```
