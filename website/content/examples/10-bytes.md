---
title: "Bytes e buffers"
description: Exemplo real do repositório Aipo.
---

# Bytes e buffers

Código extraído de [`examples/10_bytes.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/examples/10_bytes.aipo), sem adaptar a sintaxe às propostas futuras.

## Executar

```bash
cargo run -q -p aipo-cli -- run examples/10_bytes.aipo
```

## Código completo

```aipo
# `Bytes(count)` allocates a zeroed block; indexing yields `Byte`.
let data = Bytes(8)
io.println(len(data))
io.println(data[0])
io.println(len(data[0..3]))
```

A [saída esperada](https://github.com/poppyTM/aipo-lang/blob/main/examples/10_bytes.stdout) é uma fixture versionada. O site não afirma ter executado este exemplo na revisão atual.

[Todos os exemplos](/examples/) · [Manual](/manual/)
