---
title: "Condições e repetições"
description: Exemplo real do repositório, não um snippet hipotético.
---

# Condições e repetições

Este programa foi extraído de [`examples/01_fizzbuzz.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/examples/01_fizzbuzz.aipo). Você pode copiar o código e executá-lo com a CLI.

## Executar

```bash
cargo run -q -p aipo-cli -- run examples/01_fizzbuzz.aipo
```

## Código completo

```aipo
# FizzBuzz até 15, escrito no estilo canônico do Aipo: funções puras para a
# classificação e `repeat` para a contagem.
fn classify(n) {
    if n % 15 == 0 {
        return "FizzBuzz"
    } elif n % 3 == 0 {
        return "Fizz"
    } elif n % 5 == 0 {
        return "Buzz"
    }
    return String(n)
}

repeat 15 as i {
    io.println(classify(i + 1))
}
```

A [saída esperada](https://github.com/poppyTM/aipo-lang/blob/main/examples/01_fizzbuzz.stdout) está versionada ao lado do exemplo. Não representa uma nova execução nesta revisão da documentação.

[Todos os exemplos](/examples/) · [Consultar a sintaxe](/manual/)
