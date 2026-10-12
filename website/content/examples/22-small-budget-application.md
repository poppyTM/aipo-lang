---
title: "Aplicação de orçamento"
description: Exemplo real do repositório Aipo.
---

# Aplicação de orçamento

Fonte original: [`examples/22_small_budget_application.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/examples/22_small_budget_application.aipo).

## Executar

```bash
cargo run -q -p aipo-cli -- run examples/22_small_budget_application.aipo
```

## Código completo

```aipo
# A tiny budget ledger: structs carry the rules, `attempt` carries the errors.
struct Entry {
    label
    var amount
}

Entry:init(var self, label, amount) {
    self.label = label
    self.amount = amount
}

Entry:invariant {
    self.amount >= 0
}

var ledger = []
ledger.add(Entry{ label: "seed", amount: 100 })
ledger.add(Entry{ label: "book", amount: 30 })

var spent = 0
each entry in ledger {
    spent = spent + entry.amount
}
io.println(spent)
io.println(len(ledger))

attempt {
    let bad = Entry{ label: "oops", amount: 10 }
    bad.amount = -5
    io.println("unreachable")
} failed err {
    io.println(f"rejected: {err.message}")
}
io.println(len(ledger))
```

[Saída esperada registrada no repositório](https://github.com/poppyTM/aipo-lang/blob/main/examples/22_small_budget_application.stdout). Esta página mostra a fonte real, não certifica uma nova execução no HEAD.

[Todos os exemplos](/examples/) · [Manual](/manual/)
