---
title: "Parâmetros e nomes"
description: Exemplo real do repositório, não um snippet hipotético.
---

# Parâmetros e nomes

Este programa foi extraído de [`examples/07_functions_defaults_named_args.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/examples/07_functions_defaults_named_args.aipo). Você pode copiar o código e executá-lo com a CLI.

## Executar

```bash
cargo run -q -p aipo-cli -- run examples/07_functions_defaults_named_args.aipo
```

## Código completo

```aipo
# Defaults are evaluated per call; named arguments may come in any order.
fn greet(name, greeting = "hi") {
    return greeting + " " + name
}

io.println(greet("ana"))
io.println(greet("bob", greeting = "hey"))
io.println(greet(greeting = "yo", name = "cid"))
```

A [saída esperada](https://github.com/poppyTM/aipo-lang/blob/main/examples/07_functions_defaults_named_args.stdout) está versionada ao lado do exemplo. Não representa uma nova execução nesta revisão da documentação.

[Todos os exemplos](/examples/) · [Consultar a sintaxe](/manual/)
