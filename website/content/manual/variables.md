---
title: "Variáveis e valores"
description: Manual prático baseado nos exemplos da implementação atual.
---

# Variáveis e valores

**Use `let` quando o valor não muda; use `var` quando precisa reatribuir.** Esta distinção aparece em praticamente todo programa Aipo.

## O menor exemplo útil

```aipo
let name = "ana"
var score = 10
score = score + 5
io.println(name)
io.println(score)
io.println(none)
io.println(true)
```

O exemplo vem de [`06_variables_and_values.aipo`](/examples/06-variables-and-values) e usa uma variável imutável (`name`) e uma mutável (`score`).

## Valores comuns

| Tipo ou valor | Exemplo | Para quê |
| --- | --- | --- |
| Inteiro | `10` | Contagens, índices |
| Ponto flutuante | `2.5` | Medidas |
| Texto | `"ana"` | Strings Unicode |
| Booleano | `true`, `false` | Condições |
| Ausência | `none` | Valor ausente |
| Byte | `Byte(65)` | Unidade binária |

**Atenção:** reatribuir com `name = "outra"` não funciona após `let`. Valores e coleções têm regras próprias de mutação; não conclua que uma lista é imutável apenas porque seu *binding* usa `let`.

**Pratique:** altere `score` para começar em 0, some 2 duas vezes e imprima o resultado.

[Próximo: texto](/manual/text) · [Exemplo completo](/examples/06-variables-and-values).
