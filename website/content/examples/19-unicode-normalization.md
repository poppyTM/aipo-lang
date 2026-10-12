---
title: "Normalização Unicode"
description: Exemplo real do repositório Aipo.
---

# Normalização Unicode

Fonte original: [`examples/19_unicode_normalization.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/examples/19_unicode_normalization.aipo).

## Executar

```bash
cargo run -q -p aipo-cli -- run examples/19_unicode_normalization.aipo
```

## Código completo

```aipo
# Unicode text stays normalized across operations: an escaped pair, a case
# change and a reversal all come back composed.
io.println("caf\u{e9}")
io.println("CAF\u{e9}".lower())
io.println("h\u{e9}llo".reverse())
io.println("na\u{ef}ve".capitalize())
io.println("a,b".split(",").len())
```

[Saída esperada registrada no repositório](https://github.com/poppyTM/aipo-lang/blob/main/examples/19_unicode_normalization.stdout). Esta página mostra a fonte real, não certifica uma nova execução no HEAD.

[Todos os exemplos](/examples/) · [Manual](/manual/)
