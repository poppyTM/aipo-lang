---
title: "Unicode e strings"
description: Exemplo real do repositório, não um snippet hipotético.
---

# Unicode e strings

Este programa foi extraído de [`examples/09_strings_unicode_and_formatting.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/examples/09_strings_unicode_and_formatting.aipo). Você pode copiar o código e executá-lo com a CLI.

## Executar

```bash
cargo run -q -p aipo-cli -- run examples/09_strings_unicode_and_formatting.aipo
```

## Código completo

```aipo
# Text is Unicode code points, always NFC. Interpolation lowers to `+`.
let name = "ana"
io.println(f"hi {name}!")
io.println("a,B,c".split(","))
io.println("-".join(["a", "b"]))
io.println("aaa".replace("a", "b"))
io.println("héllo".upper())
io.println("café".len())
io.println("🎉".len())
io.println("Name: {name}".format({ "name": name }))
```

A [saída esperada](https://github.com/poppyTM/aipo-lang/blob/main/examples/09_strings_unicode_and_formatting.stdout) está versionada ao lado do exemplo. Não representa uma nova execução nesta revisão da documentação.

[Todos os exemplos](/examples/) · [Consultar a sintaxe](/manual/)
