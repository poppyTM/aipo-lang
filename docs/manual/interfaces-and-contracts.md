# Interfaces & Contratos

O sistema de tipos do Aipo une a ergonomia da tipagem dinâmica com a precisão dos **contratos de assinatura**, **invariantes de dados** e **interfaces com subtipagem estrutural automática**.

---

## Estruturas (`struct`)

Estruturas agregam campos nomeados e são delimitadas por chaves `{ ... }`. 

Por padrão de design seguro e previsível, **todos os campos de uma estrutura são imutáveis**. Quando for necessário permitir mutação de um campo durante o ciclo de vida da instância, declare-o explicitamente com a palavra-chave `var`:

```aipo
struct Servidor {
    id
    criado_em
    var status = "offline"
    var carga = 0.0
}

# Instanciação estrutural usando chave-valor simétrico com ':'
let s = Servidor{
    id: "srv-1",
    criado_em: 1600000000,
    status: "online",
    carga: 0.42,
}

io.println(s.id)     # "srv-1"
io.println(s.status) # "online"
```

Tentar reatribuir um campo imutável após a construção da instância dispara o diagnóstico semântico estático `AIPO_SEM_FIXED_REASSIGN`.

---

## Hook de Construção (`init`)

O hook `init` se associa ao tipo pelo prefixo `Tipo:` e permite validar, transformar e inicializar os campos da instância antes de sua publicação final:

```aipo
struct Usuario {
    email
    nome
}

Usuario:init(email, nome) {
    if not email.contains("@") {
        return fail("Formato de e-mail inválido")
    }
    self.email = email
    self.nome = nome
}

let u = Usuario{ email: "user@example.com", nome: "Dev" }
io.println(u.email) # "user@example.com"
```

---

## Invariantes Estruturais (`invariant`)

As invariantes declaram predicados lógicos em `Tipo:invariant` que **devem permanecer verdadeiros durante todo o ciclo de vida do objeto**:

```aipo
struct Intervalo {
    var inicio = 0
    var fim = 0
}

Intervalo:init(inicio, fim) {
    self.inicio = inicio
    self.fim = fim
}

Intervalo:invariant {
    self.inicio <= self.fim
}

let inter = Intervalo{ inicio: 5, fim: 10 }
io.println(inter.inicio) # 5
io.println(inter.fim)    # 10
```

Sempre que um campo de uma estrutura com bloco `invariant` for alterado, o motor de execução verifica automaticamente o predicado. Caso a verificação falhe, a operação é rejeitada. Se estiver dentro de um bloco `attempt { ... }`, as mutações anteriores sofrem rollback automático pelo journal transacional.

---

## Métodos e Mutabilidade Universal (`var self`)

Métodos são associados a um tipo pelo prefixo `Tipo:`. O receptor `self` é implícito em métodos imutáveis e explícito como `var self` em métodos que mutam.

Por padrão de segurança, o receptor `self` é **somente leitura**. Quando um método precisa alterar o estado interno da instância, ele declara explicitamente `var self`, alinhando a mutabilidade de métodos à mesma regra universal de variáveis da linguagem:

```aipo
struct Contador {
    var valor = 0
}

# Método de leitura: self é implícito e imutável
Contador:atual() -> Int {
    return self.valor
}

# Método mutador: var self declara explicitamente a intenção de modificar
Contador:incrementar(var self) {
    self.valor += 1
}
```

---

## Interfaces e Subtipagem Estrutural Automática (`interface`)

Interfaces declaram contratos estruturais de métodos. Em Aipo, conformidade não exige declarações burocráticas no topo do arquivo: **a subtipagem é estrutural e automática** (modelo inspirado em linguagens modernas de alta produtividade como Go e Luau).

Se uma estrutura implementa todos os métodos exigidos por uma `interface` com assinaturas e contratos compatíveis (incluindo aridade e mutabilidade de `self`), ela **automaticamente satisfaz a interface**, sem necessidade de nenhum comando adicional:

```aipo
interface Renderizavel {
    desenhar() -> String
}

#!satisfies Renderizavel
struct Botao {
    texto
}

Botao:desenhar() -> String {
    return f"[Botão: {self.texto}]"
}

# Botao satisfaz Renderizavel por correspondência estrutural de métodos.
# A diretiva #!satisfies exige essa relação e verifica em tempo de compilação;
# sem ela, a correspondência continua válida — a diretiva apenas documenta
# a intenção e falha se a conformidade deixar de valer.

# Aceita qualquer valor que satisfaça a interface Renderizavel
fn renderizar_elemento(item: Renderizavel) -> String {
    return item.desenhar()
}

let btn = Botao{ texto: "Salvar" }
io.println(renderizar_elemento(btn)) # "[Botão: Salvar]"
```

A conformidade é verificada estaticamente pelo analisador semântico (`aipo-sema`), checando a existência dos métodos, número de argumentos, tipos de parâmetros e retornos, e a mutabilidade compatível do receptor (`self` vs `var self`).

---

## Associação em Lote (`::`)

Funções livres existentes podem ser promovidas em lote para métodos associados a um tipo através do token `::`:

```aipo
fn perimetro(self) -> Float {
    return 2.0 * (self.largura + self.altura)
}

fn duplicar(var self) {
    self.largura *= 2.0
    self.altura *= 2.0
}

# Promove ambas as funções para métodos de Retangulo
Retangulo::[perimetro, duplicar]
```

| Regra | Detalhe |
|---|---|
| **Primeiro Parâmetro** | **Obrigatoriamente `self` ou `var self`.** Funções sem `self` como primeiro parâmetro são rejeitadas com erro estático pelo compilador. |
| **Mutabilidade** | Preservada da origem: `self` gera método somente leitura; `var self` gera método mutador. |
| **Acesso** | A função original continua acessível livremente (`perimetro(ret)`) e como método (`ret.perimetro()`). |

---

## Enums: Tipos de Soma Fechados (`enum`)

O `enum` representa tipos algébricos fechados ("isto é A **ou** B"), onde o conjunto de valores possíveis é conhecido em tempo de compilação:

```aipo
enum EstadoConexao {
    Desconectado,
    Tentando(tentativa: Int),
    Conectado { ip: String, ping_ms: Int },
    Erro(mensagem: String),
}
```

### Formas de Variante
1. **Sem payload:** Valor constante (`Desconectado`).
2. **Payload posicional:** Um valor entre parênteses (`Tentando(tentativa: Int)`).
3. **Payload nomeado:** Campos rotulados entre chaves (`Conectado { ip: String, ping_ms: Int }`).

### Construção e Pattern Matching
A construção de um enum qualifica o tipo diretamente (`EstadoConexao.Desconectado`), sem `new` ou `::`.

O `match` sobre enum é verificado por **exaustividade**: se não houver um braço `else`, **todas** as variantes devem ser cobertas obrigatoriamente:

```aipo
# `match` é um statement, não uma expressão: cada braço tem corpo próprio e o
# valor é produzido pelo enunciado de retorno depois dele.
fn relatar(estado: EstadoConexao) -> String {
    var texto = ""
    match estado {
        when EstadoConexao.Desconectado {
            texto = "Sem conexão"
        }
        when EstadoConexao.Tentando(tentativa) {
            texto = f"Tentativa #{tentativa}"
        }
        when EstadoConexao.Conectado { ip, ping_ms } {
            texto = f"Conectado a {ip} ({ping_ms}ms)"
        }
        when EstadoConexao.Erro(motivo) {
            texto = f"Falha: {motivo}"
        }
    }
    return texto
}
```

Enums também suportam métodos (`EstadoConexao:metodo()`), invariantes (`EstadoConexao:invariant`) e diretiva de interface `#!satisfies`.

---

## Diretivas do Compilador (`#!nome`)

Diretivas são comentários estruturados lidos diretamente pelo compilador que se aplicam ao item imediatamente seguinte:

| Diretiva | Aplicação | Efeito |
|---|---|---|
| `#!satisfies I1, I2` | `struct` ou `enum` | Exige conformidade estrutural com as interfaces em tempo de compilação. |
| `#!test` / `#!test[tag]` / `#!test("nome")` | `fn` livre | Marca a função como teste unitário descoberto pelo runner de testes. |
| `#!deprecated("msg")` | `fn`, `struct` ou método | Emite avisos durante a checagem semântica indicando obsolescência. |
| `#!todo("msg")` | qualquer item | Rastreia débitos técnicos sem interromper o fluxo de compilação. |
