# Exemplos Práticos & Receitas de Código

Esta seção reúne padrões de código idiomáticos, receitas do mundo real e o catálogo completo dos 25 exemplos canônicos que acompanham o repositório oficial da linguagem Aipo.

---

## 1. Pipeline de Dados com `|>` e Funções de Ordem Superior

O operador pipeline `|>` insere o resultado da expressão anterior como o primeiro argumento da função seguinte, criando fluxos de leitura contínua e natural da esquerda para a direita:

```aipo
struct Produto {
    nome: String
    preco: Int
    categoria: String
}

let catalogo = [
    Produto{ nome: "Teclado", preco: 250, categoria: "Hardware" },
    Produto{ nome: "Livro de Rust", preco: 120, categoria: "Educação" },
    Produto{ nome: "Mouse Óptico", preco: 80, categoria: "Hardware" },
    Produto{ nome: "Monitor 4K", preco: 2200, categoria: "Hardware" }
]

# Filtrar produtos de hardware acessíveis (< R$ 500) e obter apenas os nomes
fn filtrar_hardware_acessivel(itens, teto: Int) {
    let filtrados = itens.filter(fn (p) { return p.categoria == "Hardware" and p.preco <= teto })
    return filtrados.transform(fn (p) { return p.nome })
}

let acessiveis = catalogo |> filtrar_hardware_acessivel(300)
each nome in acessiveis {
    io.println(f"Disponível: {nome}")
}
```

---

## 2. Modelagem com Invariantes e Rollback Atômico

No Aipo, structs com hooks `invariant()` protegem seu estado em todas as fronteiras mutáveis estáveis. Se um método violar o invariante ou lançar um `fail(...)`, **todas as mutações sofrerão rollback imediato**:

```aipo
struct Carteira {
    usuario: String
    var saldo: Int
}

Carteira:invariant {
    self.saldo >= 0
}

Carteira:creditar(var self, valor: Int) {
    if valor <= 0 {
        fail("o valor do crédito deve ser positivo")
    }
    self.saldo += valor
}

Carteira:debitar(var self, valor: Int) {
    self.saldo -= valor
}

let c = Carteira{ usuario: "Lucas", saldo: 150 }

attempt {
    c.creditar(50)     # saldo sobe para 200
    c.debitar(500)     # violação: saldo ficaria -300!
} failed err {
    io.println(f"Operação cancelada: {err.message}")
}

# O saldo permanece 150 (o estado de entrada anterior foi integralmente restaurado)
io.println(f"Saldo seguro: {c.saldo}")
```

---

## 3. Concorrência com `async`, `task` e `await do`

O modelo assíncrono do Aipo é cooperativo, determinístico e baseado em tarefas:

```aipo
async fn buscar_usuario(id: Int) {
    task.sleep(50)
    return {"id": id, "nome": "Dev Aipo"}
}

async fn buscar_permissoes(id: Int) {
    task.sleep(30)
    return ["leitura", "escrita", "deploy"]
}

async fn carregar_perfil(id: Int) {
    let t_user = task.spawn(buscar_usuario, [id])
    let t_perms = task.spawn(buscar_permissoes, [id])

    let user = await t_user
    let perms = await t_perms

    return {"usuario": user, "permissoes": perms}
}
```

---

## 4. Manipulação Segura de JSON

A biblioteca padrão `json` oferece validação estrita sem ambiguidades:

```aipo
let payload_texto = r"""{"servico": "auth", "porta": 8080, "ativo": true}"""

# Parsing com fallback seguro usando or_else
let dados = json.parse(payload_texto) or_else fail("JSON malformado")

let servico = dados["servico"]
let porta = dados["porta"]
io.println(f"Servico: {servico}")
io.println(f"Porta: {porta}")

# Serialização de estruturas de dados
let resposta = {
    "status": "ok",
    "timestamp": 1727376000
}
let json_gerado = json.stringify(resposta)
io.println(json_gerado)
```

---

## 5. Catálogo dos 25 Exemplos do Repositório

Todos os exemplos abaixo encontram-se testados e executáveis no diretório `examples/` da raiz do repositório:

| Exemplo | Tópicos Principais |
| :--- | :--- |
| `01_fizzbuzz.aipo` | Funções puras, `repeat ... as`, operadores mod `%` e controle `if/elif`. |
| `02_local_functions.aipo` | Funções locais aninhadas, closures e autorrecursão direta. |
| `03_contracts_and_interfaces.aipo` | Assinaturas de contrato, interfaces estruturais e invariantes com rollback. |
| `04_data_pipeline.aipo` | Pipeline completo `|>`, coleções com métodos e trailing blocks `do { ... }`. |
| `05_modules/` | Modularidade com `export`, importações seletivas e encapsulamento privado. |
| `06_variables_and_values.aipo` | Imutabilidade com `let`, mutabilidade com `var` e tipos primitivos. |
| `07_functions_defaults_named_args.aipo` | Argumentos padrão (`greeting = "hi"`) e passagem nomeada de parâmetros. |
| `08_lists_dicts_and_slices.aipo` | Listas, dicionários, índices negativos e fatiamento tolerante (`..`). |
| `09_strings_unicode_and_formatting.aipo` | Interpolação `f"..."`, raw strings `r"..."` e caracteres Unicode em UTF-8. |
| `10_bytes.aipo` | Manipulação de buffers binários `Bytes` e conversões seguras. |
| `11_failures_or_else_attempt.aipo` | Tratamento idiomático de falhas com `fail`, `or_else` e `attempt/failed`. |
| `12_struct_init_fixed_invariant.aipo` | Construtores `init`, campos imutáveis por padrão e validação `invariant`. |
| `13_mutation_and_rollback.aipo` | Reversão atômica de transação ao violar invariantes de struct. |
| `14_interfaces_and_satisfy.aipo` | Subtipagem estrutural de interfaces e asserção explícita com `satisfy`. |
| `15_pipelines_and_trailing_blocks.aipo` | Aplicação de encadeamento `|>` e closures em blocos de chamada. |
| `16_ranges_repeat_each.aipo` | Iterações com intervalos `0..10`, laços `each` e controles `break`/`continue`. |
| `17_word_frequency.aipo` | Contagem de frequência de termos com divisão de texto e dicionários. |
| `18_small_statistics.aipo` | Módulo `math`, cálculos de média e desvios numéricos. |
| `19_unicode_normalization.aipo` | Normalização canônica NFC em fronteiras de texto Unicode. |
| `20_safe_numeric_boundaries.aipo` | Aritmética segura dentro do intervalo de precisão ±(2^53 - 1). |
| `21_closure_state.aipo` | Captura de estado compartilhado entre invocações de closures. |
| `22_small_budget_application.aipo` | Mini-aplicativo de controle financeiro com structs e transações. |
| `23_multi_module_application/` | Projeto multi-arquivos com separação em carteira e precificação. |
| `24_idiomatic_aipo_showcase.aipo` | Vitrine idiomática completa combinando todas as melhores features. |
| `25_snake_game.aipo` | Simulação completa do jogo da cobrinha (Snake) em terminal com structs, listas, colisões e pontuação. |
