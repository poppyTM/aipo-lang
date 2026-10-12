---
title: "Falhas, recuperação e rollback"
description: "Referência de uso organizada por tarefas e comportamento do Aipo."
---

# Falhas, recuperação e rollback

O Aipo trata problemas recuperáveis com valores `Failure`. Existem duas formas úteis: **fornecer um valor alternativo** com `or_else` ou **tratar uma falha em um bloco** com `attempt / failed`.

## Duas formas de recuperar

```aipo
fn parse_age(text) {
    let age = Int(text) or_else -1
    if age < 0 {
        return fail (f"not an age: {text}")
    }
    return age
}

io.println(parse_age("33"))
io.println(parse_age("nope") or_else "fallback")

attempt {
    io.println(parse_age("xyz"))
} failed err {
    io.println(f"caught: {err.message}")
}
```

Fonte: [exemplo de falhas](/examples/11-failures-or-else-attempt).

## Invariantes de objeto

O método especial `Tipo:invariant` permite definir uma condição que deve permanecer verdadeira em mutações de instâncias publicadas.

```aipo
struct Span {
    var lo
    var hi
}

Span:invariant {
    self.lo < self.hi
}
```

No [exemplo completo de rollback](/examples/13-mutation-and-rollback), uma mudança que viola a condição é rejeitada e os campos são restaurados. A semântica precisa ser avaliada conforme o backend executado; não prometa paridade automática com RegVM ou Wasm.

**Pratique:** experimente um texto inválido em `Int(text)` e troque a mensagem de recuperação.

[Interfaces e contratos](/manual/interfaces) · [Diagnósticos](/reference/diagnostics).
