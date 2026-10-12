---
title: "Funções locais"
description: Exemplo real do repositório, não um snippet hipotético.
---

# Funções locais

Este programa foi extraído de [`examples/02_local_functions.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/examples/02_local_functions.aipo). Você pode copiar o código e executá-lo com a CLI.

## Executar

```bash
cargo run -q -p aipo-cli -- run examples/02_local_functions.aipo
```

## Código completo

```aipo
# Escopo e funções locais, seguindo o canon ("Funções locais e closures — decidido"):
# o binding local existe quando a execução alcança a declaração, o nome é visível no
# próprio corpo para autorrecursão e um `var` capturado é compartilhado entre as
# chamadas da closure.
fn make_counter(label) {
    var count = 0

    fn bump() {
        count += 1
        return count
    }

    fn describe() {
        return f"{label}: {count}"
    }

    return { "bump": bump, "describe": describe }
}

let counter = make_counter("hits")
counter["bump"]()
counter["bump"]()
io.println(counter["describe"]())

# Autorrecursão: `helper` chama `helper` dentro do próprio corpo.
fn fact(n) {
    fn helper(k, acc) {
        if k <= 1 {
            return acc
        }
        return helper(k - 1, acc * k)
    }
    return helper(n, 1)
}
io.println(f"fact 5 = {fact(5)}")

# O mesmo `make_counter` executado duas vezes cria ambientes capturados independentes.
let other = make_counter("other")
other["bump"]()
io.println(other["describe"]())
io.println(counter["describe"]())
```

A [saída esperada](https://github.com/poppyTM/aipo-lang/blob/main/examples/02_local_functions.stdout) está versionada ao lado do exemplo. Não representa uma nova execução nesta revisão da documentação.

[Todos os exemplos](/examples/) · [Consultar a sintaxe](/manual/)
