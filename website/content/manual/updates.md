---
title: "Atualização funcional com `with`"
description: Manual atualizado com fontes da linguagem Aipo.
---

# Atualização funcional com `with`

`with` permite produzir **uma nova struct baseada em outra**, alterando somente os campos escolhidos. O objeto original permanece disponível sem essas mudanças.

## Exemplo

```aipo
struct Point {
    var x
    var y
    var z
}

let origin = Point{x = 0, y = 0, z = 0}
let moved = origin with { x: 3, z: 9 }
io.println(String(origin.x))
io.println(String(moved.x))
```

O código é um recorte do programa de conformidade [`30_struct_update_with.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/docs/conformance/programs/30_struct_update_with.aipo).

## Como pensar sobre `with`

- Os campos não citados mantêm seus valores.
- As alterações não dependem da ordem escrita no bloco.
- O valor-base pode ser uma variável ou uma expressão que produz uma struct.
- Campos declarados `fixed` não podem ser substituídos por `with`.
- `with` é para uma **cópia modificada**; `var self` é para um método que altera uma instância.

**Quando preferir:** use `with` para estados, configurações e transformações em que criar um valor novo deixa a intenção mais explícita.

[Structs e métodos](/manual/structs) · [Rollback](/manual/failures).
