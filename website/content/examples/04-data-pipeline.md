---
title: "Pipeline de dados"
description: Exemplo real do repositório, não um snippet hipotético.
---

# Pipeline de dados

Este programa foi extraído de [`examples/04_data_pipeline.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/examples/04_data_pipeline.aipo). Você pode copiar o código e executá-lo com a CLI.

## Executar

```bash
cargo run -q -p aipo-cli -- run examples/04_data_pipeline.aipo
```

## Código completo

```aipo
# Um pipeline de dados completo: `struct` + `impl`, coleções com métodos,
# pipeline `|>`, trailing block `do { ... }` e recuperação de falhas.
struct Order {
    id
    customer
    total
}

Order:summary(self) -> String {
    return f"#{self.id} {self.customer} {self.total}"
}

var orders = []
orders.add(Order{ id: 1, customer: "ana", total: 120 })
orders.add(Order{ id: 2, customer: "bruno", total: 80 })
orders.add(Order{ id: 3, customer: "ana", total: 200 })

# `|>` é açúcar de chamada: insere o valor à esquerda como primeiro argumento.
fn totals_for(list, who) {
    let mine = list.filter(fn (o) { return o.customer == who })
    return mine.transform(fn (o) { return o.total })
}

var ana_total = 0
each total in orders |> totals_for("ana") {
    ana_total = ana_total + total
}
io.println(f"ana total: {ana_total}")

each order in orders {
    io.println(order.summary())
}

# Trailing block: a closure é passada como último argumento da chamada.
fn with_prefix(prefix, block) {
    return block() + prefix
}
io.println(with_prefix("!") do {
        return "hello"
    })

# Falhas são valores: `or_else` para fallback local, `attempt` para o bloco.
fn parse_total(text) {
    let value = Int(text) or_else fail (f"invalid total: {text}")
    return value
}

io.println(parse_total("42"))
attempt {
    io.println(parse_total("oops"))
} failed err {
    io.println(f"rejected: {err.message}")
}
```

A [saída esperada](https://github.com/poppyTM/aipo-lang/blob/main/examples/04_data_pipeline.stdout) está versionada ao lado do exemplo. Não representa uma nova execução nesta revisão da documentação.

[Todos os exemplos](/examples/) · [Consultar a sintaxe](/manual/)
