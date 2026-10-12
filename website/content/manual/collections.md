---
title: "Listas, dicionários e sequências"
description: Manual prático baseado nos exemplos da implementação atual.
---

# Listas, dicionários e sequências

Use uma **lista** quando a ordem e a posição importam; use um **dicionário** quando precisa associar chaves a valores. A linguagem também tem conjuntos e sequências avaliadas sob demanda.

## Lista e dicionário

```aipo
var xs = [5, 3, 9]
io.println(xs[0])
io.println(xs[-1])
io.println(xs[0..2])
xs.add(7)
io.println(xs.len())
io.println(xs.contains(3))
let user = { "name": "ana", "level": 3 }
io.println(user["name"])
io.println(user.keys())
io.println(user.get("missing"))
```

Fonte: [exemplo completo](/examples/08-lists-dicts-and-slices).

## Recursos disponíveis

| Recurso | Quando usar |
| --- | --- |
| `[a, b, c]` | Lista ordenada |
| `{ "a": 1 }` | Dicionário |
| `xs[-1]` | Último elemento |
| `xs[0..2]` | Fatia (*slice*) |
| `.add(x)`, `.contains(x)` | Adição e consulta |
| `.transform(fn (...))` | Transformação de itens |
| `.filter(fn (...))` | Seleção de itens |
| `Set` | Conjunto sem duplicatas, ordem de inserção |
| `Sequence` | Pipeline lazy |

```aipo
let items = [3, 1, 2]
let doubled = items.transform(fn (v) { return v + 1 })
io.println(doubled)
io.println(items.filter(fn (v) { return v > 2 }))
```

Este segundo trecho aparece em [pipelines e blocos](/examples/15-pipelines-and-trailing-blocks).

**Pratique:** selecione apenas números maiores que 2 na primeira lista.

[Próximo: fluxo](/manual/control-flow) · [Mais coleções](/manual/standard-library).
