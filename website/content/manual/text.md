---
title: "Texto e Unicode"
description: Manual prático baseado nos exemplos da implementação atual.
---

# Texto e Unicode

Strings no Aipo são UTF-8; suas operações lidam com texto e não com bytes arbitrários. **Use `f"..."` para inserir valores em mensagens.**

## Na prática

```aipo
let name = "ana"
io.println(f"hi {name}!")
io.println("a,B,c".split(","))
io.println("-".join(["a", "b"]))
io.println("aaa".replace("a", "b"))
io.println("héllo".upper())
io.println("café".len())
io.println("🎉".len())
```

Esse trecho é do [exemplo de texto e Unicode](/examples/09-strings-unicode-and-formatting). A biblioteca também inclui `.lower()`, `.trim()` e formatação baseada em um dicionário de nomes.

## O que precisa lembrar

- `"texto"`: string literal comum.
- `f"Olá, {nome}"`: interpolação.
- `.split(",")`: quebra um texto em partes.
- `.join(lista)`: combina textos.
- `.replace(a, b)`: substitui ocorrências.
- `.len()`: conta comprimento textual conforme o contrato da linguagem, não o tamanho de um arquivo codificado em UTF-8.
- `Bytes` é o tipo apropriado para bytes brutos; não confundir índices textuais com posições de bytes.

A normalização Unicode (NFC) também é abordada no [exemplo de normalização](/examples/19-unicode-normalization).

**Pratique:** crie uma mensagem interpolando dois valores e aplique `.upper()` a ela.

[Próximo: números](/manual/numbers) · [Biblioteca de texto](/manual/standard-library#texto-e-colecoes).
