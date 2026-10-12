---
title: "Funções, argumentos e closures"
description: Manual prático baseado nos exemplos da implementação atual.
---

# Funções, argumentos e closures

Funções são definidas com `fn`. Use-as para dar nome a uma operação pequena e reaproveitável. `return` devolve um valor.

## Função simples, defaults e argumentos nomeados

```aipo
fn greet(name, greeting = "hi") {
    return greeting + " " + name
}

io.println(greet("ana"))
io.println(greet("bob", greeting = "hey"))
io.println(greet(greeting = "yo", name = "cid"))
```

Fonte: [funções e argumentos nomeados](/examples/07-functions-defaults-named-args).

## Passar uma função como valor

```aipo
let items = [3, 1, 2]
let doubled = items.transform(fn (v) { return v + 1 })
io.println(doubled)
```

Uma *closure* pode captar variáveis do escopo externo. O [exemplo de closures com estado](/examples/21-closure-state) mostra a diferença entre capturar e atualizar estado. Funções locais e recursão estão em [funções locais](/examples/02-local-functions).

## O que observar

- Parâmetros opcionais podem ter valor padrão.
- Argumentos nomeados explicitam qual campo recebe cada valor.
- Contratos de tipos opcionais podem aparecer em parâmetros e retornos.
- Funções anônimas `fn (x) { ... }` permitem transformações curtas.
- O Aipo também tem lambdas `=>` em parte da superfície; consulte a [sintaxe do compilador](/reference/syntax) antes de usar em um backend alternativo.

**Pratique:** crie `greet("ana", greeting = "olá")`.

[Próximo: estruturas](/manual/structs) · [Exemplo completo](/examples/07-functions-defaults-named-args).
