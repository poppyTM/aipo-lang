---
title: "Falhas e recuperação"
description: Exemplo real do repositório Aipo.
---

# Falhas e recuperação

Código extraído de [`examples/11_failures_or_else_attempt.aipo`](https://github.com/poppyTM/aipo-lang/blob/main/examples/11_failures_or_else_attempt.aipo), sem adaptar a sintaxe às propostas futuras.

## Executar

```bash
cargo run -q -p aipo-cli -- run examples/11_failures_or_else_attempt.aipo
```

## Código completo

```aipo
# Recoverable problems are `Failure` values: `or_else` supplies a fallback
# and `attempt` catches with access to `err.message`.
fn parse_age(text) {
    let age = Int(text) or_else - 1
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

A [saída esperada](https://github.com/poppyTM/aipo-lang/blob/main/examples/11_failures_or_else_attempt.stdout) é uma fixture versionada. O site não afirma ter executado este exemplo na revisão atual.

[Todos os exemplos](/examples/) · [Manual](/manual/)
