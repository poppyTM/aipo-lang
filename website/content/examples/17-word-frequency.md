---
title: "Frequência de palavras"
description: Exemplo real do repositório Aipo.
---

# Frequência de palavras

Fonte original: [`examples/17_word_frequency.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/examples/17_word_frequency.aipo).

## Executar

```bash
cargo run -q -p aipo-cli -- run examples/17_word_frequency.aipo
```

## Código completo

```aipo
# Word frequency with split, dict counting and ordered keys.
let text = "ana bob ana cid bob ana"
let counts = {}
each word in text.split(" ") {
    if counts.has(word) {
        counts[word] = counts[word] + 1
    } else {
        counts[word] = 1
    }
}
each word in counts.keys() {
    io.println(f"{word}: {counts[word]}")
}
```

[Saída esperada registrada no repositório](https://github.com/poppyTM/aipo-lang/blob/main/examples/17_word_frequency.stdout). Esta página mostra a fonte real, não certifica uma nova execução no HEAD.

[Todos os exemplos](/examples/) · [Manual](/manual/)
