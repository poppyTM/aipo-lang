---
title: "Listas e dicionários"
description: Exemplo real do repositório, não um snippet hipotético.
---

# Listas e dicionários

Este programa foi extraído de [`examples/08_lists_dicts_and_slices.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/examples/08_lists_dicts_and_slices.aipo). Você pode copiar o código e executá-lo com a CLI.

## Executar

```bash
cargo run -q -p aipo-cli -- run examples/08_lists_dicts_and_slices.aipo
```

## Código completo

```aipo
# Lists keep order; dicts keep insertion order. Negative indices count
# from the end; slices tolerate out-of-range bounds.
var xs = [5, 3, 9]
io.println(xs[0])
io.println(xs[-1])
io.println(xs[0..2])
io.println(xs[1..99])
xs.add(7)
io.println(xs.len())
io.println(xs.contains(3))
let user = { "name": "ana", "level": 3 }
io.println(user["name"])
io.println(user.keys())
io.println(user.get("missing"))
```

A [saída esperada](https://github.com/poppyTM/aipo-lang/blob/main/examples/08_lists_dicts_and_slices.stdout) está versionada ao lado do exemplo. Não representa uma nova execução nesta revisão da documentação.

[Todos os exemplos](/examples/) · [Consultar a sintaxe](/manual/)
