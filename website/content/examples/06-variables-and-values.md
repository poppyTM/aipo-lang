---
title: "Valores e variáveis"
description: Exemplo real do repositório, não um snippet hipotético.
---

# Valores e variáveis

Este programa foi extraído de [`examples/06_variables_and_values.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/examples/06_variables_and_values.aipo). Você pode copiar o código e executá-lo com a CLI.

## Executar

```bash
cargo run -q -p aipo-cli -- run examples/06_variables_and_values.aipo
```

## Código completo

```aipo
# Variables hold values; `var` allows reassignment, `let` does not.
let name = "ana"
var score = 10
score = score + 5
io.println(name)
io.println(score)
io.println(none)
io.println(true)
io.println(2.5)
io.println(Byte(65))
```

A [saída esperada](https://github.com/poppyTM/aipo-lang/blob/main/examples/06_variables_and_values.stdout) está versionada ao lado do exemplo. Não representa uma nova execução nesta revisão da documentação.

[Todos os exemplos](/examples/) · [Consultar a sintaxe](/manual/)
