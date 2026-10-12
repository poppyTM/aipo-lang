---
title: "Estatística simples"
description: Exemplo real do repositório Aipo.
---

# Estatística simples

Fonte original: [`examples/18_small_statistics.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/examples/18_small_statistics.aipo).

## Executar

```bash
cargo run -q -p aipo-cli -- run examples/18_small_statistics.aipo
```

## Código completo

```aipo
# Small statistics over a data set with `math` and loops.
let data = [4, 8, 15, 16, 23, 42]
var sum = 0
each n in data {
    sum = sum + n
}
let mean = sum / len(data)
io.println(sum)
io.println(mean)
io.println(math.min(3, 9))
io.println(math.max(3, 9))
io.println(math.sqrt(144))
io.println(data.filter(fn (x) { return x > mean }))
```

[Saída esperada registrada no repositório](https://github.com/poppyTM/aipo-lang/blob/main/examples/18_small_statistics.stdout). Esta página mostra a fonte real, não certifica uma nova execução no HEAD.

[Todos os exemplos](/examples/) · [Manual](/manual/)
