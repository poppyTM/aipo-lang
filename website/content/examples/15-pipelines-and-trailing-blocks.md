---
title: "Pipelines e blocos"
description: Exemplo real do repositório Aipo.
---

# Pipelines e blocos

Código extraído de [`examples/15_pipelines_and_trailing_blocks.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/examples/15_pipelines_and_trailing_blocks.aipo), sem adaptar a sintaxe às propostas futuras.

## Executar

```bash
cargo run -q -p aipo-cli -- run examples/15_pipelines_and_trailing_blocks.aipo
```

## Código completo

```aipo
# `|>` pipes a value into a call; a trailing `do { … }` block passes a
# closure as the final argument.
let items = [3, 1, 2]
let doubled = items.transform(fn (v) { return v + 1 })
io.println(doubled)
io.println(items.transform do (v) {
        return v * 10
    })
io.println(items.filter(fn (v) { return v > 2 }))
io.println([1, 2, 3] |> len)
```

A [saída esperada](https://github.com/poppyTM/aipo-lang/blob/main/examples/15_pipelines_and_trailing_blocks.stdout) é uma fixture versionada. O site não afirma ter executado este exemplo na revisão atual.

[Todos os exemplos](/examples/) · [Manual](/manual/)
