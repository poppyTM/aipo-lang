---
title: "Números, limites e Bytes"
description: Manual prático baseado nos exemplos da implementação atual.
---

# Números, limites e Bytes

Aipo tem `Int`, `Float` e `Byte`. Há regras de segurança para conversões e limites numéricos. Prefira conversões explícitas quando um valor vier de texto.

## Conversão segura

```aipo
let number = Int("33") or_else -1
io.println(number)
```

`or_else` fornece um valor alternativo quando a conversão produz uma falha recuperável. Veja o [exemplo de recuperação](/examples/11-failures-or-else-attempt).

## Buffers binários

```aipo
let data = Bytes(8)
io.println(len(data))
io.println(data[0])
io.println(len(data[0..3]))
```

Este programa aparece em [`10_bytes.aipo`](/examples/10-bytes). `Bytes(8)` cria um buffer com oito bytes inicializados em zero. Para leitura, escrita e codificação, consulte [biblioteca padrão](/manual/standard-library).

## Cuidados

- Uma divisão por zero deve ser tratada como falha, não ignorada.
- Não suponha que toda conversão de `Float` para `Int` é válida.
- `/` e `//` têm propósitos diferentes: verifique seus operandos e os limites.
- Em dados vindos de arquivos ou host, sempre valide o tamanho e a origem antes de converter.

**Pratique:** altere `Bytes(8)` para `Bytes(4)` e confira o valor de `len(data)`.

[Próximo: coleções](/manual/collections) · [Limites numéricos](/examples/20-safe-numeric-boundaries).
