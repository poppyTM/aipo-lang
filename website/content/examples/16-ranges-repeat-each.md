---
title: "Intervalos e laços"
description: Exemplo real do repositório Aipo.
---

# Intervalos e laços

Código extraído de [`examples/16_ranges_repeat_each.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/examples/16_ranges_repeat_each.aipo), sem adaptar a sintaxe às propostas futuras.

## Executar

```bash
cargo run -q -p aipo-cli -- run examples/16_ranges_repeat_each.aipo
```

## Código completo

```aipo
# Ranges are half-open values; `repeat` counts, `each` iterates with values
# (and optionally indices); `break` and `continue` shape the flow.
io.println(0..3)
var total = 0
repeat 4 as i {
    total = total + i
}
io.println(total)
each fruit in ["a", "b"] {
    io.println(fruit)
}
each index, value in [10, 20] {
    io.println(f"{index}:{value}")
}
var seen = []
each n in 0..10 {
    if n > 2 {
        break
    }
    if n == 1 {
        continue
    }
    seen.add(n)
}
io.println(seen)
```

A [saída esperada](https://github.com/poppyTM/aipo-lang/blob/main/examples/16_ranges_repeat_each.stdout) é uma fixture versionada. O site não afirma ter executado este exemplo na revisão atual.

[Todos os exemplos](/examples/) · [Manual](/manual/)
