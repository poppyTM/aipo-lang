---
title: "Operadores e recursos de expressão"
description: Manual atualizado com fontes da linguagem Aipo.
---

# Operadores e recursos de expressão

O Aipo oferece operadores aritméticos, comparações, expressões condicionais, encadeamentos e propagação de falhas.

## Referência rápida

| Escrita | Significado |
| --- | --- |
| `+`, `-`, `*`, `/` | Operações aritméticas |
| `//` | Divisão inteira; **não é comentário** |
| `and`, `or`, `not` | Lógica booleana |
| `==`, `!=`, `<`, `<=`, `>`, `>=` | Comparações |
| `0..10` | Intervalo de valores |
| `value or_else fallback` | Valor alternativo se ocorrer `Failure` |
| `expr?` | Propagar uma `Failure` |
| `value |> fn` | Pipeline com chamada |
| `a with { field: value }` | Cópia funcional de struct |

## Condição que produz valor

```aipo
var flag = true
io.println(if flag then 10 else 20)
flag = false
io.println(if flag then 10 else 20)
```

Fonte: [`34_conditional_values.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/docs/conformance/programs/34_conditional_values.aipo).

## Bases de literais

```aipo
io.println(0xFF)
io.println(0b1010)
io.println(0o17)
io.println(1_000_000)
io.println(1.5e-3)
```

Fonte: [`27_numeric_literal_bases.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/docs/conformance/programs/27_numeric_literal_bases.aipo).

## Propagar falhas com `?`

```aipo
fn double_age(text) {
    let age = parse_age(text)?
    return age * 2
}
```

Nesse trecho `parse_age` é uma função externa definida no [exemplo completo de propagação](https://github.com/poppyTM/aipo-lang/blob/main/docs/conformance/programs/29_try_propagation_operator.aipo). `?` devolve o valor ou propaga a falha. Não confunda com `Type?`, que significa tipo opcional.

**Pratique:** combine `or_else` com uma conversão numérica.

[Voltar ao manual](/manual/) · [Falhas](/manual/failures).
