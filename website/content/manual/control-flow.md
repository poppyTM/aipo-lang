---
title: "Condições, laços e match"
description: Manual prático baseado nos exemplos da implementação atual.
---

# Condições, laços e match

Aipo usa blocos com chaves `{ }` no código dos exemplos atuais. Você não precisa escrever parênteses ao redor da condição.

## Condição

```aipo
let age = 18
if age >= 18 {
    io.println("maior")
} else {
    io.println("menor")
}
```

## Repetir e percorrer

```aipo
var total = 0
repeat 4 as i {
    total = total + i
}
io.println(total)

each fruit in ["a", "b"] {
    io.println(fruit)
}
```

Fonte: [intervalos, `repeat`, `each`, `break` e `continue`](/examples/16-ranges-repeat-each).

## Quando escolher cada forma

| Sintaxe | Uso |
| --- | --- |
| `if` / `elif` / `else` | Tomar decisões |
| `if c then a else b` | Produzir valor em uma expressão |
| `while` | Repetir enquanto uma condição for verdadeira |
| `repeat n as i` | Repetir um número definido de vezes |
| `each x in lista` | Percorrer itens |
| `break` / `continue` | Interromper / pular iteração |
| `match` / `when` | Escolher entre padrões |

O [exemplo idiomático](/examples/24-idiomatic-aipo-showcase) mostra `match` em uma aplicação pequena. Não confunda `match` com uma cadeia de testes improvisados; escolha conforme a legibilidade.

**Pratique:** modifique `repeat 4` para `repeat 5` e explique o resultado.

[Próximo: funções](/manual/functions) · [Controle de fluxo em código real](/examples/01-fizzbuzz).
