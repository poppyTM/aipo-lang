---
title: "Limites numéricos"
description: Exemplo real do repositório Aipo.
---

# Limites numéricos

Fonte original: [`examples/20_safe_numeric_boundaries.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/examples/20_safe_numeric_boundaries.aipo).

## Executar

```bash
cargo run -q -p aipo-cli -- run examples/20_safe_numeric_boundaries.aipo
```

## Código completo

```aipo
# Boundaries stay inside ±(2^53 − 1) and finite floats: out-of-range
# conversions are recoverable `Failure`, handled here with `or_else`.
io.println(9007199254740991)
io.println(Byte(255))
io.println(Byte(300) or_else - 1)
io.println(Int("12") or_else - 1)
io.println(Int("nope") or_else - 1)
io.println(math.clamp(5, 0, 10))
io.println(math.clamp(5, 3, 0) or_else - 1)
```

[Saída esperada registrada no repositório](https://github.com/poppyTM/aipo-lang/blob/main/examples/20_safe_numeric_boundaries.stdout). Esta página mostra a fonte real, não certifica uma nova execução no HEAD.

[Todos os exemplos](/examples/) · [Manual](/manual/)
